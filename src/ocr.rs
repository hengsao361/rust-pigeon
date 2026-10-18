use crate::platform::CapturedImage;
use image::{DynamicImage, GrayImage, ImageBuffer, ImageFormat, Rgba, imageops::FilterType};
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

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
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(prepared)
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|error| format!("编码 OCR 图片失败：{error}"))?;

        let mut command = Command::new(&self.executable);
        command
            .args(["stdin", "stdout", "-l", "eng", "--psm", "7"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(tessdata) = self
            .executable
            .parent()
            .map(|parent| parent.join("tessdata"))
            .filter(|path| path.is_dir())
        {
            command.arg("--tessdata-dir").arg(tessdata);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|error| {
            format!(
                "无法启动 Tesseract（{}）：{error}",
                self.executable.display()
            )
        })?;

        child
            .stdin
            .take()
            .ok_or_else(|| "无法打开 Tesseract 输入流".to_owned())?
            .write_all(png.get_ref())
            .map_err(|error| format!("写入 Tesseract 失败：{error}"))?;

        let output = child
            .wait_with_output()
            .map_err(|error| format!("等待 Tesseract 失败：{error}"))?;

        if !output.status.success() {
            let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if details.is_empty() {
                format!("Tesseract 退出码：{}", output.status)
            } else {
                format!("Tesseract 识别失败：{details}")
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
