use super::{CapturedImage, Platform};
use crate::config::CaptureRegion;
use std::sync::mpsc::{self, Receiver};

#[derive(Debug, Default)]
pub struct NativePlatform;

impl Platform for NativePlatform {
    fn capture(&self, _region: CaptureRegion) -> Result<CapturedImage, String> {
        Err("屏幕捕捉仅支持 Windows".to_owned())
    }

    fn printers(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn print_ring(&self, _printer_name: &str, _value: &str) -> Result<(), String> {
        Err("RAW 热敏打印仅支持 Windows".to_owned())
    }

    fn virtual_screen(&self) -> Result<CaptureRegion, String> {
        Ok(CaptureRegion {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        })
    }
}

pub fn spawn_region_hotkey() -> Result<Receiver<()>, String> {
    let (_sender, receiver) = mpsc::channel();
    Ok(receiver)
}
