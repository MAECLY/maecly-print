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
//!
//! Every error is a [`LocalizedError`] of the `InvalidRequest` family with a
//! stable reason (`page_range_empty`, `page_range_backwards`, …) and the
//! params its sentence needs (FR-092), so the dialog, a context-menu print and
//! the CLI's JSON all name the problem the same way in the user's language.
//! The English `message` is the CLI's text and the log's.

use crate::util::error::{IpcErrorCode, LocalizedError};

/// An `InvalidRequest` page-range error with `reason` and its English text.
fn invalid(reason: &'static str, message: impl Into<String>) -> LocalizedError {
    LocalizedError::new(IpcErrorCode::InvalidRequest, reason, message)
}

/// Parse a page-range expression into sorted, coalesced inclusive `(start, end)`
/// spans (both 1-based). Returns an error describing the first problem found.
pub fn parse(input: &str) -> Result<Vec<(u32, u32)>, LocalizedError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(invalid("page_range_empty", "page range is empty"));
    }

    let mut spans = Vec::new();
    for (i, raw) in trimmed.split(',').enumerate() {
        let entry = raw.trim();
        if entry.is_empty() {
            return Err(invalid(
                "page_range_empty_entry",
                format!("page range has an empty entry at position {}", i + 1),
            )
            .param("position", i + 1));
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
///
/// The error is `page_range_outside` with `span` (`9` or `2-3`, written the
/// way the user types it, so it reads the same in every language) and the
/// numeric `count` its plural sentence picks a form by. The caller adds the
/// file's `name` (`ipc::jobs::check_ranges_fit_documents`).
pub fn check_bounds(spans: &[(u32, u32)], pages: Option<u32>) -> Result<(), LocalizedError> {
    let Some(pages) = pages else {
        return Ok(());
    };
    for (start, end) in spans {
        if *end > pages {
            let (span, label) = if start == end {
                (start.to_string(), format!("page {start}"))
            } else {
                (format!("{start}-{end}"), format!("range {start}-{end}"))
            };
            return Err(invalid(
                "page_range_outside",
                format!(
                    "{label} is outside the document: it only has {pages} {}",
                    if pages == 1 { "page" } else { "pages" }
                ),
            )
            .param("span", span)
            .param("count", pages));
        }
    }
    Ok(())
}

/// [`parse`] followed by [`check_bounds`] — the full contract for a range that
/// is about to be printed against a document whose length is known.
pub fn parse_within(input: &str, pages: Option<u32>) -> Result<Vec<(u32, u32)>, LocalizedError> {
    let spans = parse(input)?;
    check_bounds(&spans, pages)?;
    Ok(spans)
}

/// Parse one comma-free entry — either `"N"` or `"A-B"`.
fn parse_entry(entry: &str) -> Result<(u32, u32), LocalizedError> {
    match entry.split_once('-') {
        Some((lhs, rhs)) => {
            let (lhs, rhs) = (lhs.trim(), rhs.trim());
            if lhs.is_empty() || rhs.is_empty() {
                return Err(invalid(
                    "page_range_incomplete",
                    format!("'{entry}' is not a complete range — write it as e.g. \"2-5\""),
                )
                .param("entry", entry));
            }
            let start = parse_page(lhs)?;
            let end = parse_page(rhs)?;
            if start > end {
                return Err(invalid(
                    "page_range_backwards",
                    format!("range '{entry}' runs backwards: {start} is greater than {end}"),
                )
                .param("entry", entry)
                .param("start", start)
                .param("end", end));
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
fn parse_page(token: &str) -> Result<u32, LocalizedError> {
    let page: u32 = token.parse().map_err(|_| {
        invalid(
            "page_number_invalid",
            format!("'{token}' is not a valid page number"),
        )
        .param("token", token)
    })?;
    if page == 0 {
        return Err(invalid(
            "page_number_zero",
            "page numbers start at 1, not 0",
        ));
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
        let err = parse_within("2-3", Some(1)).unwrap_err().message;
        assert!(err.contains("2-3"), "names the span: {err}");
        assert!(err.contains("1 page"), "singular page total: {err}");

        let err = parse_within("9", Some(8)).unwrap_err().message;
        assert!(err.contains("page 9"), "names the page: {err}");
        assert!(err.contains("8 pages"), "plural page total: {err}");
    }

    #[test]
    fn fr_042_grammar_errors_still_win_over_bounds() {
        // A malformed range is reported as malformed, not as out-of-bounds.
        let err = parse_within("5-1", Some(100)).unwrap_err();
        assert!(err.message.contains("backwards"), "{err}");
        assert_eq!(err.reason, "page_range_backwards");
    }

    /// FR-092: every grammar and bounds error carries its stable reason and
    /// the params its EN/ES sentence interpolates.
    #[test]
    fn fr_092_page_range_errors_carry_reason_and_params() {
        let cases: &[(&str, Option<u32>, &str, &[(&str, &str)])] = &[
            ("  ", None, "page_range_empty", &[]),
            ("1,,3", None, "page_range_empty_entry", &[("position", "2")]),
            ("5-", None, "page_range_incomplete", &[("entry", "5-")]),
            (
                "5-1",
                None,
                "page_range_backwards",
                &[("entry", "5-1"), ("start", "5"), ("end", "1")],
            ),
            ("x", None, "page_number_invalid", &[("token", "x")]),
            ("0", None, "page_number_zero", &[]),
            (
                "2-3",
                Some(1),
                "page_range_outside",
                &[("span", "2-3"), ("count", "1")],
            ),
            (
                "9",
                Some(8),
                "page_range_outside",
                &[("span", "9"), ("count", "8")],
            ),
        ];
        for (input, pages, reason, params) in cases {
            let err = parse_within(input, *pages).unwrap_err();
            assert_eq!(err.code, IpcErrorCode::InvalidRequest, "{input}");
            assert_eq!(err.reason, *reason, "{input}");
            for (k, v) in *params {
                assert_eq!(
                    err.params.get(*k).map(String::as_str),
                    Some(*v),
                    "{input}: {k}"
                );
            }
        }
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
