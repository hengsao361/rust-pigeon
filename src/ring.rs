use std::collections::BTreeSet;

const MIN_REGION_CODE: u8 = 1;
const MAX_REGION_CODE: u8 = 33;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingNumber(String);

impl RingNumber {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse_exact(input: &str) -> Option<Self> {
        let normalized = normalize_candidate(input);
        let parts: Vec<_> = normalized.split('-').collect();

        let valid = match parts.as_slice() {
            [serial] => valid_serial(serial),
            [region, serial] => valid_region(region) && valid_serial(serial),
            [year, region, serial] => {
                valid_year(year) && valid_region(region) && valid_serial(serial)
            }
            _ => false,
        };

        valid.then_some(Self(normalized))
    }
}

pub fn extract_ring_numbers(raw: &str) -> Vec<RingNumber> {
    let normalized = collapse_hyphen_whitespace(&normalize_text(raw));
    let tokens: Vec<_> = normalized.split_whitespace().collect();
    let mut found = BTreeSet::new();
    let mut index = 0;

    while index < tokens.len() {
        let mut consumed = 0;
        for width in (1..=3.min(tokens.len() - index)).rev() {
            let candidate = tokens[index..index + width].join("-");
            if let Some(ring) = RingNumber::parse_exact(&candidate) {
                found.insert(ring.0);
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
                if let Some(ring) = RingNumber::parse_exact(&candidate) {
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

    found.into_iter().map(RingNumber).collect()
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
    value.len() == 7 && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_all_supported_shapes() {
        for value in [
            "2026-10-0178948",
            "2026-JN-0387764",
            "03-0482758",
            "JN-0347895",
            "0237849",
        ] {
            assert_eq!(RingNumber::parse_exact(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn rejects_unofficial_regions_and_bad_shapes() {
        for value in [
            "2026-00-0178948",
            "2026-34-0178948",
            "34-0178948",
            "2026-10-178948",
            "2026-0178948",
            "12345678",
        ] {
            assert!(RingNumber::parse_exact(value).is_none(), "{value}");
        }
    }

    #[test]
    fn extracts_from_labels_and_normalizes_common_ocr_errors() {
        let values = extract_ring_numbers("足环号： 2026－1O－O178948\nignore me");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].as_str(), "2026-10-0178948");
    }

    #[test]
    fn extracts_jn_case_insensitively() {
        let values = extract_ring_numbers("Ring: jn - 0347895");
        assert_eq!(values[0].as_str(), "JN-0347895");
    }

    #[test]
    fn joins_ring_numbers_split_across_lines() {
        for (raw, expected) in [
            ("2025-02\n0348234", "2025-02-0348234"),
            ("2026\n03-0498274", "2026-03-0498274"),
            ("2027\nJN\n1234567", "2027-JN-1234567"),
            ("RING NO.\n04\n7654321", "04-7654321"),
        ] {
            let values = extract_ring_numbers(raw);
            assert_eq!(values.len(), 1, "{raw:?}");
            assert_eq!(values[0].as_str(), expected);
        }
    }

    #[test]
    fn returns_unique_sorted_matches() {
        let values = extract_ring_numbers("0237849 / 0237849 / 03-0482758");
        let text: Vec<_> = values.iter().map(RingNumber::as_str).collect();
        assert_eq!(text, ["0237849", "03-0482758"]);
    }

    #[test]
    fn does_not_downgrade_a_malformed_long_number_to_a_shorter_valid_shape() {
        assert!(extract_ring_numbers("RING2026-10-0178948").is_empty());
        assert!(extract_ring_numbers("2026-99-0178948").is_empty());
        assert_eq!(
            extract_ring_numbers("RING-03-0482758")[0].as_str(),
            "03-0482758"
        );
    }
}
