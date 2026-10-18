use super::{CapturedImage, Platform};
use crate::{
    config::CaptureRegion,
    receipt::{PrintTicket, ReceiptFonts, ReceiptTimestamp, escpos_raster_receipt, render_receipt},
};
use fontdue::{Font, FontSettings};
use std::{
    ffi::c_void,
    fs,
    mem::size_of,
    sync::{
        OnceLock,
        mpsc::{self, Receiver},
    },
    thread,
};
use windows::{
    Win32::{
        Foundation::GetLastError,
        Graphics::{
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
                CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits,
                HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
            },
            Printing::{
                ClosePrinter, DOC_INFO_1W, EndDocPrinter, EndPagePrinter, EnumPrintersW,
                OpenPrinterW, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_4W,
                StartDocPrinterW, StartPagePrinter, WritePrinter,
            },
        },
        System::SystemInformation::GetLocalTime,
        UI::{
            Input::KeyboardAndMouse::{
                MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
            },
            WindowsAndMessaging::{
                GetMessageW, GetSystemMetrics, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
                SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WM_HOTKEY,
            },
        },
    },
    core::{HRESULT, PCWSTR, PWSTR},
};

#[derive(Debug, Default)]
pub struct NativePlatform;

pub fn spawn_region_hotkey() -> Result<Receiver<()>, String> {
    const HOTKEY_ID: i32 = 0x5250;
    const KEY_R: u32 = 0x52;

    let (event_tx, event_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("pigeon-hotkey".to_owned())
        .spawn(move || unsafe {
            let registered = RegisterHotKey(
                None,
                HOTKEY_ID,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                KEY_R,
            );
            if registered.is_err() {
                let _ = ready_tx.send(Err(last_error("注册 Ctrl+Shift+R 全局热键")));
                return;
            }
            let _ = ready_tx.send(Ok(()));

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                if message.message == WM_HOTKEY
                    && message.wParam.0 == HOTKEY_ID as usize
                    && event_tx.send(()).is_err()
                {
                    break;
                }
            }
            let _ = UnregisterHotKey(None, HOTKEY_ID);
        })
        .map_err(|error| format!("创建热键线程失败：{error}"))?;

    ready_rx
        .recv()
        .map_err(|_| "热键线程启动失败".to_owned())??;
    Ok(event_rx)
}

