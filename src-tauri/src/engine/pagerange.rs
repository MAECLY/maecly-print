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
//!
//! Grammar is only half the contract. `docs/03-requirements/print-settings.md`
//! §`page_ranges` also requires ranges to stay **within document bounds**, and
//! to fail "at validation time, not at print time" — a range past the last page
//! spools an empty job, which is the worst kind of failure (silent). That check
//! needs the document, which the grammar parser does not have, so it lives in
//! [`check_bounds`] / [`parse_within`] and is applied by the submit path once
//! the per-file page count is known.

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

/// Reject spans that fall outside a document of `pages` pages.
///
/// `pages == None` means the page count is unknown (an oversized or unreadable
/// PDF — see `crate::queue::probe_pages`) and the range is let through: we
/// cannot bound what we cannot count, and refusing to print such a document
/// would be worse than trusting the user.
///
/// The rule is deliberately "the whole range must be printable", i.e. every
/// span's `end` must be `<= pages`, not merely its `start`. The looser variant
/// would bless `1-9999` as an idiom for "everything", but that idiom already
/// has a spelling — leave the field blank — and accepting it would let a typo
/// like `1-500` on a 5-page report pass validation while quietly printing 5
/// pages.
pub fn check_bounds(spans: &[(u32, u32)], pages: Option<u32>) -> Result<(), String> {
    let Some(pages) = pages else {
        return Ok(());
    };
    for (start, end) in spans {
        if *end > pages {
            let span = if start == end {
                format!("page {start}")
            } else {
                format!("range {start}-{end}")
            };
            return Err(format!(
                "{span} is outside the document: it only has {pages} {}",
                if pages == 1 { "page" } else { "pages" }
            ));
        }
    }
    Ok(())
}

/// [`parse`] followed by [`check_bounds`] — the full contract for a range that
/// is about to be printed against a document whose length is known.
pub fn parse_within(input: &str, pages: Option<u32>) -> Result<Vec<(u32, u32)>, String> {
    let spans = parse(input)?;
    check_bounds(&spans, pages)?;
    Ok(spans)
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
    fn fr_042_range_past_the_last_page_is_rejected() {
        // print-settings.md §page_ranges: "within document bounds ... fail at
        // validation time, not at print time".
        assert!(parse_within("500-600", Some(8)).is_err());
        assert!(parse_within("9", Some(8)).is_err());
        assert!(parse_within("1-5,20", Some(8)).is_err());
        // The whole span must fit, not just its start.
        assert!(parse_within("1-9999", Some(8)).is_err());
    }

    #[test]
    fn fr_042_range_inside_the_document_is_accepted() {
        assert_eq!(parse_within("1-8", Some(8)).unwrap(), vec![(1, 8)]);
        assert_eq!(parse_within("8", Some(8)).unwrap(), vec![(8, 8)]);
        assert_eq!(
            parse_within("1-3,7", Some(8)).unwrap(),
            vec![(1, 3), (7, 7)]
        );
    }

    #[test]
    fn fr_042_unknown_page_count_never_blocks() {
        // Oversized/unreadable PDFs probe to `None`; we cannot bound them.
        assert!(parse_within("500-600", None).is_ok());
        assert!(check_bounds(&[(1, u32::MAX)], None).is_ok());
    }

    #[test]
    fn fr_042_single_page_image_only_accepts_page_one() {
        assert!(parse_within("1", Some(1)).is_ok());
        assert!(parse_within("2", Some(1)).is_err());
    }

    #[test]
    fn fr_042_bounds_error_names_the_span_and_the_total() {
        let err = parse_within("2-3", Some(1)).unwrap_err();
        assert!(err.contains("2-3"), "names the span: {err}");
        assert!(err.contains("1 page"), "singular page total: {err}");

        let err = parse_within("9", Some(8)).unwrap_err();
        assert!(err.contains("page 9"), "names the page: {err}");
        assert!(err.contains("8 pages"), "plural page total: {err}");
    }

    #[test]
    fn fr_042_grammar_errors_still_win_over_bounds() {
        // A malformed range is reported as malformed, not as out-of-bounds.
        let err = parse_within("5-1", Some(100)).unwrap_err();
        assert!(err.contains("backwards"), "{err}");
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
