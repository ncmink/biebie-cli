mod hash;
mod scanner;
mod uploader;

use clap::Parser;

/// Scan media folder and upload metadata to API
#[derive(Parser)]
struct Args {
    /// Path to scan
    folder: String,
    // API endpoint to upload
    // #[arg(long)]
    // api: String,
}

fn main() {
    let args = Args::parse();

    println!("📁 Scanning: {}", args.folder);

    let files = scanner::scan_folder(&args.folder);

    // print files
    for file in files {
        println!(
            "📄 {} ({} bytes, {}, hash: {}, type: {})",
            file.filename, file.size, file.mime, file.hash, file.filetype
        );
    }

    // println!("📤 Uploading to API: {}", args.api);
    // uploader::upload_metadata(&args.api, &files);
}
