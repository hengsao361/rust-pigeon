use crate::{config::CaptureRegion, receipt::PrintTicket};
use std::sync::mpsc::Receiver;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSelection {
    pub region: CaptureRegion,
    pub screen: CaptureRegion,
}

pub type RegionSelectionResult = Result<Option<RegionSelection>, String>;
pub type RegionSelectionReceiver = Receiver<RegionSelectionResult>;

#[cfg(any(windows, test))]
pub(crate) fn region_from_client_points(
    screen: CaptureRegion,
    start: (i32, i32),
    end: (i32, i32),
) -> CaptureRegion {
    let max_x = screen.width.min(i32::MAX as u32) as i32;
    let max_y = screen.height.min(i32::MAX as u32) as i32;
    let start_x = start.0.clamp(0, max_x);
    let start_y = start.1.clamp(0, max_y);
    let end_x = end.0.clamp(0, max_x);
    let end_y = end.1.clamp(0, max_y);
    let left = start_x.min(end_x);
    let top = start_y.min(end_y);

    CaptureRegion {
        x: screen.x.saturating_add(left),
        y: screen.y.saturating_add(top),
        width: start_x.abs_diff(end_x),
        height: start_y.abs_diff(end_y),
    }
}

#[cfg(not(windows))]
mod stub;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_overlay;
#[cfg(windows)]
mod windows_selector;

#[cfg(not(windows))]
pub use stub::*;
#[cfg(windows)]
pub use windows::*;
#[cfg(windows)]
pub use windows_overlay::*;
#[cfg(windows)]
pub use windows_selector::*;

pub trait Platform: Send + Sync + 'static {
    fn capture(&self, region: CaptureRegion) -> Result<CapturedImage, String>;
    fn printers(&self) -> Result<Vec<String>, String>;
    fn print_ring(&self, printer_name: &str, ticket: &PrintTicket) -> Result<(), String>;
    fn virtual_screen(&self) -> Result<CaptureRegion, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_selection_uses_exact_mouse_down_point() {
        let screen = CaptureRegion {
            x: -1920,
            y: 0,
            width: 3840,
            height: 1080,
        };

        assert_eq!(
            region_from_client_points(screen, (125, 80), (525, 280)),
            CaptureRegion {
                x: -1795,
                y: 80,
                width: 400,
                height: 200,
            }
        );
        assert_eq!(
            region_from_client_points(screen, (525, 280), (125, 80)),
            CaptureRegion {
                x: -1795,
                y: 80,
                width: 400,
                height: 200,
            }
        );
    }
}