impl Platform for NativePlatform {
    fn capture(&self, region: CaptureRegion) -> Result<CapturedImage, String> {
        if !region.is_valid() {
            return Err("截图区域无效".to_owned());
        }

        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.0.is_null() {
                return Err(last_error("GetDC"));
            }
            let memory_dc = CreateCompatibleDC(Some(screen_dc));
            if memory_dc.0.is_null() {
                ReleaseDC(None, screen_dc);
                return Err(last_error("CreateCompatibleDC"));
            }
            let bitmap =
                CreateCompatibleBitmap(screen_dc, region.width as i32, region.height as i32);
            if bitmap.0.is_null() {
                let _ = DeleteDC(memory_dc);
                ReleaseDC(None, screen_dc);
                return Err(last_error("CreateCompatibleBitmap"));
            }

            let previous = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
            let copied = BitBlt(
                memory_dc,
                0,
                0,
                region.width as i32,
                region.height as i32,
                Some(screen_dc),
                region.x,
                region.y,
                SRCCOPY | CAPTUREBLT,
            );

            let result = if copied.is_err() {
                Err(last_error("BitBlt"))
            } else {
                let mut info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: region.width as i32,
                        biHeight: -(region.height as i32),
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bgra = vec![0_u8; region.width as usize * region.height as usize * 4];
                let lines = GetDIBits(
                    memory_dc,
                    bitmap,
                    0,
                    region.height,
                    Some(bgra.as_mut_ptr().cast()),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                if lines == 0 {
                    Err(last_error("GetDIBits"))
                } else {
                    for pixel in bgra.as_chunks_mut::<4>().0 {
                        pixel.swap(0, 2);
                        pixel[3] = 255;
                    }
                    Ok(CapturedImage {
                        width: region.width,
                        height: region.height,
                        rgba: bgra,
                    })
                }
            };

            SelectObject(memory_dc, previous);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory_dc);
            ReleaseDC(None, screen_dc);
            result
        }
    }

    fn printers(&self) -> Result<Vec<String>, String> {
        unsafe {
            let mut needed = 0;
            let mut returned = 0;
            let _ = EnumPrintersW(
                PRINTER_ENUM_LOCAL,
                None,
                4,
                None,
                &mut needed,
                &mut returned,
            );
            if needed == 0 {
                return Ok(Vec::new());
            }

            let mut bytes = vec![0_u8; needed as usize];
            EnumPrintersW(
                PRINTER_ENUM_LOCAL,
                None,
                4,
                Some(&mut bytes),
                &mut needed,
                &mut returned,
            )
            .map_err(|_| last_error("EnumPrintersW"))?;

            let entries = std::slice::from_raw_parts(
                bytes.as_ptr().cast::<PRINTER_INFO_4W>(),
                returned as usize,
            );
            let mut names: Vec<String> = entries
                .iter()
                .filter_map(|entry| pwstr_to_string(entry.pPrinterName))
                .collect();
            names.sort_unstable();
            names.dedup();
            Ok(names)
        }
    }

    fn print_ring(&self, printer_name: &str, ticket: &PrintTicket) -> Result<(), String> {
        let printer = wide(printer_name);
        let document_name = wide("Rust Pigeon Ring");
        let datatype = wide("RAW");
        let fonts = receipt_fonts()?;
        let timestamp = local_timestamp();
        let bitmap = render_receipt(ticket, &timestamp, fonts)?;
        let bytes = escpos_raster_receipt(&bitmap);

        unsafe {
            let mut handle = PRINTER_HANDLE::default();
            OpenPrinterW(PCWSTR(printer.as_ptr()), &mut handle, None)
                .map_err(|_| last_error("OpenPrinterW"))?;

            let info = DOC_INFO_1W {
                pDocName: PWSTR(document_name.as_ptr() as *mut _),
                pOutputFile: PWSTR::null(),
                pDatatype: PWSTR(datatype.as_ptr() as *mut _),
            };
            let job = StartDocPrinterW(handle, 1, &info);
            if job == 0 {
                let error = last_error("StartDocPrinterW");
                let _ = ClosePrinter(handle);
                return Err(error);
            }
            if !StartPagePrinter(handle).as_bool() {
                let error = last_error("StartPagePrinter");
                let _ = EndDocPrinter(handle);
                let _ = ClosePrinter(handle);
                return Err(error);
            }

            let mut written = 0;
            let write_result = WritePrinter(
                handle,
                bytes.as_ptr().cast::<c_void>(),
                bytes.len() as u32,
                &mut written,
            );
            let write_error = (!write_result.as_bool()).then(|| last_error("WritePrinter"));
            let _ = EndPagePrinter(handle);
            let _ = EndDocPrinter(handle);
            let _ = ClosePrinter(handle);

            if let Some(error) = write_error {
                return Err(error);
            }
            if written != bytes.len() as u32 {
                return Err(format!("打印数据只写入 {written}/{} 字节", bytes.len()));
            }
            Ok(())
        }
    }

    fn virtual_screen(&self) -> Result<CaptureRegion, String> {
        unsafe {
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
}

fn receipt_fonts() -> Result<&'static ReceiptFonts, String> {
    static FONTS: OnceLock<Result<ReceiptFonts, String>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            Ok(ReceiptFonts {
                regular: load_font(
                    &[
                        r"C:\Windows\Fonts\msyh.ttc",
                        r"C:\Windows\Fonts\msyh.ttf",
                        r"C:\Windows\Fonts\simhei.ttf",
                        r"C:\Windows\Fonts\simsun.ttc",
                    ],
                    Some('鸽'),
                )?,
                bold: load_font(
                    &[
                        r"C:\Windows\Fonts\msyhbd.ttc",
                        r"C:\Windows\Fonts\msyhbd.ttf",
                        r"C:\Windows\Fonts\simhei.ttf",
                        r"C:\Windows\Fonts\msyh.ttc",
                    ],
                    Some('鸽'),
                )?,
                digits: load_font(
                    &[
                        r"C:\Windows\Fonts\arialbd.ttf",
                        r"C:\Windows\Fonts\consolab.ttf",
                        r"C:\Windows\Fonts\msyhbd.ttc",
                    ],
                    None,
                )?,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn load_font(candidates: &[&str], required_character: Option<char>) -> Result<Font, String> {
    for path in candidates {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        for collection_index in 0..8 {
            let settings = FontSettings {
                collection_index,
                ..Default::default()
            };
            let Ok(font) = Font::from_bytes(bytes.as_slice(), settings) else {
                continue;
            };
            if required_character.is_none_or(|character| font.lookup_glyph_index(character) != 0) {
                return Ok(font);
            }
        }
    }
    Err("找不到可用于票据排版的 Windows 字体（微软雅黑/黑体/Arial）".to_owned())
}

fn local_timestamp() -> ReceiptTimestamp {
    let time = unsafe { GetLocalTime() };
    ReceiptTimestamp {
        compact: format!(
            "{:04}{:02}{:02}{:02}{:02}{:02}",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
        ),
        display: format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
        ),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

unsafe fn pwstr_to_string(value: PWSTR) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut length = 0;
    while unsafe { *value.0.add(length) } != 0 {
        length += 1;
    }
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(value.0, length)
    }))
}

fn last_error(operation: &str) -> String {
    let code = unsafe { GetLastError() };
    let error = windows::core::Error::from_hresult(HRESULT::from_win32(code.0));
    format!("{operation} 失败：{error}")
}
