use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl CaptureRegion {
    pub fn is_valid(self) -> bool {
        self.width >= 8 && self.height >= 8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub region: Option<CaptureRegion>,
    pub printer_name: String,
    pub tesseract_path: String,
    pub poll_interval_ms: u64,
    pub stable_observations: u8,
    pub last_printed: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            region: None,
            printer_name: String::new(),
            tesseract_path: default_tesseract_path(),
            poll_interval_ms: 500,
            stable_observations: 3,
            last_printed: None,
        }
    }
}

fn default_tesseract_path() -> String {
    let bundled = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("tesseract/tesseract.exe"))
    });
    if let Some(path) = bundled.filter(|path| path.is_file()) {
        return path.to_string_lossy().into_owned();
    }
    "tesseract.exe".to_owned()
}

impl AppConfig {
    pub fn path() -> PathBuf {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data)
                .join("RustPigeon")
                .join("config.json");
        }

        PathBuf::from("rust-pigeon.json")
    }

    pub fn load() -> Self {
        fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(path, bytes)
    }
}
