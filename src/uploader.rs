use serde::Serialize;
// use reqwest::blocking::Client;

#[derive(Serialize, Clone)]
pub struct FileMeta {
    pub filename: String,
    pub folder: String,
    pub size: u64,
    pub mime: String,
    pub hash: String,
    pub filetype: String, // image / video / other
}

// pub fn upload_metadata(api_url: &str, files: &[FileMeta]) {
//     let client = Client::new();
//     let resp = client.post(api_url)
//         .json(&files)
//         .send();

//     match resp {
//         Ok(res) => println!("✅ Uploaded: HTTP {}", res.status()),
//         Err(e) => eprintln!("❌ Failed to upload: {}", e),
//     }
// }
