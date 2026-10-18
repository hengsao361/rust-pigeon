use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MIN_REGION_CODE: u8 = 1;
const MAX_REGION_CODE: u8 = 33;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrFormat {
    #[default]
    SingleLineThreePart,
    MultilineThreePart,
    SingleLineTwoPart,
    SingleLineDigits7,
    SingleLineDigits6,
    SingleLineDigits5,
    SingleLineDigits4,
}

impl OcrFormat {
    pub const ALL: [Self; 7] = [
        Self::SingleLineThreePart,
        Self::MultilineThreePart,
        Self::SingleLineTwoPart,
        Self::SingleLineDigits7,
        Self::SingleLineDigits6,
        Self::SingleLineDigits5,
        Self::SingleLineDigits4,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SingleLineThreePart => "单行三段",
            Self::MultilineThreePart => "跨行三段",
            Self::SingleLineTwoPart => "单行两段",
            Self::SingleLineDigits7 => "单行纯数字7位",
            Self::SingleLineDigits6 => "单行纯数字6位",
            Self::SingleLineDigits5 => "单行纯数字5位",
            Self::SingleLineDigits4 => "单行纯数字4位",
        }
    }

    const fn combines_lines(self) -> bool {
        matches!(self, Self::MultilineThreePart)
    }

    const fn max_candidate_tokens(self) -> usize {
        match self {
            Self::SingleLineThreePart | Self::MultilineThreePart => 13,
            Self::SingleLineTwoPart => 9,
            Self::SingleLineDigits7 => 7,
            Self::SingleLineDigits6 => 6,
            Self::SingleLineDigits5 => 5,
            Self::SingleLineDigits4 => 4,
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
        let normalized = normalize_candidate(input);
        let parts: Vec<_> = normalized.split('-').collect();

        let valid = match (format, parts.as_slice()) {
            (
                OcrFormat::SingleLineThreePart | OcrFormat::MultilineThreePart,
                [year, region, serial],
            ) => valid_year(year) && valid_region(region) && valid_serial(serial),
            (OcrFormat::SingleLineTwoPart, [region, serial]) => {
                valid_region(region) && valid_serial(serial)
            }
            (OcrFormat::SingleLineDigits7, [digits]) => valid_digits(digits, 7),
            (OcrFormat::SingleLineDigits6, [digits]) => valid_digits(digits, 6),
            (OcrFormat::SingleLineDigits5, [digits]) => valid_digits(digits, 5),
            (OcrFormat::SingleLineDigits4, [digits]) => valid_digits(digits, 4),
            _ => false,
        };

        valid.then_some(Self(normalized))
    }
}

pub fn extract_ring_numbers(raw: &str, format: OcrFormat) -> Vec<RingNumber> {
    let mut found = BTreeSet::new();
    if format.combines_lines() {
        extract_from_segment(raw, format, &mut found);
    } else {
        for line in raw.lines() {
            extract_from_segment(line, format, &mut found);
        }
    }

    found.into_iter().map(RingNumber).collect()
}

