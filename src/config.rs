use crate::{
    receipt::{random_business_content, sanitize_channel_input},
    ring::OcrFormat,
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const APP_DIRECTORY_NAME: &str = "RustPigeon";
const LEGACY_APP_DIRECTORY_NAME: &str = "Rust Pigeon";

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

    pub fn contains(self, other: Self) -> bool {
        let self_right = i64::from(self.x) + i64::from(self.width);
        let self_bottom = i64::from(self.y) + i64::from(self.height);
        let other_right = i64::from(other.x) + i64::from(other.width);
        let other_bottom = i64::from(other.y) + i64::from(other.height);

        other.x >= self.x
            && other.y >= self.y
            && other_right <= self_right
            && other_bottom <= self_bottom
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub region: Option<CaptureRegion>,
    pub selection_screen: Option<CaptureRegion>,
    pub printer_name: String,
    #[serde(skip)]
    pub tesseract_path: String,
    pub poll_interval_ms: u64,
    pub stable_observations: u8,
    pub repeat_ocr_during_stability: bool,
    pub ocr_format: OcrFormat,
    pub last_printed: Option<String>,
    pub channel: String,
    #[serde(skip, default = "random_business_content")]
    pub business_content: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            region: None,
            selection_screen: None,
            printer_name: String::new(),
            tesseract_path: default_tesseract_path(),
            poll_interval_ms: 500,
            stable_observations: 3,
            repeat_ocr_during_stability: false,
            ocr_format: OcrFormat::default(),
            last_printed: None,
            channel: "0".to_owned(),
            business_content: random_business_content(),
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
    pub fn data_dir() -> PathBuf {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join(APP_DIRECTORY_NAME);
        }

        PathBuf::from(".")
    }

    pub fn path() -> PathBuf {
        if std::env::var_os("APPDATA").is_some() {
            return Self::data_dir().join("config.json");
        }

        PathBuf::from("rust-pigeon.json")
    }

    pub fn load() -> Self {
        cleanup_legacy_app_directory();
        let mut config: Self = fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        config.tesseract_path = default_tesseract_path();
        config.channel = sanitize_channel_input(&config.channel);
        config.business_content = random_business_content();
        config
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

fn cleanup_legacy_app_directory() {
    let Some(app_data) = std::env::var_os("APPDATA") else {
        return;
    };
    cleanup_legacy_app_directory_at(&PathBuf::from(app_data));
}

fn cleanup_legacy_app_directory_at(app_data: &Path) {
    let legacy = app_data.join(LEGACY_APP_DIRECTORY_NAME);
    let eframe_data = legacy.join("data");
    let _ = fs::remove_file(eframe_data.join("app.ron"));
    let _ = fs::remove_dir(&eframe_data);
    let _ = fs::remove_dir(&legacy);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn region_containment_handles_negative_virtual_desktop_coordinates() {
        let screen = CaptureRegion {
            x: -1920,
            y: 0,
            width: 3840,
            height: 1080,
        };
        assert!(screen.contains(CaptureRegion {
            x: -1200,
            y: 200,
            width: 600,
            height: 300,
        }));
        assert!(!screen.contains(CaptureRegion {
            x: 1800,
            y: 900,
            width: 400,
            height: 300,
        }));
    }

    #[test]
    fn recognition_options_have_safe_defaults_for_older_configs() {
        let default_config = AppConfig::default();
        assert!(!default_config.repeat_ocr_during_stability);
        assert_eq!(default_config.ocr_format, OcrFormat::SingleLineThreePart);
        assert_eq!(
            serde_json::to_value(&default_config).unwrap()["repeat_ocr_during_stability"],
            false
        );
        assert_eq!(
            serde_json::to_value(&default_config).unwrap()["ocr_format"],
            "single_line_three_part"
        );

        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.repeat_ocr_during_stability);
        assert_eq!(config.ocr_format, OcrFormat::SingleLineThreePart);
    }

    #[test]
    fn removes_only_known_legacy_eframe_storage() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rust-pigeon-config-{unique}"));
        let legacy_data = root.join(LEGACY_APP_DIRECTORY_NAME).join("data");
        fs::create_dir_all(&legacy_data).unwrap();
        fs::write(legacy_data.join("app.ron"), b"window state").unwrap();

        cleanup_legacy_app_directory_at(&root);

        assert!(!root.join(LEGACY_APP_DIRECTORY_NAME).exists());
        let _ = fs::remove_dir_all(root);
    }
}
