mod hash;
mod scanner;
mod uploader;

use clap::Parser;

/// Scan media folder and upload metadata to API
#[derive(Parser)]
struct Args {
    /// Path to scan
    folder: String,
    /// API endpoint to upload
    #[arg(long)]
    api: Option<String>,
    /// Output format (json, csv, or console)
    #[arg(long, value_enum, default_value = "console")]
    output: OutputFormat,
    /// Save output to file
    #[arg(long)]
    output_file: Option<String>,
}

#[derive(clap::ValueEnum, Clone)]
enum OutputFormat {
    Console,
    Json,
    Csv,
}

fn main() {
    let args = Args::parse();

    println!("📁 Scanning: {}", args.folder);

    let files = scanner::scan_folder(&args.folder);

    // Output files based on format
    match args.output {
        OutputFormat::Console => {
            for file in &files {
                println!(
                    "📄 {} ({} bytes, {}, hash: {}, type: {})",
                    file.filename, file.size, file.mime, file.hash, file.filetype
                );
            }
        }
        OutputFormat::Json => {
            let json_output =
                serde_json::to_string_pretty(&files).expect("Failed to serialize to JSON");
            if let Some(output_file) = &args.output_file {
                std::fs::write(output_file, &json_output).expect("Failed to write JSON file");
                println!("💾 JSON output saved to: {}", output_file);
            } else {
                println!("{}", json_output);
            }
        }
        OutputFormat::Csv => {
            let csv_output = generate_csv(&files);
            if let Some(output_file) = &args.output_file {
                std::fs::write(output_file, &csv_output).expect("Failed to write CSV file");
                println!("💾 CSV output saved to: {}", output_file);
            } else {
                println!("{}", csv_output);
            }
        }
    }

    // Upload to API if endpoint provided
    if let Some(api_url) = args.api {
        println!("📤 Uploading to API: {}", api_url);
        if let Err(e) = uploader::upload_metadata(&api_url, &files) {
            eprintln!("Failed to upload: {}", e);
            std::process::exit(1);
        }
    } else {
        println!("💡 Use --api <URL> to upload metadata to an API endpoint");
    }
}

fn generate_csv(files: &[uploader::FileMeta]) -> String {
    let mut csv = String::from("filename,folder,size,mime,hash,filetype\n");
    for file in files {
        csv.push_str(&format!(
            "\"{}\",\"{}\",{},\"{}\",\"{}\",\"{}\"\n",
            file.filename.replace('"', "\"\""),
            file.folder.replace('"', "\"\""),
            file.size,
            file.mime,
            file.hash,
            file.filetype
        ));
    }
    csv
}
