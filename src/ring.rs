use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MIN_REGION_CODE: u8 = 1;
const MAX_REGION_CODE: u8 = 33;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrFormat {
    #[default]
    #[serde(
        alias = "single_line_three_part",
        alias = "multiline_three_part",
        alias = "single_line_two_part",
        alias = "single_line_digits7"
    )]
    General,
    SingleLineDigits6,
    SingleLineDigits5,
    SingleLineDigits4,
}

impl OcrFormat {
    pub const ALL: [Self; 4] = [
        Self::General,
        Self::SingleLineDigits6,
        Self::SingleLineDigits5,
        Self::SingleLineDigits4,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::SingleLineDigits6 => "单行纯数字6位",
            Self::SingleLineDigits5 => "单行纯数字5位",
            Self::SingleLineDigits4 => "单行纯数字4位",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ParseMode {
    GeneralSingleLine,
    StandardThreePart,
    Digits(usize),
}

impl ParseMode {
    const fn max_candidate_tokens(self) -> usize {
        match self {
            Self::GeneralSingleLine | Self::StandardThreePart => 13,
            Self::Digits(length) => length,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingNumber(String);

impl RingNumber {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse_exact(input: &str, format: OcrFormat) -> Option<Self> {
        let mode = match format {
            OcrFormat::General => ParseMode::GeneralSingleLine,
            OcrFormat::SingleLineDigits6 => ParseMode::Digits(6),
            OcrFormat::SingleLineDigits5 => ParseMode::Digits(5),
            OcrFormat::SingleLineDigits4 => ParseMode::Digits(4),
        };
        parse_exact_mode(input, mode)
    }
}

pub fn extract_ring_numbers(raw: &str, format: OcrFormat) -> Vec<RingNumber> {
    let mut found = BTreeSet::new();
    match format {
        OcrFormat::General => extract_general(raw, &mut found),
        OcrFormat::SingleLineDigits6 => extract_each_line(raw, ParseMode::Digits(6), &mut found),
        OcrFormat::SingleLineDigits5 => extract_each_line(raw, ParseMode::Digits(5), &mut found),
        OcrFormat::SingleLineDigits4 => extract_each_line(raw, ParseMode::Digits(4), &mut found),
    }
    found.into_iter().map(RingNumber).collect()
}

fn extract_general(raw: &str, found: &mut BTreeSet<String>) {
    extract_each_line(raw, ParseMode::GeneralSingleLine, found);

    let mut three_part = BTreeSet::new();
    extract_from_segment(raw, ParseMode::StandardThreePart, &mut three_part);
    for full_number in &three_part {
        if let Some((_, two_part)) = full_number.split_once('-') {
            found.remove(two_part);
        }
        if let Some((_, serial)) = full_number.rsplit_once('-') {
            found.remove(serial);
        }
    }
    found.extend(three_part);
}

fn extract_each_line(raw: &str, mode: ParseMode, found: &mut BTreeSet<String>) {
    for line in raw.lines() {
        extract_from_segment(line, mode, found);
    }
}

fn extract_from_segment(raw: &str, mode: ParseMode, found: &mut BTreeSet<String>) {
    let normalized = collapse_hyphen_whitespace(&normalize_text(raw));
    let tokens: Vec<_> = normalized.split_whitespace().collect();
    let mut index = 0;

    while index < tokens.len() {
        let mut consumed = 0;
        let max_width = mode.max_candidate_tokens().min(tokens.len() - index);
        for width in (1..=max_width).rev() {
            let mut matched = false;
            for candidate in candidate_variants(&tokens[index..index + width]) {
                if let Some(ring) = parse_exact_mode(&candidate, mode) {
                    found.insert(ring.0);
                    matched = true;
                }
            }
            if matched {
                consumed = width;
                break;
            }
        }

        if consumed > 0 {
            index += consumed;
            continue;
        }

        let token = tokens[index];
        let parts: Vec<_> = token.split('-').filter(|part| !part.is_empty()).collect();
        for width in [3, 2, 1] {
            if parts.len() < width {
                continue;
            }
            let mut found_at_this_width = false;
            for start in 0..=parts.len() - width {
                let end = start + width;
                let outside_contains_digits = parts[..start]
                    .iter()
                    .chain(parts[end..].iter())
                    .any(|part| part.bytes().any(|byte| byte.is_ascii_digit()));
                if outside_contains_digits {
                    continue;
                }
                let candidate = parts[start..end].join("-");
                if let Some(ring) = parse_exact_mode(&candidate, mode) {
                    found.insert(ring.0);
                    found_at_this_width = true;
                }
            }
            if found_at_this_width {
                break;
            }
        }
        index += 1;
    }
}

fn parse_exact_mode(input: &str, mode: ParseMode) -> Option<RingNumber> {
    let normalized = normalize_candidate(input);
    let parts: Vec<_> = normalized.split('-').collect();

    let valid = match (mode, parts.as_slice()) {
        (ParseMode::StandardThreePart, [year, region, serial]) => {
            valid_standard_three_part(year, region, serial)
        }
        (ParseMode::GeneralSingleLine, [year, region, serial]) => {
            valid_standard_three_part(year, region, serial)
        }
        (ParseMode::GeneralSingleLine, [prefix, serial]) => {
            (valid_region(prefix) && valid_digits(serial, 7))
                || (valid_memorial_prefix(prefix) && valid_digits(serial, 6))
        }
        (ParseMode::GeneralSingleLine, [digits]) => valid_digits(digits, 7),
        (ParseMode::Digits(length), [digits]) => valid_digits(digits, length),
        _ => false,
    };

    valid.then_some(RingNumber(normalized))
}

fn valid_standard_three_part(year: &str, region: &str, serial: &str) -> bool {
    valid_year(year) && valid_region(region) && valid_digits(serial, 7)
}

fn candidate_variants(tokens: &[&str]) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let gap_count = tokens.len() - 1;
    let variant_count = 1usize << gap_count;
    let mut variants = Vec::with_capacity(variant_count);

    for mask in 0..variant_count {
        let mut candidate = tokens[0].to_owned();
        for gap in 0..gap_count {
            if mask & (1 << gap) != 0 {
                candidate.push('-');
            }
            candidate.push_str(tokens[gap + 1]);
        }
        variants.push(candidate);
    }

    variants
}

fn collapse_hyphen_whitespace(input: &str) -> String {
    let mut value = input.to_owned();
    loop {
        let next = value.replace(" -", "-").replace("- ", "-");
        if next == value {
            return value;
        }
        value = next;
    }
}

fn normalize_text(input: &str) -> String {
    input
        .chars()
        .map(|ch| match ch {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}'
            | '\u{ff0d}' => '-',
            '０'..='９' => char::from_u32('0' as u32 + (ch as u32 - '０' as u32)).unwrap(),
            c if c.is_ascii_alphanumeric() || c == '-' => c.to_ascii_uppercase(),
            _ => ' ',
        })
        .collect()
}

fn normalize_candidate(input: &str) -> String {
    normalize_text(input)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
        .split('-')
        .map(normalize_part)
        .collect::<Vec<_>>()
        .join("-")
}

fn normalize_part(part: &str) -> String {
    part.chars()
        .map(|ch| match ch.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

fn valid_year(value: &str) -> bool {
    value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit()) && value.starts_with("20")
}

fn valid_region(value: &str) -> bool {
    if value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    value
        .parse::<u8>()
        .is_ok_and(|code| (MIN_REGION_CODE..=MAX_REGION_CODE).contains(&code))
}

fn valid_memorial_prefix(value: &str) -> bool {
    value
        .strip_prefix("JNH")
        .or_else(|| value.strip_prefix("JN"))
        .is_some_and(|code| valid_digits(code, 2))
}

fn valid_digits(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_accepts_all_supported_standard_and_memorial_shapes() {
        for value in [
            "2026-10-0178948",
            "03-0482758",
            "0237849",
            "JN05-123456",
            "JNH99-654321",
        ] {
            assert_eq!(
                RingNumber::parse_exact(value, OcrFormat::General)
                    .unwrap()
                    .as_str(),
                value
            );
        }
    }

    #[test]
    fn dedicated_digit_modes_accept_only_the_selected_length() {
        for (format, value) in [
            (OcrFormat::SingleLineDigits6, "237849"),
            (OcrFormat::SingleLineDigits5, "37849"),
            (OcrFormat::SingleLineDigits4, "7849"),
        ] {
            assert_eq!(
                RingNumber::parse_exact(value, format).unwrap().as_str(),
                value
            );
        }

        assert!(RingNumber::parse_exact("1234567", OcrFormat::SingleLineDigits6).is_none());
        assert!(RingNumber::parse_exact("123456", OcrFormat::SingleLineDigits5).is_none());
        assert!(RingNumber::parse_exact("12345", OcrFormat::SingleLineDigits4).is_none());
    }

    #[test]
    fn standard_rings_require_numeric_official_region_codes() {
        for value in [
            "2026-00-0178948",
            "2026-34-0178948",
            "2026-JN-0178948",
            "JN-0482758",
        ] {
            assert!(
                RingNumber::parse_exact(value, OcrFormat::General).is_none(),
                "{value}"
            );
        }
    }

    #[test]
    fn memorial_rings_require_prefix_code_hyphen_and_six_digits() {
        for value in [
            "JN5-123456",
            "JN005-123456",
            "JN05-1234567",
            "JNH05-12345",
            "JNAA-123456",
            "JN05-123-456",
        ] {
            assert!(
                RingNumber::parse_exact(value, OcrFormat::General).is_none(),
                "{value}"
            );
        }

        let values = extract_ring_numbers("纪念环 jnh O5 - 123 456", OcrFormat::General);
        assert_eq!(values[0].as_str(), "JNH05-123456");
    }

    #[test]
    fn extracts_labels_and_repairs_common_ocr_errors() {
        let values =
            extract_ring_numbers("足环号： 2026－1O－O178948\nignore me", OcrFormat::General);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].as_str(), "2026-10-0178948");
    }

    #[test]
    fn multiline_three_part_takes_precedence_over_its_shorter_components() {
        for (raw, expected) in [
            ("2025-04-0347618", "2025-04-0347618"),
            ("2025-02\n0348234", "2025-02-0348234"),
            ("2025-04-\n0347618", "2025-04-0347618"),
            ("2026\n03-0498274", "2026-03-0498274"),
            ("2026\n0 1\n038 7 764", "2026-01-0387764"),
        ] {
            let values = extract_ring_numbers(raw, OcrFormat::General);
            assert_eq!(values.len(), 1, "{raw:?}");
            assert_eq!(values[0].as_str(), expected, "{raw:?}");
        }
    }

    #[test]
    fn only_standard_three_part_can_cross_lines() {
        assert!(extract_ring_numbers("023\n7849", OcrFormat::General).is_empty());
        assert!(extract_ring_numbers("JN05-\n123456", OcrFormat::General).is_empty());
        assert_eq!(
            extract_ring_numbers("03-\n0482758", OcrFormat::General)[0].as_str(),
            "0482758"
        );
    }

    #[test]
    fn repairs_ocr_whitespace_within_single_line_formats() {
        for (raw, expected) in [
            ("203 1 - 0 1 - 001 5 369", "2031-01-0015369"),
            ("2031- 11-0015369", "2031-11-0015369"),
            ("1 1 - 001 2 345", "11-0012345"),
            ("023 1 849", "0231849"),
        ] {
            let values = extract_ring_numbers(raw, OcrFormat::General);
            assert_eq!(values.len(), 1, "{raw:?}");
            assert_eq!(values[0].as_str(), expected, "{raw:?}");
        }

        for (format, raw, expected) in [
            (OcrFormat::SingleLineDigits6, "23 7 849", "237849"),
            (OcrFormat::SingleLineDigits5, "3 7 849", "37849"),
            (OcrFormat::SingleLineDigits4, "7 849", "7849"),
        ] {
            let values = extract_ring_numbers(raw, format);
            assert_eq!(values.len(), 1, "{raw:?}");
            assert_eq!(values[0].as_str(), expected, "{raw:?}");
        }
    }

    #[test]
    fn does_not_merge_two_serials() {
        let values = extract_ring_numbers("1234567 7654321", OcrFormat::General);
        let text: Vec<_> = values.iter().map(RingNumber::as_str).collect();
        assert_eq!(text, ["1234567", "7654321"]);
    }

    #[test]
    fn returns_unique_sorted_matches_in_general_mode() {
        let values = extract_ring_numbers(
            "0237849 / 0237849 / 7654321 / 03-0482758 / JN05-123456",
            OcrFormat::General,
        );
        let text: Vec<_> = values.iter().map(RingNumber::as_str).collect();
        assert_eq!(text, ["0237849", "03-0482758", "7654321", "JN05-123456"]);
    }

    #[test]
    fn malformed_long_numbers_do_not_downgrade_to_valid_subnumbers() {
        assert!(extract_ring_numbers("RING2026-10-0178948", OcrFormat::General).is_empty());
        assert!(extract_ring_numbers("2026-99-0178948", OcrFormat::General).is_empty());
        assert_eq!(
            extract_ring_numbers("RING-03-0482758", OcrFormat::General)[0].as_str(),
            "03-0482758"
        );
    }
}
