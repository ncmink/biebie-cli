use std::fs::File;

/// Compute a sample-based hash of a file for duplicate detection
///
/// This function reads samples from the beginning, middle, and end of a file
/// to generate a hash that can identify duplicate files efficiently without
/// reading the entire file content.
///
/// # Arguments
/// * `path` - Path to the file to hash
/// * `file_size` - Size of the file in bytes
///
/// # Returns
/// * `std::io::Result<String>` - The computed hash as a hex string
pub fn compute_sample_hash(path: &std::path::Path, file_size: u64) -> std::io::Result<String> {
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
            libc::posix_fadvise(
                fd,
                beginning_offset as libc::off_t,
                sample_size as libc::off_t,
                libc::POSIX_FADV_WILLNEED,
            );

            // Middle sample
            if file_size > sample_size as u64 * 2 {
                libc::posix_fadvise(
                    fd,
                    middle_offset as libc::off_t,
                    sample_size as libc::off_t,
                    libc::POSIX_FADV_WILLNEED,
                );
            }

            // End sample
            if file_size > sample_size as u64 {
                libc::posix_fadvise(
                    fd,
                    end_offset as libc::off_t,
                    sample_size as libc::off_t,
                    libc::POSIX_FADV_WILLNEED,
                );
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
            libc::posix_fadvise(
                fd,
                beginning_offset as libc::off_t,
                sample_size as libc::off_t,
                libc::POSIX_FADV_DONTNEED,
            );
            if file_size > sample_size as u64 * 2 {
                libc::posix_fadvise(
                    fd,
                    middle_offset as libc::off_t,
                    sample_size as libc::off_t,
                    libc::POSIX_FADV_DONTNEED,
                );
            }
            if file_size > sample_size as u64 {
                libc::posix_fadvise(
                    fd,
                    end_offset as libc::off_t,
                    sample_size as libc::off_t,
                    libc::POSIX_FADV_DONTNEED,
                );
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
