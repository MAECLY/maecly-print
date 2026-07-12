//! Page-range parsing for expressions like `"1-5,10,12-14"`.
//!
//! This is the single source of truth for page-range *syntax* (the
//! [`crate::queue::job::JobSettings::validate`] layer deliberately only checks
//! non-emptiness and defers the grammar here). The parser is strict and its
//! error messages are user-facing — they are shown inline in the print dialog —
//! so they name the offending token and say what is wrong.
//!
//! Grammar:
//! - Comma-separated entries; surrounding and interior whitespace is ignored.
//! - An entry is either a single page `N` or a span `A-B`.
//! - Pages are 1-based; `0` is rejected.
//! - A span must be ascending (`5-1` is an error) and closed (`5-` and `-5` are
//!   errors — MVP has no open-ended or from-the-end ranges).
//! - Empty input, whitespace-only input, and stray empty entries (`",,"`,
//!   `"1,,3"`) are errors.
//!
//! Overlapping or adjacent spans are allowed and normalised: the result is
//! sorted and coalesced (e.g. `"1-5,3-8"` → `[(1, 8)]`, `"1-3,4-6"` →
//! `[(1, 6)]`).

/// Parse a page-range expression into sorted, coalesced inclusive `(start, end)`
/// spans (both 1-based). Returns a human-readable error describing the first
/// problem found.
pub fn parse(input: &str) -> Result<Vec<(u32, u32)>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("page range is empty".to_string());
    }

    let mut spans = Vec::new();
    for (i, raw) in trimmed.split(',').enumerate() {
        let entry = raw.trim();
        if entry.is_empty() {
            return Err(format!(
                "page range has an empty entry at position {}",
                i + 1
            ));
        }
        spans.push(parse_entry(entry)?);
    }

    Ok(normalize(spans))
}

/// Convenience predicate: `true` iff [`parse`] would succeed.
pub fn is_valid(input: &str) -> bool {
    parse(input).is_ok()
}

/// Parse one comma-free entry — either `"N"` or `"A-B"`.
fn parse_entry(entry: &str) -> Result<(u32, u32), String> {
    match entry.split_once('-') {
        Some((lhs, rhs)) => {
            let (lhs, rhs) = (lhs.trim(), rhs.trim());
            if lhs.is_empty() || rhs.is_empty() {
                return Err(format!(
                    "'{entry}' is not a complete range — write it as e.g. \"2-5\""
                ));
            }
            let start = parse_page(lhs)?;
            let end = parse_page(rhs)?;
            if start > end {
                return Err(format!(
                    "range '{entry}' runs backwards: {start} is greater than {end}"
                ));
            }
            Ok((start, end))
        }
        None => {
            let page = parse_page(entry)?;
            Ok((page, page))
        }
    }
}

/// Parse a single 1-based page number, rejecting non-numbers and `0`.
fn parse_page(token: &str) -> Result<u32, String> {
    let page: u32 = token
        .parse()
        .map_err(|_| format!("'{token}' is not a valid page number"))?;
    if page == 0 {
        return Err("page numbers start at 1, not 0".to_string());
    }
    Ok(page)
}

/// Sort spans and merge any that overlap or touch (adjacent), so the output is
/// a minimal, canonical set of disjoint spans.
fn normalize(mut spans: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match merged.last_mut() {
            // `start <= last.end + 1` covers both overlap and adjacency; the
            // saturating add guards the (impossible for valid pages) u32::MAX.
            Some(last) if start <= last.1.saturating_add(1) => {
                if end > last.1 {
                    last.1 = end;
                }
            }
            _ => merged.push((start, end)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_mixed_expression() {
        assert_eq!(
            parse("1-5,10,12-14").unwrap(),
            vec![(1, 5), (10, 10), (12, 14)]
        );
    }

    #[test]
    fn merges_overlapping_and_adjacent_spans() {
        assert_eq!(parse("1-5,3-8").unwrap(), vec![(1, 8)]);
        assert_eq!(parse("1-3,4-6").unwrap(), vec![(1, 6)]); // adjacent
        assert_eq!(parse("5,1,3").unwrap(), vec![(1, 1), (3, 3), (5, 5)]);
        assert_eq!(parse("1,2,3,4").unwrap(), vec![(1, 4)]); // all adjacent
        assert_eq!(parse("10-12,1-3,2").unwrap(), vec![(1, 3), (10, 12)]);
    }

    #[test]
    fn descending_span_is_rejected() {
        assert!(parse("5-1").is_err());
    }

    #[test]
    fn zero_is_rejected() {
        assert!(parse("0").is_err());
        assert!(parse("1-0").is_err());
        assert!(parse("0-3").is_err());
    }

    #[test]
    fn open_ended_span_is_rejected() {
        assert!(parse("5-").is_err());
        assert!(parse("-5").is_err());
        assert!(parse("1-2-3").is_err());
    }

    #[test]
    fn empty_or_comma_only_is_rejected() {
        assert!(parse("").is_err());
        assert!(parse("   ").is_err());
        assert!(parse(",").is_err());
        assert!(parse(",,").is_err());
        assert!(parse("1,,3").is_err());
        assert!(parse("1,").is_err());
    }

    #[test]
    fn non_numeric_is_rejected() {
        assert!(parse("abc").is_err());
        assert!(parse("1-x").is_err());
    }

    #[test]
    fn whitespace_is_tolerated_everywhere() {
        assert_eq!(parse(" 1 - 3 , 7 ").unwrap(), vec![(1, 3), (7, 7)]);
        assert_eq!(parse("  1-5 ,  10  ").unwrap(), vec![(1, 5), (10, 10)]);
    }

    #[test]
    fn is_valid_matches_parse() {
        assert!(is_valid("1-3"));
        assert!(is_valid("1-5,10,12-14"));
        assert!(!is_valid("3-1"));
        assert!(!is_valid(""));
        assert!(!is_valid("0"));
    }
}
