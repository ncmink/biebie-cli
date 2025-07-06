use std::fs;
use std::sync::Arc;
use std::time::Instant;
use std::collections::HashMap;

use walkdir::WalkDir;
use mime_guess::from_path;
use blake3::hash;
use dashmap::DashMap;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::{prelude::*, ThreadPoolBuilder};
use memmap2::Mmap;

use crate::uploader::FileMeta;

const PROGRESS_BAR_TEMPLATE: &str = "[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} - {msg}";
const PROGRESS_CHARS: &str = "##-";
const LARGE_FILE_THRESHOLD: u64 = 10 * 1024 * 1024; // 10MB - reduced for better memory mapping usage
const VERY_LARGE_FILE_THRESHOLD: u64 = 100 * 1024 * 1024; // 100MB

/// Directory processing unit for hierarchical scanning
#[derive(Debug)]
struct DirBatch {
    path: String,
    files: Vec<walkdir::DirEntry>,
    depth: usize,
}

/// Scans a folder recursively and returns metadata for all unique files
pub fn scan_folder(folder: &str) -> Vec<FileMeta> {
    print_system_info();
    
    let start_time = Instant::now();
    
    // Create custom ThreadPool for optimal performance
    let thread_count = determine_optimal_thread_count();
    let custom_pool = ThreadPoolBuilder::new()
        .num_threads(thread_count)
        .thread_name(|i| format!("scanner-{}", i))
        .build()
        .expect("Failed to create custom thread pool");
    
    println!("Created custom ThreadPool with {} threads", thread_count);
    
    // Discover and organize files by directory hierarchy
    let dir_batches = discover_nested_structure(folder);
    
    if dir_batches.is_empty() {
        println!("No files found in folder: {}", folder);
        return Vec::new();
    }
    
    let total_files: usize = dir_batches.iter().map(|batch| batch.files.len()).sum();
    println!("Found {} files in {} directories to process", total_files, dir_batches.len());
    
    let progress_bar = ProgressBar::new(total_files as u64);
    progress_bar.set_style(
        ProgressStyle::with_template(PROGRESS_BAR_TEMPLATE)
            .unwrap()
            .progress_chars(PROGRESS_CHARS),
    );
    
    let duplicate_tracker = Arc::new(DashMap::new());
    
    // Process using custom ThreadPool with Rayon scope
    let results = custom_pool.install(|| {
        process_nested_folders_with_scope(&dir_batches, &progress_bar, &duplicate_tracker)
    });
    
    progress_bar.finish_with_message("Scan completed!");
    print_completion_stats(&start_time, results.len());
    
    results
}

/// Determines optimal thread count based on system capabilities and workload
fn determine_optimal_thread_count() -> usize {
    let cpu_count = num_cpus::get();
    let physical_cores = num_cpus::get_physical();
    
    // For I/O intensive work like file scanning, we can use more threads than CPU cores
    // But limit to avoid too much context switching
    let optimal = std::cmp::min(cpu_count * 2, physical_cores * 4);
    std::cmp::max(optimal, 4) // Minimum 4 threads
}

/// Discovers and organizes files into hierarchical directory batches
fn discover_nested_structure(folder: &str) -> Vec<DirBatch> {
    println!("Stage 1: Discovering nested folder structure...");
    
    // Group files by their parent directory
    let mut dir_file_map: HashMap<String, Vec<walkdir::DirEntry>> = HashMap::new();
    let mut dir_depths: HashMap<String, usize> = HashMap::new();
    
    for entry in WalkDir::new(folder).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && should_process_file(&entry) {
            let parent_path = entry.path().parent()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| folder.to_string());
            
            let depth = entry.path().components().count();
            
            dir_file_map.entry(parent_path.clone())
                .or_insert_with(Vec::new)
                .push(entry);
            
            dir_depths.insert(parent_path, depth);
        }
    }
    
    // Convert to DirBatch and sort by depth (deeper directories first)
    let mut batches: Vec<DirBatch> = dir_file_map
        .into_iter()
        .map(|(path, files)| DirBatch {
            depth: dir_depths.get(&path).copied().unwrap_or(0),
            path,
            files,
        })
        .collect();
    
    // Sort by depth (process deeper folders first for better cache locality)
    batches.sort_by_key(|batch| std::cmp::Reverse(batch.depth));
    
    println!("Organized into {} directory batches", batches.len());
    batches
}

