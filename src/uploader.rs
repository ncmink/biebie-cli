use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::Client;
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize, Clone)]
pub struct FileMeta {
    pub filename: String,
    pub folder: String,
    pub size: u64,
    pub mime: String,
    pub hash: String,
    pub filetype: String, // image / video / other
}

#[derive(Serialize)]
pub struct UploadRequest {
    pub files: Vec<FileMeta>,
    pub scan_timestamp: String,
    pub total_files: usize,
    pub total_size: u64,
}

pub fn upload_metadata(
    api_url: &str,
    files: &[FileMeta],
) -> Result<(), Box<dyn std::error::Error>> {
    if files.is_empty() {
        println!("No files to upload");
        return Ok(());
    }

    println!("📤 Preparing to upload {} files to API...", files.len());

    let client = Client::new();
    let total_size: u64 = files.iter().map(|f| f.size).sum();

    let upload_request = UploadRequest {
        files: files.to_vec(),
        scan_timestamp: chrono::Utc::now().to_rfc3339(),
        total_files: files.len(),
        total_size,
    };

    // Create progress bar for upload
    let progress_bar = ProgressBar::new(1);
    progress_bar.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {spinner:.green} {msg}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
    );
    progress_bar.set_message("Uploading metadata...");

    let resp = client
        .post(api_url)
        .timeout(Duration::from_secs(30))
        .header("Content-Type", "application/json")
        .json(&upload_request)
        .send();

    progress_bar.finish_and_clear();

    match resp {
        Ok(response) => {
            if response.status().is_success() {
                println!(
                    "✅ Successfully uploaded metadata: HTTP {}",
                    response.status()
                );
                println!(
                    "   📊 Files: {}, Total size: {} bytes",
                    files.len(),
                    total_size
                );
            } else {
                eprintln!("⚠️  API responded with error: HTTP {}", response.status());
                if let Ok(text) = response.text() {
                    eprintln!("   Response: {}", text);
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to upload metadata: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}
