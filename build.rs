use image::{DynamicImage, ImageFormat, imageops::FilterType};
use std::{
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.png.b64");

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let png = decode_base64(include_str!("assets/app-icon.png.b64"));
    let png_path = output_dir.join("app-icon.png");
    fs::write(&png_path, &png).expect("write app icon PNG");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    if target_os == "windows" && host.contains("windows") {
        embed_windows_icon(&output_dir, &png);
    }
}

fn embed_windows_icon(output_dir: &Path, source_png: &[u8]) {
    let icon_path = output_dir.join("app-icon.ico");
    fs::write(&icon_path, create_ico(source_png)).expect("write app icon ICO");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon_path.to_str().expect("UTF-8 icon path"))
        .set("ProductName", "Rust Pigeon")
        .set("FileDescription", "信鸽足环自动识别打印")
        .set("OriginalFilename", "rust-pigeon.exe");
    resource.compile().expect("embed Windows application icon");
}

fn create_ico(source_png: &[u8]) -> Vec<u8> {
    let source = image::load_from_memory_with_format(source_png, ImageFormat::Png)
        .expect("decode app icon PNG");
    let sizes = [256_u32, 128, 64, 48, 32, 16];
    let images: Vec<(u32, Vec<u8>)> = sizes
        .into_iter()
        .map(|size| (size, resize_as_png(&source, size)))
        .collect();

    let directory_size = 6 + images.len() * 16;
    let mut offset = directory_size as u32;
    let total_size = directory_size + images.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
    let mut ico = Vec::with_capacity(total_size);
    ico.extend_from_slice(&0_u16.to_le_bytes());
    ico.extend_from_slice(&1_u16.to_le_bytes());
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());

    for (size, bytes) in &images {
        ico.push(if *size == 256 { 0 } else { *size as u8 });
        ico.push(if *size == 256 { 0 } else { *size as u8 });
        ico.push(0);
        ico.push(0);
        ico.extend_from_slice(&1_u16.to_le_bytes());
        ico.extend_from_slice(&32_u16.to_le_bytes());
        ico.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += bytes.len() as u32;
    }
    for (_, bytes) in images {
        ico.extend_from_slice(&bytes);
    }
    ico
}

fn resize_as_png(source: &DynamicImage, size: u32) -> Vec<u8> {
    let resized = source.resize_exact(size, size, FilterType::Lanczos3);
    let mut output = Cursor::new(Vec::new());
    resized
        .write_to(&mut output, ImageFormat::Png)
        .expect("encode resized icon PNG");
    output.into_inner()
}

fn decode_base64(source: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(source.len() * 3 / 4);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;

    for byte in source.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ if byte.is_ascii_whitespace() => continue,
            _ => panic!("invalid base64 character"),
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((accumulator >> bits) as u8);
            accumulator &= (1_u32 << bits) - 1;
        }
    }
    output
}