/// Process nested folders using Rayon scope for optimal thread management
fn process_nested_folders_with_scope(
    dir_batches: &[DirBatch],
    progress_bar: &ProgressBar,
    seen_hashes: &Arc<DashMap<String, String>>,
) -> Vec<FileMeta> {
    println!("Stage 2: Processing files with custom ThreadPool and scope...");
    
    let results = Arc::new(DashMap::new());
    
    // Use Rayon scope to process directory batches in parallel
    rayon::scope(|scope| {
        for (batch_idx, dir_batch) in dir_batches.iter().enumerate() {
            let results_ref = Arc::clone(&results);
            let seen_hashes_ref = Arc::clone(seen_hashes);
            let progress_bar_ref = progress_bar;
            
            // Spawn each directory batch in parallel scope
            scope.spawn(move |_nested_scope| {
                process_directory_batch_scoped(
                    dir_batch,
                    batch_idx,
                    results_ref,
                    seen_hashes_ref,
                    progress_bar_ref,
                );
            });
        }
    });
    
    // Collect results from all batches
    let mut final_results: Vec<FileMeta> = results
        .iter()
        .map(|entry| entry.value().clone())
        .collect();
    
    // Sort by filename for consistent output
    final_results.sort_by(|a, b| a.filename.cmp(&b.filename));
    
    final_results
}

/// Process a single directory batch using nested scope for file-level parallelism
fn process_directory_batch_scoped(
    dir_batch: &DirBatch,
    batch_idx: usize,
    results: Arc<DashMap<String, FileMeta>>,
    seen_hashes: Arc<DashMap<String, String>>,
    progress_bar: &ProgressBar,
) {
    // Log directory processing (using the path field)
    if dir_batch.files.len() > 10 {
        progress_bar.set_message(format!("Processing directory: {} ({} files)", 
                                        dir_batch.path, dir_batch.files.len()));
    }
    
    // ✅ Process files without individual progress updates
    let batch_results: Vec<Option<FileMeta>> = dir_batch.files
        .par_iter()
        .map(|entry| {
            let file_meta = process_single_file_ultra_fast(entry)?;
            // ❌ ลบ progress_bar.inc(1) ออก - ไม่ให้ thread แย่งกัน
            Some(file_meta)
        })
        .collect();
    
    // ✅ Bulk update progress bar ครั้งเดียวหลังจบ directory batch
    let processed_count = batch_results.iter().filter(|r| r.is_some()).count();
    progress_bar.inc(processed_count as u64);
    
    // Deduplicate and store results
    for file_meta_opt in batch_results {
        if let Some(file_meta) = file_meta_opt {
            // Check for duplicates
            if seen_hashes.contains_key(&file_meta.hash) {
                continue; // Skip duplicate
            }
            
            // Store unique file
            seen_hashes.insert(file_meta.hash.clone(), file_meta.filename.clone());
            let key = format!("{}_{}", batch_idx, file_meta.filename);
            results.insert(key, file_meta);
        }
    }
}

/// Early filtering to skip files we don't want to process
fn should_process_file(entry: &walkdir::DirEntry) -> bool {
    let path = entry.path();
    
    // Skip hidden files and system files
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.') {
            return false;
        }
    }
    
    // Skip very small files (likely not media)
    if let Ok(metadata) = entry.metadata() {
        if metadata.len() < 1024 { // Skip files smaller than 1KB
            return false;
        }
    }
    
    true
}

