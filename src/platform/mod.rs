use crate::config::CaptureRegion;

#[derive(Debug, Clone)]
pub struct CapturedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CapturedImage {
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.width.hash(&mut hasher);
        self.height.hash(&mut hasher);
        self.rgba.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(not(windows))]
mod stub;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use stub::*;
#[cfg(windows)]
pub use windows::*;

pub trait Platform: Send + Sync + 'static {
    fn capture(&self, region: CaptureRegion) -> Result<CapturedImage, String>;
    fn printers(&self) -> Result<Vec<String>, String>;
    fn print_ring(&self, printer_name: &str, value: &str) -> Result<(), String>;
    fn virtual_screen(&self) -> Result<CaptureRegion, String>;
}

pub fn escpos_ring_receipt(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(value.len() + 24);
    bytes.extend_from_slice(&[0x1b, 0x40]); // Initialize.
    bytes.extend_from_slice(&[0x1b, 0x61, 0x01]); // Center.
    bytes.extend_from_slice(&[0x1d, 0x21, 0x11]); // Double width and height.
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(&[0x1d, 0x21, 0x00]);
    bytes.extend_from_slice(b"\n\n\n");
    bytes.extend_from_slice(&[0x1d, 0x56, 0x42, 0x00]); // Feed then partial cut.
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_initializes_formats_and_cuts() {
        let bytes = escpos_ring_receipt("2026-10-0178948");
        assert!(bytes.starts_with(&[0x1b, 0x40, 0x1b, 0x61, 0x01]));
        assert!(bytes.windows(16).any(|part| part == b"2026-10-0178948\n"));
        assert!(bytes.ends_with(&[0x1d, 0x56, 0x42, 0x00]));
    }
}
