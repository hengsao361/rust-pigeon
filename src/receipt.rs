use fontdue::Font;
use qrcode::{QrCode, types::Color};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECEIPT_WIDTH: u32 = 576;
pub const RECEIPT_HEIGHT: u32 = 420;
const RASTER_VERTICAL_PADDING_DOTS: usize = 8;
pub const DEFAULT_BUSINESS_CONTENTS: [&str; 6] = [
    "勇夺桂冠",
    "所向披靡",
    "载誉归来",
    "决胜千里",
    "羽翼生风",
    "王者归来",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintTicket {
    pub ring_number: String,
    pub business_content: String,
    pub channel: String,
}

impl PrintTicket {
    pub fn new(ring_number: &str, business_content: &str, channel: &str) -> Self {
        Self {
            ring_number: ring_number.to_owned(),
            business_content: business_content.to_owned(),
            channel: format_channel(channel),
        }
    }

    pub fn qr_payload(&self, unix_timestamp_seconds: u64) -> String {
        format!(
            "ycd:{}@{unix_timestamp_seconds}&ch{}",
            self.ring_number, self.channel
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptTimestamp {
    pub unix_seconds: u64,
    pub display: String,
}

pub struct ReceiptFonts {
    pub regular: Font,
    pub bold: Font,
    pub digits: Font,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonochromeBitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn random_business_content() -> String {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    DEFAULT_BUSINESS_CONTENTS[seed as usize % DEFAULT_BUSINESS_CONTENTS.len()].to_owned()
}

pub fn format_channel(value: &str) -> String {
    let number = value
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u8>()
        .unwrap_or(0)
        .min(99);
    format!("{number:02}")
}

pub fn sanitize_channel_input(value: &str) -> String {
    let digits: String = value.chars().filter(char::is_ascii_digit).take(2).collect();
    if digits.is_empty() {
        "0".to_owned()
    } else {
        digits
    }
}

pub fn render_receipt(
    ticket: &PrintTicket,
    timestamp: &ReceiptTimestamp,
    fonts: &ReceiptFonts,
) -> Result<MonochromeBitmap, String> {
    let mut canvas = Canvas::new(RECEIPT_WIDTH as usize, RECEIPT_HEIGHT as usize);

    canvas.fill_rect(22, 25, 206, 4, 0);
    canvas.fill_rect(370, 25, 184, 4, 0);
    canvas.draw_text_box(
        &fonts.bold,
        &ticket.business_content,
        218,
        5,
        142,
        44,
        27.0,
        15.0,
    );

    let payload = ticket.qr_payload(timestamp.unix_seconds);
    canvas.draw_qr(&payload, 24, 72, 220)?;
    canvas.stroke_rounded_rect(260, 62, 296, 244, 54, 4);

    let (prefix, serial) = split_ring_number(&ticket.ring_number);
    if let Some(prefix) = prefix {
        canvas.draw_text_box(
            &fonts.regular,
            &format!("{prefix}-"),
            278,
            91,
            260,
            68,
            45.0,
            25.0,
        );
    }
    canvas.draw_text_box(&fonts.digits, serial, 274, 157, 268, 104, 76.0, 38.0);

    canvas.draw_text_box(&fonts.bold, "羽成达", 28, 315, 210, 48, 31.0, 20.0);
    canvas.draw_text_box(
        &fonts.regular,
        &timestamp.display,
        292,
        326,
        262,
        38,
        22.0,
        15.0,
    );
    canvas.fill_rect(22, 390, 205, 4, 0);
    canvas.fill_rect(349, 390, 205, 4, 0);
    canvas.draw_text_box(&fonts.bold, &ticket.channel, 231, 369, 114, 42, 22.0, 15.0);

    Ok(MonochromeBitmap {
        width: RECEIPT_WIDTH,
        height: RECEIPT_HEIGHT,
        pixels: canvas.pixels,
    })
}

pub fn escpos_raster_receipt(bitmap: &MonochromeBitmap) -> Vec<u8> {
    let width_bytes = bitmap.width.div_ceil(8);
    let (start_y, end_y) = vertical_print_bounds(bitmap);
    let print_height = (end_y - start_y) as u32;
    let mut bytes = Vec::with_capacity((width_bytes * print_height) as usize + 32);
    bytes.extend_from_slice(&[0x1b, 0x40]); // Initialize.
    bytes.extend_from_slice(&[0x1b, 0x61, 0x01]); // Center.
    bytes.extend_from_slice(&[
        0x1d,
        0x76,
        0x30,
        0x00,
        width_bytes as u8,
        (width_bytes >> 8) as u8,
        print_height as u8,
        (print_height >> 8) as u8,
    ]);

    for y in start_y..end_y {
        for byte_x in 0..width_bytes as usize {
            let mut packed = 0_u8;
            for bit in 0..8 {
                let x = byte_x * 8 + bit;
                if x < bitmap.width as usize && bitmap.pixels[y * bitmap.width as usize + x] < 180 {
                    packed |= 0x80 >> bit;
                }
            }
            bytes.push(packed);
        }
    }

    bytes.extend_from_slice(&[0x1d, 0x56, 0x42, 0x00]); // Feed to cutter, then partial cut.
    bytes
}

fn vertical_print_bounds(bitmap: &MonochromeBitmap) -> (usize, usize) {
    let width = bitmap.width as usize;
    let height = bitmap.height as usize;
    let Some(expected_len) = width.checked_mul(height) else {
        return (0, height);
    };
    if width == 0 || height == 0 || bitmap.pixels.len() < expected_len {
        return (0, height);
    }

    let row_has_ink = |y: usize| {
        bitmap.pixels[y * width..(y + 1) * width]
            .iter()
            .any(|pixel| *pixel < 180)
    };
    let Some(first_ink_row) = (0..height).find(|y| row_has_ink(*y)) else {
        return (0, height);
    };
    let last_ink_row = (first_ink_row..height)
        .rfind(|y| row_has_ink(*y))
        .unwrap_or(first_ink_row);

    (
        first_ink_row.saturating_sub(RASTER_VERTICAL_PADDING_DOTS),
        (last_ink_row + 1 + RASTER_VERTICAL_PADDING_DOTS).min(height),
    )
}

fn split_ring_number(value: &str) -> (Option<&str>, &str) {
    value
        .rsplit_once('-')
        .map_or((None, value), |(prefix, serial)| (Some(prefix), serial))
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![255; width * height],
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, value: u8) {
        for py in y..(y + height).min(self.height) {
            for px in x..(x + width).min(self.width) {
                self.pixels[py * self.width + px] = value;
            }
        }
    }

    fn stroke_rounded_rect(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        radius: usize,
        thickness: usize,
    ) {
        let right = x + width;
        let bottom = y + height;
        for py in y..bottom.min(self.height) {
            for px in x..right.min(self.width) {
                let outer = inside_rounded_rect(px, py, x, y, right, bottom, radius);
                let inner = inside_rounded_rect(
                    px,
                    py,
                    x + thickness,
                    y + thickness,
                    right.saturating_sub(thickness),
                    bottom.saturating_sub(thickness),
                    radius.saturating_sub(thickness),
                );
                if outer && !inner {
                    self.pixels[py * self.width + px] = 0;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_box(
        &mut self,
        font: &Font,
        value: &str,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        preferred_size: f32,
        minimum_size: f32,
    ) {
        let mut size = preferred_size;
        while size > minimum_size && measure_text(font, value, size) > width as f32 {
            size -= 1.0;
        }

        let text_width = measure_text(font, value, size);
        let mut cursor_x = x as f32 + ((width as f32 - text_width) / 2.0).max(0.0);
        let (ascent, descent) = font
            .horizontal_line_metrics(size)
            .map_or((size, 0.0), |line| (line.ascent, line.descent));
        let text_height = ascent - descent;
        let baseline = y as f32 + ((height as f32 - text_height) / 2.0).max(0.0) + ascent;

        for character in value.chars() {
            let (metrics, bitmap) = font.rasterize(character, size);
            let glyph_x = cursor_x.round() as i32 + metrics.xmin;
            let glyph_y = (baseline - metrics.ymin as f32 - metrics.height as f32).round() as i32;
            self.blend_glyph(glyph_x, glyph_y, metrics.width, metrics.height, &bitmap);
            cursor_x += metrics.advance_width;
        }
    }

    fn blend_glyph(&mut self, x: i32, y: i32, width: usize, height: usize, bitmap: &[u8]) {
        for row in 0..height {
            for column in 0..width {
                let px = x + column as i32;
                let py = y + row as i32;
                if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                    continue;
                }
                let coverage = u16::from(bitmap[row * width + column]);
                let index = py as usize * self.width + px as usize;
                self.pixels[index] =
                    ((u16::from(self.pixels[index]) * (255 - coverage)) / 255) as u8;
            }
        }
    }

    fn draw_qr(
        &mut self,
        payload: &str,
        x: usize,
        y: usize,
        box_size: usize,
    ) -> Result<(), String> {
        let code =
            QrCode::new(payload.as_bytes()).map_err(|error| format!("生成二维码失败：{error}"))?;
        let quiet_zone = 4;
        let module_count = code.width() + quiet_zone * 2;
        let scale = box_size / module_count;
        if scale == 0 {
            return Err("二维码区域过小".to_owned());
        }
        let actual_size = module_count * scale;
        let origin_x = x + (box_size - actual_size) / 2 + quiet_zone * scale;
        let origin_y = y + (box_size - actual_size) / 2 + quiet_zone * scale;

        for module_y in 0..code.width() {
            for module_x in 0..code.width() {
                if code[(module_x, module_y)] == Color::Dark {
                    self.fill_rect(
                        origin_x + module_x * scale,
                        origin_y + module_y * scale,
                        scale,
                        scale,
                        0,
                    );
                }
            }
        }
        Ok(())
    }
}

fn measure_text(font: &Font, value: &str, size: f32) -> f32 {
    value
        .chars()
        .map(|character| font.metrics(character, size).advance_width)
        .sum()
}

fn inside_rounded_rect(
    x: usize,
    y: usize,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    radius: usize,
) -> bool {
    if x < left || x >= right || y < top || y >= bottom {
        return false;
    }
    if radius == 0 || (x >= left + radius && x < right.saturating_sub(radius)) {
        return true;
    }
    if y >= top + radius && y < bottom.saturating_sub(radius) {
        return true;
    }

    let center_x = if x < left + radius {
        left + radius
    } else {
        right.saturating_sub(radius + 1)
    };
    let center_y = if y < top + radius {
        top + radius
    } else {
        bottom.saturating_sub(radius + 1)
    };
    let dx = x.abs_diff(center_x);
    let dy = y.abs_diff(center_y);
    dx * dx + dy * dy <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_is_two_digits_and_numeric_only() {
        assert_eq!(format_channel("0"), "00");
        assert_eq!(format_channel("5"), "05");
        assert_eq!(format_channel("27"), "27");
        assert_eq!(sanitize_channel_input("a123"), "12");
        assert_eq!(sanitize_channel_input(""), "0");
    }

    #[test]
    fn qr_payload_contains_ring_unix_timestamp_and_channel() {
        let ticket = PrintTicket::new("2026-03-0015369", "勇夺桂冠", "5");
        assert_eq!(
            ticket.qr_payload(1_792_392_979),
            "ycd:2026-03-0015369@1792392979&ch05"
        );
    }

    #[test]
    fn raster_command_has_expected_dimensions_and_cut() {
        let bitmap = MonochromeBitmap {
            width: 16,
            height: 2,
            pixels: vec![0; 32],
        };
        let bytes = escpos_raster_receipt(&bitmap);
        assert!(bytes.starts_with(&[0x1b, 0x40, 0x1b, 0x61, 0x01]));
        assert!(
            bytes
                .windows(8)
                .any(|part| part == [0x1d, 0x76, 0x30, 0x00, 2, 0, 2, 0])
        );
        assert!(bytes.ends_with(&[0x1d, 0x56, 0x42, 0x00]));
        assert_eq!(bytes.len(), 2 + 3 + 8 + 4 + 4);
    }

    #[test]
    fn raster_trims_blank_rows_and_keeps_safe_padding() {
        let mut bitmap = MonochromeBitmap {
            width: 8,
            height: 30,
            pixels: vec![255; 8 * 30],
        };
        bitmap.pixels[12 * 8] = 0;
        bitmap.pixels[14 * 8 + 7] = 0;

        assert_eq!(vertical_print_bounds(&bitmap), (4, 23));

        let bytes = escpos_raster_receipt(&bitmap);
        assert!(
            bytes
                .windows(8)
                .any(|part| part == [0x1d, 0x76, 0x30, 0x00, 1, 0, 19, 0])
        );
        assert_eq!(bytes.len(), 2 + 3 + 8 + 19 + 4);
    }

    #[test]
    fn split_display_keeps_separator_with_prefix() {
        assert_eq!(
            split_ring_number("2026-03-0015369"),
            (Some("2026-03"), "0015369")
        );
        assert_eq!(split_ring_number("JNH05-034789"), (Some("JNH05"), "034789"));
        assert_eq!(split_ring_number("0237849"), (None, "0237849"));
    }
}