/// Ultra-optimized single file processing with memory mapping and reduced allocations
fn process_single_file_ultra_fast(entry: &walkdir::DirEntry) -> Option<FileMeta> {
    let path = entry.path();
    
    // Get metadata once - batch system calls
    let metadata = path.metadata().ok()?;
    let file_size = metadata.len();
    
    // Fast MIME type detection using file extension first
    let mime_type = from_path(path).first_or_octet_stream();
    let mime_str = mime_type.essence_str(); // More efficient than to_string()
    
    // Ultra-fast hash computation strategy based on file size
    let file_hash = if file_size > VERY_LARGE_FILE_THRESHOLD {
        // For very large files, use sampling hash (much faster)
        compute_sample_hash(path, file_size).ok()?
    } else if file_size > LARGE_FILE_THRESHOLD {
        // Memory map for large files
        compute_hash_mmap(path).ok()?
    } else {
        // Direct read for small files
        let file_content = fs::read(path).ok()?;
        hash(&file_content).to_hex().to_string()
    };
    
    // Efficient file type determination
    let file_type = determine_file_type_fast(mime_str);
    
    // Minimize allocations
    Some(FileMeta {
        filename: path.display().to_string(),
        folder: path.parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        size: file_size,
        mime: mime_str.to_string(),
        hash: file_hash,
        filetype: file_type,
    })
}

/// Memory-mapped hash computation for large files
fn compute_hash_mmap(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let file = fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let hash_result = hash(&mmap);
    Ok(hash_result.to_hex().to_string())
}

