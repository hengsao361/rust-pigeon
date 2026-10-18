use crate::{config::CaptureRegion, receipt::PrintTicket};

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
    fn print_ring(&self, printer_name: &str, ticket: &PrintTicket) -> Result<(), String>;
    fn virtual_screen(&self) -> Result<CaptureRegion, String>;
}
