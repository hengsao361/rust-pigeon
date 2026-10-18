use crate::config::CaptureRegion;
use std::{
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HGDIOBJ,
            InvalidateRect, PAINTSTRUCT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
            HWND_TOPMOST, LWA_COLORKEY, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_HIDE,
            SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowDisplayAffinity,
            SetWindowPos, ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WM_ERASEBKGND,
            WM_PAINT, WM_QUIT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
    core::PCWSTR,
};

const OVERLAY_MARGIN: i32 = 4;
const BORDER_THICKNESS: i32 = 3;
const DASH_LENGTH: i32 = 14;
const DASH_GAP: i32 = 8;
const TRANSPARENT_COLOR: COLORREF = COLORREF(0x000000);
const BORDER_COLOR: COLORREF = COLORREF(0x00ff00);

enum OverlayCommand {
    SetRegion(Option<CaptureRegion>),
    Shutdown,
}

pub struct RegionOverlay {
    commands: Sender<OverlayCommand>,
    worker: Option<thread::JoinHandle<()>>,
}

impl RegionOverlay {
    pub fn set_region(&self, region: Option<CaptureRegion>) {
        let _ = self.commands.send(OverlayCommand::SetRegion(region));
    }

    fn shutdown(&mut self) {
        let _ = self.commands.send(OverlayCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for RegionOverlay {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn spawn_region_overlay() -> Result<RegionOverlay, String> {
    let (command_tx, command_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("pigeon-region-overlay".to_owned())
        .spawn(move || overlay_thread(command_rx, ready_tx))
        .map_err(|error| format!("创建捕捉区域边框线程失败：{error}"))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(RegionOverlay {
            commands: command_tx,
            worker: Some(worker),
        }),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            let _ = worker.join();
            Err("捕捉区域边框线程启动失败".to_owned())
        }
    }
}

fn overlay_thread(commands: Receiver<OverlayCommand>, ready: mpsc::SyncSender<Result<(), String>>) {
    let window = match unsafe { create_overlay_window() } {
        Ok(window) => window,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));

    let mut running = true;
    while running {
        match commands.recv_timeout(Duration::from_millis(16)) {
            Ok(OverlayCommand::SetRegion(region)) => unsafe { update_overlay(window, region) },
            Ok(OverlayCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                running = false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        loop {
            match commands.try_recv() {
                Ok(OverlayCommand::SetRegion(region)) => unsafe { update_overlay(window, region) },
                Ok(OverlayCommand::Shutdown) | Err(TryRecvError::Disconnected) => {
                    running = false;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    running = false;
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    let _ = unsafe { DestroyWindow(window) };
}

unsafe fn create_overlay_window() -> Result<HWND, String> {
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|error| format!("读取程序模块失败：{error}"))?;
    let instance = HINSTANCE(module.0);
    let class_name = wide("RustPigeonRegionOverlay");
    let window_name = wide("Rust Pigeon Capture Region");
    let class = WNDCLASSW {
        lpfnWndProc: Some(overlay_window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("注册捕捉区域边框窗口失败".to_owned());
    }

    let extended_style =
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;
    let window = unsafe {
        CreateWindowExW(
            extended_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(window_name.as_ptr()),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|error| format!("创建捕捉区域边框窗口失败：{error}"))?;

    unsafe { SetLayeredWindowAttributes(window, TRANSPARENT_COLOR, 0, LWA_COLORKEY) }
        .map_err(|error| format!("设置捕捉区域边框透明色失败：{error}"))?;
    let _ = unsafe { SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) };
    Ok(window)
}

unsafe fn update_overlay(window: HWND, region: Option<CaptureRegion>) {
    let Some(region) = region.filter(|region| region.is_valid()) else {
        let _ = unsafe { ShowWindow(window, SW_HIDE) };
        return;
    };

    let max_dimension = (i32::MAX - OVERLAY_MARGIN * 2) as u32;
    let width = region.width.min(max_dimension) as i32 + OVERLAY_MARGIN * 2;
    let height = region.height.min(max_dimension) as i32 + OVERLAY_MARGIN * 2;
    let _ = unsafe {
        SetWindowPos(
            window,
            Some(HWND_TOPMOST),
            region.x.saturating_sub(OVERLAY_MARGIN),
            region.y.saturating_sub(OVERLAY_MARGIN),
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    let _ = unsafe { InvalidateRect(Some(window), None, true) };
}

unsafe extern "system" fn overlay_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe { paint_overlay(window) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn paint_overlay(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let device = unsafe { BeginPaint(window, &mut paint) };
    let mut client = RECT::default();
    if unsafe { GetClientRect(window, &mut client) }.is_ok() {
        let background = unsafe { CreateSolidBrush(TRANSPARENT_COLOR) };
        let border = unsafe { CreateSolidBrush(BORDER_COLOR) };
        unsafe {
            FillRect(device, &client, background);
            paint_horizontal_dashes(device, border, 0, client.right);
            paint_horizontal_dashes(
                device,
                border,
                (client.bottom - BORDER_THICKNESS).max(0),
                client.right,
            );
            paint_vertical_dashes(device, border, 0, client.bottom);
            paint_vertical_dashes(
                device,
                border,
                (client.right - BORDER_THICKNESS).max(0),
                client.bottom,
            );
            let _ = DeleteObject(HGDIOBJ(background.0));
            let _ = DeleteObject(HGDIOBJ(border.0));
        }
    }
    let _ = unsafe { EndPaint(window, &paint) };
}

unsafe fn paint_horizontal_dashes(
    device: windows::Win32::Graphics::Gdi::HDC,
    brush: windows::Win32::Graphics::Gdi::HBRUSH,
    top: i32,
    width: i32,
) {
    let mut left = 0;
    while left < width {
        let dash = RECT {
            left,
            top,
            right: (left + DASH_LENGTH).min(width),
            bottom: top + BORDER_THICKNESS,
        };
        unsafe { FillRect(device, &dash, brush) };
        left += DASH_LENGTH + DASH_GAP;
    }
}

unsafe fn paint_vertical_dashes(
    device: windows::Win32::Graphics::Gdi::HDC,
    brush: windows::Win32::Graphics::Gdi::HBRUSH,
    left: i32,
    height: i32,
) {
    let mut top = 0;
    while top < height {
        let dash = RECT {
            left,
            top,
            right: left + BORDER_THICKNESS,
            bottom: (top + DASH_LENGTH).min(height),
        };
        unsafe { FillRect(device, &dash, brush) };
        top += DASH_LENGTH + DASH_GAP;
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