/// Sample-based hash for very large files with kernel optimization hints
fn compute_sample_hash(path: &std::path::Path, file_size: u64) -> Result<String, std::io::Error> {
    use std::fs::File;
    
    let file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    
    // Sample configuration
    let sample_size = 64 * 1024; // 64KB samples
    let mut buffer = vec![0; sample_size];
    
    // Calculate sample positions
    let beginning_offset = 0;
    let middle_offset = file_size / 2;
    let end_offset = file_size.saturating_sub(sample_size as u64);
    
    // Platform-specific implementation with kernel optimization hints
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::FileExt;
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        
        // ให้ kernel รู้ว่าเราจะอ่านแบบ random access
        unsafe {
            libc::posix_fadvise(fd, 0, file_size as libc::off_t, libc::POSIX_FADV_RANDOM);
        }
        
        // Hint kernel ว่าเราจะอ่านช่วงนี้เร็วๆ นี้ (preload to cache)
        unsafe {
            // Beginning sample
            libc::posix_fadvise(fd, beginning_offset as libc::off_t, sample_size as libc::off_t, libc::POSIX_FADV_WILLNEED);
            
            // Middle sample
            if file_size > sample_size as u64 * 2 {
                libc::posix_fadvise(fd, middle_offset as libc::off_t, sample_size as libc::off_t, libc::POSIX_FADV_WILLNEED);
            }
            
            // End sample
            if file_size > sample_size as u64 {
                libc::posix_fadvise(fd, end_offset as libc::off_t, sample_size as libc::off_t, libc::POSIX_FADV_WILLNEED);
            }
        }
        
        // อ่านข้อมูลหลังจาก hint kernel แล้ว
        
        // Beginning - อ่านจากตำแหน่ง 0
        let bytes_read = file.read_at(&mut buffer, beginning_offset)?;
        hasher.update(&buffer[..bytes_read]);
        
        // Middle - อ่านจากตำแหน่งกลางไฟล์
        if file_size > sample_size as u64 * 2 {
            let bytes_read = file.read_at(&mut buffer, middle_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
        
        // End - อ่านจากท้ายไฟล์
        if file_size > sample_size as u64 {
            let bytes_read = file.read_at(&mut buffer, end_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
        
        // บอก kernel ว่าเราใช้ข้อมูลเสร็จแล้ว (สามารถ evict จาก cache ได้)
        unsafe {
            libc::posix_fadvise(fd, beginning_offset as libc::off_t, sample_size as libc::off_t, libc::POSIX_FADV_DONTNEED);
            if file_size > sample_size as u64 * 2 {
                libc::posix_fadvise(fd, middle_offset as libc::off_t, sample_size as libc::off_t, libc::POSIX_FADV_DONTNEED);
            }
            if file_size > sample_size as u64 {
                libc::posix_fadvise(fd, end_offset as libc::off_t, sample_size as libc::off_t, libc::POSIX_FADV_DONTNEED);
            }
        }
    }
    
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::FileExt;
        
        // macOS doesn't have posix_fadvise, so we just read without hints
        // Beginning - อ่านจากตำแหน่ง 0
        let bytes_read = file.read_at(&mut buffer, beginning_offset)?;
        hasher.update(&buffer[..bytes_read]);
        
        // Middle - อ่านจากตำแหน่งกลางไฟล์
        if file_size > sample_size as u64 * 2 {
            let bytes_read = file.read_at(&mut buffer, middle_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
        
        // End - อ่านจากท้ายไฟล์
        if file_size > sample_size as u64 {
            let bytes_read = file.read_at(&mut buffer, end_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
    }
    
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    {
        use std::os::unix::fs::FileExt;
        
        // Other Unix systems - use basic read_at without hints
        // Beginning - อ่านจากตำแหน่ง 0
        let bytes_read = file.read_at(&mut buffer, beginning_offset)?;
        hasher.update(&buffer[..bytes_read]);
        
        // Middle - อ่านจากตำแหน่งกลางไฟล์
        if file_size > sample_size as u64 * 2 {
            let bytes_read = file.read_at(&mut buffer, middle_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
        
        // End - อ่านจากท้ายไฟล์
        if file_size > sample_size as u64 {
            let bytes_read = file.read_at(&mut buffer, end_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
    }
    
    #[cfg(windows)]
    {
        // Windows implementation (ไม่มี posix_fadvise)
        use std::os::windows::fs::FileExt;
        
        // Beginning
        let bytes_read = file.seek_read(&mut buffer, beginning_offset)?;
        hasher.update(&buffer[..bytes_read]);
        
        // Middle
        if file_size > sample_size as u64 * 2 {
            let bytes_read = file.seek_read(&mut buffer, middle_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
        
        // End
        if file_size > sample_size as u64 {
            let bytes_read = file.seek_read(&mut buffer, end_offset)?;
            hasher.update(&buffer[..bytes_read]);
        }
    }
    
    // Fallback for other platforms
    #[cfg(not(any(unix, windows)))]
    {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = file;
        
        // Beginning
        file.seek(SeekFrom::Start(beginning_offset))?;
        let bytes_read = file.read(&mut buffer)?;
        hasher.update(&buffer[..bytes_read]);
        
        // Middle
        if file_size > sample_size as u64 * 2 {
            file.seek(SeekFrom::Start(middle_offset))?;
            let bytes_read = file.read(&mut buffer)?;
            hasher.update(&buffer[..bytes_read]);
        }
        
        // End
        if file_size > sample_size as u64 {
            file.seek(SeekFrom::Start(end_offset))?;
            let bytes_read = file.read(&mut buffer)?;
            hasher.update(&buffer[..bytes_read]);
        }
    }
    
    // Add file size to hash to distinguish files of different sizes
    hasher.update(&file_size.to_le_bytes());
    
    Ok(hasher.finalize().to_hex().to_string())
}

/// Fast file type determination without string allocation
fn determine_file_type_fast(mime_str: &str) -> String {
    match mime_str.as_bytes()[0] {
        b'i' if mime_str.starts_with("image/") => "image".to_string(),
        b'v' if mime_str.starts_with("video/") => "video".to_string(),
        _ => "other".to_string(),
    }
}

/// Prints system information about thread pool and CPU cores
fn print_system_info() {
    let num_threads = rayon::current_num_threads();
    println!("System Info:");
    println!("  Rayon threads: {}", num_threads);
    println!("  CPU cores: {}", num_cpus::get());
}

/// Prints completion statistics
fn print_completion_stats(start_time: &Instant, file_count: usize) {
    let elapsed = start_time.elapsed();
    println!(
        "Scanning completed in {:.2?} - processed {} unique files",
        elapsed, file_count
    );
}


