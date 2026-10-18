use super::{RegionSelection, RegionSelectionReceiver, region_from_client_points};
use crate::config::CaptureRegion;
use std::{cell::RefCell, sync::mpsc, thread};
use windows::{
    Win32::{
        Foundation::{
            COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT,
            RECT, WPARAM,
        },
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HGDIOBJ,
            InvalidateRect, PAINTSTRUCT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
                GetMessageW, GetSystemMetrics, IDC_CROSS, LWA_ALPHA, LoadCursorW, MSG,
                PostQuitMessage, RegisterClassW, SW_SHOW, SetForegroundWindow,
                SetLayeredWindowAttributes, ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY,
                WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
                WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::PCWSTR,
};

const MASK_ALPHA: u8 = 112;
const MASK_COLOR: COLORREF = COLORREF(0x00383838);
const SELECTION_COLOR: COLORREF = COLORREF(0x00ffcc00);
const SELECTION_STROKE: i32 = 4;

struct SelectorState {
    screen: CaptureRegion,
    start: Option<(i32, i32)>,
    current: Option<(i32, i32)>,
    result: Option<mpsc::Sender<Result<Option<RegionSelection>, String>>>,
}

thread_local! {
    static SELECTOR_STATE: RefCell<Option<SelectorState>> = const { RefCell::new(None) };
}

pub fn spawn_region_selector() -> Result<RegionSelectionReceiver, String> {
    let screen = virtual_screen()?;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("pigeon-region-selector".to_owned())
        .spawn(move || run_selector(screen, sender))
        .map_err(|error| format!("创建区域框选线程失败：{error}"))?;
    Ok(receiver)
}

fn virtual_screen() -> Result<CaptureRegion, String> {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        };

        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if width <= 0 || height <= 0 {
            return Err("无法读取 Windows 虚拟桌面尺寸".to_owned());
        }
        Ok(CaptureRegion {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: width as u32,
            height: height as u32,
        })
    }
}

fn run_selector(
    screen: CaptureRegion,
    sender: mpsc::Sender<Result<Option<RegionSelection>, String>>,
) {
    SELECTOR_STATE.with(|state| {
        *state.borrow_mut() = Some(SelectorState {
            screen,
            start: None,
            current: None,
            result: Some(sender),
        });
    });

    let window = match unsafe { create_selector_window(screen) } {
        Ok(window) => window,
        Err(error) => {
            send_result(Err(error));
            SELECTOR_STATE.with(|state| state.borrow_mut().take());
            return;
        }
    };

    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    SELECTOR_STATE.with(|state| state.borrow_mut().take());
}

unsafe fn create_selector_window(screen: CaptureRegion) -> Result<HWND, String> {
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|error| format!("读取程序模块失败：{error}"))?;
    let instance = HINSTANCE(module.0);
    let class_name = wide("RustPigeonNativeRegionSelector");
    let window_name = wide("Rust Pigeon Region Selector");
    let cursor = unsafe { LoadCursorW(None, IDC_CROSS) }
        .map_err(|error| format!("加载框选光标失败：{error}"))?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(selector_window_proc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0
        && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
    {
        return Err("注册区域框选窗口失败".to_owned());
    }

    let width = screen.width.min(i32::MAX as u32) as i32;
    let height = screen.height.min(i32::MAX as u32) as i32;
    let window = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(window_name.as_ptr()),
            WS_POPUP,
            screen.x,
            screen.y,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|error| format!("创建区域框选遮罩失败：{error}"))?;

    unsafe { SetLayeredWindowAttributes(window, COLORREF(0), MASK_ALPHA, LWA_ALPHA) }
        .map_err(|error| format!("设置区域框选遮罩透明度失败：{error}"))?;
    Ok(window)
}

unsafe extern "system" fn selector_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe { paint_selector(window) };
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = client_point(lparam);
            SELECTOR_STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.start = Some(point);
                    state.current = Some(point);
                }
            });
            let _ = unsafe { SetCapture(window) };
            let _ = unsafe { InvalidateRect(Some(window), None, false) };
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let point = client_point(lparam);
            let selecting = SELECTOR_STATE.with(|state| {
                let mut state = state.borrow_mut();
                let Some(state) = state.as_mut() else {
                    return false;
                };
                if state.start.is_some() {
                    state.current = Some(point);
                    true
                } else {
                    false
                }
            });
            if selecting {
                let _ = unsafe { InvalidateRect(Some(window), None, false) };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let end = client_point(lparam);
            let selection = SELECTOR_STATE.with(|state| {
                let mut state = state.borrow_mut();
                let state = state.as_mut()?;
                let start = state.start?;
                Some(RegionSelection {
                    region: region_from_client_points(state.screen, start, end),
                    screen: state.screen,
                })
            });
            let _ = unsafe { ReleaseCapture() };
            send_result(Ok(selection));
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == usize::from(VK_ESCAPE.0) => {
            send_result(Ok(None));
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_CLOSE => {
            send_result(Ok(None));
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn paint_selector(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let device = unsafe { BeginPaint(window, &mut paint) };
    let mut client = RECT::default();
    if unsafe { GetClientRect(window, &mut client) }.is_ok() {
        let mask = unsafe { CreateSolidBrush(MASK_COLOR) };
        unsafe { FillRect(device, &client, mask) };
        let _ = unsafe { DeleteObject(HGDIOBJ(mask.0)) };

        if let Some((start, current)) = SELECTOR_STATE.with(|state| {
            let state = state.borrow();
            let state = state.as_ref()?;
            Some((state.start?, state.current?))
        }) {
            let left = start.0.min(current.0);
            let top = start.1.min(current.1);
            let right = start.0.max(current.0);
            let bottom = start.1.max(current.1);
            let border = unsafe { CreateSolidBrush(SELECTION_COLOR) };
            for edge in [
                RECT {
                    left,
                    top,
                    right,
                    bottom: (top + SELECTION_STROKE).min(bottom),
                },
                RECT {
                    left,
                    top: (bottom - SELECTION_STROKE).max(top),
                    right,
                    bottom,
                },
                RECT {
                    left,
                    top,
                    right: (left + SELECTION_STROKE).min(right),
                    bottom,
                },
                RECT {
                    left: (right - SELECTION_STROKE).max(left),
                    top,
                    right,
                    bottom,
                },
            ] {
                unsafe { FillRect(device, &edge, border) };
            }
            let _ = unsafe { DeleteObject(HGDIOBJ(border.0)) };
        }
    }
    let _ = unsafe { EndPaint(window, &paint) };
}

fn send_result(result: Result<Option<RegionSelection>, String>) {
    SELECTOR_STATE.with(|state| {
        if let Some(sender) = state
            .borrow_mut()
            .as_mut()
            .and_then(|state| state.result.take())
        {
            let _ = sender.send(result);
        }
    });
}

fn client_point(lparam: LPARAM) -> (i32, i32) {
    let value = lparam.0 as u32;
    let x = (value & 0xffff) as u16 as i16 as i32;
    let y = ((value >> 16) & 0xffff) as u16 as i16 as i32;
    (x, y)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