fn extract_from_segment(raw: &str, format: OcrFormat, found: &mut BTreeSet<String>) {
    let normalized = collapse_hyphen_whitespace(&normalize_text(raw));
    let tokens: Vec<_> = normalized.split_whitespace().collect();
    let mut index = 0;

    while index < tokens.len() {
        let mut consumed = 0;
        let max_width = format.max_candidate_tokens().min(tokens.len() - index);
        for width in (1..=max_width).rev() {
            let mut matched = false;
            for candidate in candidate_variants(&tokens[index..index + width]) {
                if let Some(ring) = RingNumber::parse_exact(&candidate, format) {
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
                let candidate = parts[start..start + width].join("-");
                if let Some(ring) = RingNumber::parse_exact(&candidate, format) {
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
    if part.eq_ignore_ascii_case("JN") {
        return "JN".to_owned();
    }

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
    if value == "JN" {
        return true;
    }
    if value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    value
        .parse::<u8>()
        .is_ok_and(|code| (MIN_REGION_CODE..=MAX_REGION_CODE).contains(&code))
}

fn valid_serial(value: &str) -> bool {
    valid_digits(value, 7)
}

fn valid_digits(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_each_configured_format() {
        for (format, value) in [
            (OcrFormat::SingleLineThreePart, "2026-10-0178948"),
            (OcrFormat::MultilineThreePart, "2026-JN-0387764"),
            (OcrFormat::SingleLineTwoPart, "03-0482758"),
            (OcrFormat::SingleLineDigits7, "0237849"),
            (OcrFormat::SingleLineDigits6, "237849"),
            (OcrFormat::SingleLineDigits5, "37849"),
            (OcrFormat::SingleLineDigits4, "7849"),
        ] {
            assert_eq!(
                RingNumber::parse_exact(value, format).unwrap().as_str(),
                value
            );
        }
    }

    #[test]
    fn selected_format_rejects_other_shapes_and_invalid_values() {
        for (format, value) in [
            (OcrFormat::SingleLineThreePart, "2026-00-0178948"),
            (OcrFormat::SingleLineThreePart, "2026-34-0178948"),
            (OcrFormat::SingleLineThreePart, "03-0482758"),
            (OcrFormat::SingleLineTwoPart, "2026-03-0482758"),
            (OcrFormat::SingleLineTwoPart, "34-0482758"),
            (OcrFormat::SingleLineDigits7, "123456"),
            (OcrFormat::SingleLineDigits6, "1234567"),
            (OcrFormat::SingleLineDigits5, "123456"),
            (OcrFormat::SingleLineDigits4, "12345"),
        ] {
            assert!(RingNumber::parse_exact(value, format).is_none(), "{value}");
        }
    }

    #[test]
    fn extracts_from_labels_and_normalizes_common_ocr_errors() {
        let values = extract_ring_numbers(
            "足环号： 2026－1O－O178948\nignore me",
            OcrFormat::SingleLineThreePart,
        );
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].as_str(), "2026-10-0178948");
    }

    #[test]
    fn extracts_jn_case_insensitively() {
        let values = extract_ring_numbers("Ring: jn - 0347895", OcrFormat::SingleLineTwoPart);
        assert_eq!(values[0].as_str(), "JN-0347895");
    }

    #[test]
    fn multiline_three_part_joins_supported_line_layouts() {
        for (raw, expected) in [
            ("2025-04-0347618", "2025-04-0347618"),
            ("2025-02\n0348234", "2025-02-0348234"),
            ("2025-04-\n0347618", "2025-04-0347618"),
            ("2026\n03-0498274", "2026-03-0498274"),
            ("2027\nJN\n1234567", "2027-JN-1234567"),
            ("2026\n0 1\n038 7 764", "2026-01-0387764"),
        ] {
            let values = extract_ring_numbers(raw, OcrFormat::MultilineThreePart);
            assert_eq!(values.len(), 1, "{raw:?}");
            assert_eq!(values[0].as_str(), expected);
        }

        assert!(extract_ring_numbers("03-\n4857234", OcrFormat::MultilineThreePart).is_empty());
    }

    #[test]
    fn single_line_modes_never_combine_lines() {
        assert!(
            extract_ring_numbers("2025-02\n0348234", OcrFormat::SingleLineThreePart).is_empty()
        );
        assert!(extract_ring_numbers("03-\n0482758", OcrFormat::SingleLineTwoPart).is_empty());
        assert!(extract_ring_numbers("023\n7849", OcrFormat::SingleLineDigits7).is_empty());
    }

    #[test]
    fn repairs_ocr_whitespace_within_the_selected_single_line_format() {
        for (raw, expected) in [
            ("203 1 - 0 1 - 001 5 369", "2031-01-0015369"),
            ("2031- 11-0015369", "2031-11-0015369"),
        ] {
            let values = extract_ring_numbers(raw, OcrFormat::SingleLineThreePart);
            assert_eq!(values.len(), 1, "{raw:?}");
            assert_eq!(values[0].as_str(), expected, "{raw:?}");
        }

        let values = extract_ring_numbers("1 1 - 001 2 345", OcrFormat::SingleLineTwoPart);
        assert_eq!(values[0].as_str(), "11-0012345");

        for (format, raw, expected) in [
            (OcrFormat::SingleLineDigits7, "023 1 849", "0231849"),
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
        let values = extract_ring_numbers("1234567 7654321", OcrFormat::SingleLineDigits7);
        let text: Vec<_> = values.iter().map(RingNumber::as_str).collect();
        assert_eq!(text, ["1234567", "7654321"]);
    }

    #[test]
    fn returns_unique_sorted_matches_for_the_selected_format() {
        let values = extract_ring_numbers(
            "0237849 / 0237849 / 7654321 / 03-0482758",
            OcrFormat::SingleLineDigits7,
        );
        let text: Vec<_> = values.iter().map(RingNumber::as_str).collect();
        assert_eq!(text, ["0237849", "7654321"]);
    }

    #[test]
    fn does_not_downgrade_a_malformed_long_number_to_a_shorter_valid_shape() {
        assert!(
            extract_ring_numbers("RING2026-10-0178948", OcrFormat::SingleLineThreePart).is_empty()
        );
        assert!(extract_ring_numbers("2026-99-0178948", OcrFormat::SingleLineThreePart).is_empty());
        assert_eq!(
            extract_ring_numbers("RING-03-0482758", OcrFormat::SingleLineTwoPart)[0].as_str(),
            "03-0482758"
        );
    }
}
