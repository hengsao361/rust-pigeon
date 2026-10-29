use crate::platform::CapturedImage;
use image::{DynamicImage, GrayImage, ImageBuffer, ImageFormat, Rgba, imageops::FilterType};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const TESSERACT_TIMEOUT: Duration = Duration::from_secs(10);
static OCR_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub trait OcrEngine: Send {
    fn recognize(&mut self, image: &CapturedImage) -> Result<String, String>;
}

pub struct TesseractCli {
    executable: PathBuf,
}

impl TesseractCli {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            executable: path.as_ref().to_owned(),
        }
    }
}

impl OcrEngine for TesseractCli {
    fn recognize(&mut self, image: &CapturedImage) -> Result<String, String> {
        let buffer =
            ImageBuffer::<Rgba<u8>, _>::from_raw(image.width, image.height, image.rgba.clone())
                .ok_or_else(|| "截图像素数据长度不正确".to_owned())?;

        let prepared = preprocess(DynamicImage::ImageRgba8(buffer));
        let temp_files = OcrTempFiles::new()?;
        DynamicImage::ImageLuma8(prepared)
            .save_with_format(&temp_files.input, ImageFormat::Png)
            .map_err(|error| format!("保存 OCR 临时图片失败：{error}"))?;

        let mut command = Command::new(&self.executable);
        command
            .arg(&temp_files.input)
            .arg(&temp_files.output_base)
            .args(["-l", "eng", "--psm", "7"])
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(tessdata) = self
            .executable
            .parent()
            .map(|parent| parent.join("tessdata"))
            .filter(|path| path.is_dir())
        {
            command
                .env_remove("TESSDATA_PREFIX")
                .arg("--tessdata-dir")
                .arg(tessdata);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command.spawn().map_err(|error| {
            format!(
                "无法启动 Tesseract（{}）：{error}",
                self.executable.display()
            )
        })?;

        let output = wait_with_timeout(child, TESSERACT_TIMEOUT).map_err(|error| match error {
            WaitError::Io(error) => format!("等待 Tesseract 失败：{error}"),
            WaitError::Timeout(stderr) => {
                let details = String::from_utf8_lossy(&stderr).trim().to_owned();
                if details.is_empty() {
                    "Tesseract 超过 10 秒未完成，已终止".to_owned()
                } else {
                    format!("Tesseract 超过 10 秒未完成，已终止：{details}")
                }
            }
        })?;

        if !output.status.success() {
            let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if details.is_empty() {
                format!("Tesseract 退出码：{}", output.status)
            } else {
                format!("Tesseract 识别失败：{details}")
            });
        }

        fs::read_to_string(&temp_files.output_text)
            .map(|text| text.trim().to_owned())
            .map_err(|error| format!("读取 Tesseract 识别结果失败：{error}"))
    }
}

struct OcrTempFiles {
    input: PathBuf,
    output_base: PathBuf,
    output_text: PathBuf,
}

impl OcrTempFiles {
    fn new() -> Result<Self, String> {
        let directory = std::env::temp_dir().join("RustPigeon").join("ocr");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("创建 OCR 临时目录失败：{error}"))?;

        let sequence = OCR_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stem = format!("ocr-{}-{sequence}", std::process::id());
        let input = directory.join(format!("{stem}.png"));
        let output_base = directory.join(format!("{stem}-result"));
        let output_text = output_base.with_extension("txt");

        // Process IDs can be reused after an abnormal exit, so remove any stale collision.
        let _ = fs::remove_file(&input);
        let _ = fs::remove_file(&output_text);

        Ok(Self {
            input,
            output_base,
            output_text,
        })
    }
}

impl Drop for OcrTempFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.input);
        let _ = fs::remove_file(&self.output_text);
    }
}

#[derive(Debug)]
enum WaitError {
    Io(io::Error),
    Timeout(Vec<u8>),
}

fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<Output, WaitError> {
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(WaitError::Io)? {
            Some(_) => return child.wait_with_output().map_err(WaitError::Io),
            None if started.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            None => {
                let _ = child.kill();
                let output = child.wait_with_output().map_err(WaitError::Io)?;
                return Err(WaitError::Timeout(output.stderr));
            }
        }
    }
}

fn preprocess(image: DynamicImage) -> GrayImage {
    let gray = image.into_luma8();
    let scale = if gray.height() < 80 {
        3
    } else if gray.height() < 160 {
        2
    } else {
        1
    };
    let mut gray = if scale > 1 {
        image::imageops::resize(
            &gray,
            gray.width().saturating_mul(scale),
            gray.height().saturating_mul(scale),
            FilterType::Lanczos3,
        )
    } else {
        gray
    };

    let average = gray.pixels().map(|pixel| pixel[0] as u64).sum::<u64>()
        / (gray.width() as u64 * gray.height() as u64).max(1);
    if average < 128 {
        for pixel in gray.pixels_mut() {
            pixel[0] = 255 - pixel[0];
        }
    }

    let threshold = otsu_threshold(&gray);
    for pixel in gray.pixels_mut() {
        pixel[0] = if pixel[0] > threshold { 255 } else { 0 };
    }
    gray
}

fn otsu_threshold(image: &GrayImage) -> u8 {
    let mut histogram = [0_u64; 256];
    for pixel in image.pixels() {
        histogram[pixel[0] as usize] += 1;
    }

    let total = image.width() as u64 * image.height() as u64;
    let weighted_total = histogram
        .iter()
        .enumerate()
        .map(|(value, count)| value as u64 * count)
        .sum::<u64>();
    let mut background_weight = 0_u64;
    let mut background_sum = 0_u64;
    let mut best_variance = 0_f64;
    let mut best = 127_u8;

    for (value, count) in histogram.into_iter().enumerate() {
        background_weight += count;
        if background_weight == 0 {
            continue;
        }
        let foreground_weight = total.saturating_sub(background_weight);
        if foreground_weight == 0 {
            break;
        }
        background_sum += value as u64 * count;
        let background_mean = background_sum as f64 / background_weight as f64;
        let foreground_mean = (weighted_total - background_sum) as f64 / foreground_weight as f64;
        let variance = background_weight as f64
            * foreground_weight as f64
            * (background_mean - foreground_mean).powi(2);
        if variance > best_variance {
            best_variance = variance;
            best = value as u8;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_files_are_unique_and_removed_on_drop() {
        let first = OcrTempFiles::new().unwrap();
        let second = OcrTempFiles::new().unwrap();
        assert_ne!(first.input, second.input);

        fs::write(&first.input, b"input").unwrap();
        fs::write(&first.output_text, b"output").unwrap();
        let input = first.input.clone();
        let output = first.output_text.clone();
        drop(first);

        assert!(!input.exists());
        assert!(!output.exists());
    }

    #[test]
    fn wait_with_timeout_returns_completed_process() {
        let child = fast_command().spawn().unwrap();
        let output = wait_with_timeout(child, Duration::from_secs(1)).unwrap();
        assert!(output.status.success());
    }

    #[test]
    fn wait_with_timeout_terminates_slow_process() {
        let child = slow_command().spawn().unwrap();
        let result = wait_with_timeout(child, Duration::from_millis(100));
        assert!(matches!(result, Err(WaitError::Timeout(_))));
    }

    #[cfg(windows)]
    fn fast_command() -> Command {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /B 0"]);
        command
    }

    #[cfg(not(windows))]
    fn fast_command() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        command
    }

    #[cfg(windows)]
    fn slow_command() -> Command {
        let mut command = Command::new("ping.exe");
        command
            .args(["-n", "6", "127.0.0.1"])
            .stdout(Stdio::null());
        command
    }

    #[cfg(not(windows))]
    fn slow_command() -> Command {
        let mut command = Command::new("sleep");
        command.arg("2");
        command
    }
}
