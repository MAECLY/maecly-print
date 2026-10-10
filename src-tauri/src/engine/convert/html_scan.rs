//! A small HTML scanner for staging pages ([`super::html`]): enough of the
//! HTML tokenizer to tell real tags from text inside comments, `<!...>`
//! declarations and raw-text elements (`<script>`, `<style>`, `<textarea>`,
//! `<title>`...), and to find every reference a page could make the browser
//! fallback fetch from a network share.
//!
//! Two different jobs, with two different rules:
//!
//! - **Where to splice, which `<base>` is real** ([`insertion_point`],
//!   [`own_base_href`]) follows the tokenizer: a `<head>` or `<base>` written
//!   inside `<!-- -->` or a script is text, not a tag (a security finding: a
//!   page beginning `<!-- <head> -->` used to get its Content-Security-Policy
//!   spliced inside the comment, where it does nothing).
//! - **Which references to neutralise** ([`neutralize_share_refs`]) must find
//!   a *superset* of what the browser sees, so it does not trust its own idea
//!   of where comments end: it parses a tag at every `<` followed by a letter,
//!   anywhere, and looks at every attribute value, every `<style>` block and
//!   every CSS `url()`/`@import`/string. A false positive only empties an
//!   attribute that was never going to load anything; a miss would hand the
//!   user's NTLM credentials to a share. Shares are recognised by the one
//!   shared predicate, [`super::local_ref`].

use std::ops::Range;

use super::local_ref;

/// Elements whose content is text, not markup, up to their end tag.
const RAW_TEXT: &[&str] = &[
    "script", "style", "xmp", "iframe", "noembed", "noframes", "textarea", "title",
];

/// One attribute of a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    /// Lowercase name.
    pub name: String,
    /// The value with character references decoded.
    pub value: String,
    /// The raw value's byte range in the document (inside the quotes); an
    /// empty range at the end of the name for an attribute without a value.
    pub raw: Range<usize>,
}

/// A start or end tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Lowercase name.
    pub name: String,
    /// Whether this is an end tag (`</x>`).
    pub is_end: bool,
    /// The tag's attributes, in order.
    pub attrs: Vec<Attr>,
    /// Byte range of the whole tag, `<` to `>` inclusive.
    pub span: Range<usize>,
}

impl Tag {
    /// The decoded value of the first attribute named `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }
}

fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn find(s: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > s.len() {
        return None;
    }
    s[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// Parse the tag that starts at `at` (`<name` or `</name`, name starting
/// with an ASCII letter), the way the HTML tokenizer does: attribute names
/// end at whitespace, `/`, `>` or `=`; values are double-quoted,
/// single-quoted or unquoted. `None` when `at` does not start a tag.
pub fn parse_tag(html: &[u8], at: usize) -> Option<Tag> {
    let is_end = html.get(at + 1) == Some(&b'/');
    let name_at = at + if is_end { 2 } else { 1 };
    if !html.get(name_at)?.is_ascii_alphabetic() {
        return None;
    }
    let mut i = name_at;
    while i < html.len() && !is_space(html[i]) && !matches!(html[i], b'/' | b'>') {
        i += 1;
    }
    let name = String::from_utf8_lossy(&html[name_at..i]).to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while i < html.len() && (is_space(html[i]) || html[i] == b'/') {
            i += 1;
        }
        if i >= html.len() {
            return Some(Tag {
                name,
                is_end,
                attrs,
                span: at..html.len(),
            });
        }
        if html[i] == b'>' {
            return Some(Tag {
                name,
                is_end,
                attrs,
                span: at..i + 1,
            });
        }
        // Attribute name (a leading `=` belongs to the name).
        let name_start = i;
        i += 1;
        while i < html.len() && !is_space(html[i]) && !matches!(html[i], b'/' | b'>' | b'=') {
            i += 1;
        }
        let attr_name = String::from_utf8_lossy(&html[name_start..i]).to_ascii_lowercase();
        let mut j = i;
        while j < html.len() && is_space(html[j]) {
            j += 1;
        }
        if html.get(j) != Some(&b'=') {
            attrs.push(Attr {
                name: attr_name,
                value: String::new(),
                raw: i..i,
            });
            i = j;
            continue;
        }
        j += 1;
        while j < html.len() && is_space(html[j]) {
            j += 1;
        }
        let raw = match html.get(j) {
            Some(&q @ (b'"' | b'\'')) => {
                let start = j + 1;
                let end = find(html, &[q], start).unwrap_or(html.len());
                i = (end + 1).min(html.len());
                start..end
            }
            _ => {
                let start = j;
                let mut end = j;
                while end < html.len() && !is_space(html[end]) && html[end] != b'>' {
                    end += 1;
                }
                i = end;
                start..end
            }
        };
        attrs.push(Attr {
            name: attr_name,
            value: decode_char_refs(&String::from_utf8_lossy(&html[raw.clone()])),
            raw,
        });
    }
}

/// The real tags of `html`, in order, skipping comments, `<!...>` and `<?...>`
/// declarations and the content of raw-text elements. Stops at `<plaintext>`.
pub fn tags(html: &[u8]) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(lt) = find(html, b"<", i) {
        let rest = &html[lt..];
        if rest.starts_with(b"<!--") {
            i = comment_end(html, lt);
            continue;
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            i = find(html, b">", lt).map_or(html.len(), |e| e + 1);
            continue;
        }
        if rest.starts_with(b"</") && !rest.get(2).is_some_and(u8::is_ascii_alphabetic) {
            // `</>` is dropped, `</3` is a bogus comment.
            i = find(html, b">", lt).map_or(html.len(), |e| e + 1);
            continue;
        }
        let Some(tag) = parse_tag(html, lt) else {
            i = lt + 1;
            continue;
        };
        i = tag.span.end;
        if !tag.is_end && tag.name == "plaintext" {
            out.push(tag);
            break;
        }
        if !tag.is_end && RAW_TEXT.contains(&tag.name.as_str()) {
            i = raw_text_end(html, i, &tag.name);
        }
        out.push(tag);
    }
    out
}

/// Where the comment starting at `at` (`<!--`) ends, just past its closing
/// `-->` (or `--!>`, or the abrupt `<!-->` / `<!--->`); the end of the input
/// when it is never closed.
fn comment_end(html: &[u8], at: usize) -> usize {
    let body = at + 4;
    if html.get(body) == Some(&b'>') {
        return body + 1;
    }
    if html.get(body..body + 2) == Some(b"->") {
        return body + 2;
    }
    let close = find(html, b"-->", body).map(|e| e + 3);
    let bang = find(html, b"--!>", body).map(|e| e + 4);
    match (close, bang) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => html.len(),
    }
}

/// Where the raw text of element `name`, starting at `from`, ends: at its
/// end tag `</name` followed by whitespace, `/` or `>`.
fn raw_text_end(html: &[u8], from: usize, name: &str) -> usize {
    let needle = format!("</{name}");
    let mut i = from;
    while let Some(at) = html[i..]
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
        .map(|p| i + p)
    {
        let next = html.get(at + needle.len()).copied();
        if next.is_none_or(|b| is_space(b) || matches!(b, b'/' | b'>')) {
            return at;
        }
        i = at + 1;
    }
    html.len()
}

/// Where the renderer's head goes: just after the doctype when the document
/// starts with one (after a byte-order mark, whitespace, comments and `<?...>`
/// declarations), else at the very start (after a UTF-8 byte-order mark).
/// Never inside a comment, and never after a script that could run before
/// the policy exists. `None` when there is no such place: a doctype that is
/// never closed would put the policy after the whole page, so the caller
/// refuses the page instead (fail closed).
pub fn insertion_point(html: &[u8]) -> Option<usize> {
    let start = if html.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    let mut i = start;
    loop {
        while i < html.len() && is_space(html[i]) {
            i += 1;
        }
        let rest = &html[i..];
        if rest.starts_with(b"<!--") {
            i = comment_end(html, i);
        } else if rest.starts_with(b"<?") {
            i = find(html, b">", i).map_or(html.len(), |e| e + 1);
        } else if starts_with_ci(rest, b"<!doctype") {
            return find(html, b">", i).map(|e| e + 1);
        } else {
            return Some(start);
        }
    }
}

/// The `href` of the page's own first real `<base>` element (one with an
/// `href`), decoded. A `<base>` inside a comment or a script is not one.
pub fn own_base_href(html: &[u8]) -> Option<String> {
    tags(html)
        .into_iter()
        .find(|t| !t.is_end && t.name == "base" && t.attr("href").is_some())
        .and_then(|t| t.attr("href").map(str::to_string))
}

/// Decode HTML character references in an attribute value: numeric ones
/// (`&#47;`, `&#x2F;`, with or without `;`) and the named ones that spell
/// URL punctuation (`&sol;`, `&bsol;`, `&colon;`, `&Tab;`, `&NewLine;`, ...),
/// plus the usual `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`. Anything else
/// is left as written. Pure.
pub fn decode_char_refs(s: &str) -> String {
    const NAMED: &[(&str, char)] = &[
        ("amp", '&'),
        ("lt", '<'),
        ("gt", '>'),
        ("quot", '"'),
        ("apos", '\''),
        ("sol", '/'),
        ("bsol", '\\'),
        ("colon", ':'),
        ("period", '.'),
        ("percnt", '%'),
        ("Tab", '\t'),
        ("NewLine", '\n'),
        ("lpar", '('),
        ("rpar", ')'),
        ("semi", ';'),
        ("equals", '='),
        ("num", '#'),
        ("quest", '?'),
        ("commat", '@'),
        ("nbsp", '\u{a0}'),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(num) = after.strip_prefix('#') {
            let (hex, digits_from) = match num.as_bytes().first() {
                Some(b'x' | b'X') => (true, 1),
                _ => (false, 0),
            };
            let digits: String = num[digits_from..]
                .chars()
                .take_while(|c| {
                    if hex {
                        c.is_ascii_hexdigit()
                    } else {
                        c.is_ascii_digit()
                    }
                })
                .collect();
            if !digits.is_empty() {
                let value = u32::from_str_radix(&digits, if hex { 16 } else { 10 }).ok();
                let c = value
                    .and_then(char::from_u32)
                    .filter(|&c| c != '\0')
                    .unwrap_or('\u{fffd}');
                out.push(c);
                let mut used = 1 + digits_from + digits.len();
                if after[used..].starts_with(';') {
                    used += 1;
                }
                rest = &after[used..];
                continue;
            }
        }
        if let Some((name, c)) = NAMED
            .iter()
            .find(|(name, _)| after.starts_with(name) && after[name.len()..].starts_with(';'))
        {
            out.push(*c);
            rest = &after[name.len() + 1..];
            continue;
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Decode CSS escapes (`\2f`, `\/`, an escaped newline). Pure.
pub fn decode_css_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 {
            match chars.peek() {
                Some(h) if h.is_ascii_hexdigit() => {
                    hex.push(*h);
                    chars.next();
                }
                _ => break,
            }
        }
        if hex.is_empty() {
            match chars.next() {
                Some('\n') | None => {}
                Some(other) => out.push(other),
            }
            continue;
        }
        if matches!(chars.peek(), Some(' ' | '\t' | '\n' | '\r' | '\x0c')) {
            chars.next();
        }
        let c = u32::from_str_radix(&hex, 16)
            .ok()
            .and_then(char::from_u32)
            .filter(|&c| c != '\0')
            .unwrap_or('\u{fffd}');
        out.push(c);
    }
    out
}

/// Characters after which a new reference may start inside a value.
fn is_ref_delimiter(c: char) -> bool {
    matches!(c, ' ' | ',' | ';' | '=' | '(' | '\'' | '"' | '\u{c}')
}

/// Whether `text` (an attribute value or CSS, already decoded) contains a
/// reference to a share at its start or after a delimiter (a space, `,`,
/// `;`, `=`, `(`, a quote): `srcset` candidates, `url(...)`, a refresh
/// `url=`, a CSS string. Pure.
pub fn contains_share_ref(text: &str) -> bool {
    // URL parsers drop tabs and newlines anywhere, so they are no
    // separators (`/<TAB>/host` is `//host`).
    let text: String = text
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let mut starts = vec![0];
    starts.extend(
        text.char_indices()
            .filter(|(_, c)| is_ref_delimiter(*c))
            .map(|(i, c)| i + c.len_utf8()),
    );
    starts.into_iter().any(|start| {
        let candidate = text[start..].trim_start_matches(|c: char| c == ' ' || c.is_control());
        let could_be = candidate.starts_with(['/', '\\', '%'])
            || candidate
                .get(..5)
                .is_some_and(|h| h.eq_ignore_ascii_case("file:"));
        if !could_be {
            return false;
        }
        // Only the start of a reference decides; a window keeps this linear.
        let end = candidate
            .char_indices()
            .nth(4096)
            .map_or(candidate.len(), |(i, _)| i);
        local_ref::is_share_ref(&candidate[..end])
    })
}

/// Whether CSS text refers to a share anywhere (`url()`, `@import`, a
/// string), escapes decoded. Pure.
pub fn css_has_share_ref(css: &str) -> bool {
    contains_share_ref(css) || contains_share_ref(&decode_css_escapes(css))
}

/// What [`neutralize_share_refs`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Neutralized {
    /// The page with every share reference removed.
    pub html: Vec<u8>,
    /// How many attribute values and `<style>` blocks were emptied.
    pub removed: usize,
}

/// `html` with every reference to a share made harmless (module docs): an
/// attribute value that names one becomes `about:blank` (a `style`
/// attribute becomes empty), and a `<style>` block that names one is emptied.
/// Used by the browser fallback, which loads the page from `file:` and would
/// otherwise open `\\host\share` for `//host/x.png`. Pure.
pub fn neutralize_share_refs(html: &[u8]) -> Neutralized {
    let mut cuts: Vec<(Range<usize>, &'static str)> = Vec::new();
    let mut i = 0;
    while let Some(lt) = find(html, b"<", i) {
        i = lt + 1;
        let Some(tag) = parse_tag(html, lt) else {
            continue;
        };
        for attr in &tag.attrs {
            let value = &attr.value;
            let share = if attr.name == "style" {
                css_has_share_ref(value)
            } else {
                contains_share_ref(value) || css_has_share_ref(value)
            };
            if share {
                let with = if attr.name == "style" {
                    ""
                } else {
                    "about:blank"
                };
                cuts.push((attr.raw.clone(), with));
            }
        }
        if !tag.is_end && tag.name == "style" {
            let end = raw_text_end(html, tag.span.end, "style");
            let css = String::from_utf8_lossy(&html[tag.span.end..end]);
            if css_has_share_ref(&css) {
                cuts.push((tag.span.end..end, ""));
            }
        }
    }
    // Overlapping cuts (a tag parsed inside another tag's value) merge.
    cuts.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let mut merged: Vec<(Range<usize>, &'static str)> = Vec::new();
    for (range, with) in cuts {
        match merged.last_mut() {
            Some((last, _)) if range.start < last.end.max(last.start + 1) => {
                last.end = last.end.max(range.end);
            }
            _ => merged.push((range, with)),
        }
    }
    let mut out = Vec::with_capacity(html.len());
    let mut at = 0;
    for (range, with) in &merged {
        out.extend_from_slice(&html[at..range.start]);
        out.extend_from_slice(with.as_bytes());
        at = range.end;
    }
    out.extend_from_slice(&html[at..]);
    Neutralized {
        html: out,
        removed: merged.len(),
    }
}

/// `css` with every `/* ... */` comment replaced by a space (an unclosed
/// comment runs to the end), the way the CSS tokenizer drops them: so
/// `@import/**/"x.css"` reads as `@import "x.css"`. Pure.
pub fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        out.push(' ');
        rest = match rest[at + 2..].find("*/") {
            Some(end) => &rest[at + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// The target of every `@import` in `css` (escapes already decoded), and
/// whether one of them could not be read (neither a string nor `url(...)`).
fn import_targets(css: &str) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut unreadable = false;
    let lower = css.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("@import").map(|i| from + i) {
        from = at + "@import".len();
        let rest = css[from..].trim_start();
        let target = if let Some(inner) = rest
            .get(..4)
            .filter(|h| h.eq_ignore_ascii_case("url("))
            .map(|_| rest[4..].trim_start())
        {
            match inner.chars().next() {
                Some(q @ ('"' | '\'')) => inner[1..].split(q).next(),
                _ => inner.split(')').next(),
            }
        } else {
            match rest.chars().next() {
                Some(q @ ('"' | '\'')) => rest[1..].split(q).next(),
                _ => None,
            }
        };
        match target {
            Some(t) => out.push(t.trim().to_string()),
            None => unreadable = true,
        }
    }
    (out, unreadable)
}

/// Whether `css` holds an `@import` whose target cannot be read, after
/// escapes and comments are taken out: the browser fallback refuses such a
/// stylesheet rather than load a file it did not check. Pure.
pub fn has_unreadable_import(css: &str) -> bool {
    import_targets(&strip_css_comments(&decode_css_escapes(css))).1
}

/// The text of every `<style>` element of `html`, found the way
/// [`neutralize_share_refs`] finds them (at every `<style`, a superset).
pub fn style_blocks(html: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(lt) = find(html, b"<", i) {
        i = lt + 1;
        if let Some(tag) = parse_tag(html, lt) {
            if !tag.is_end && tag.name == "style" {
                let end = raw_text_end(html, tag.span.end, "style");
                out.push(String::from_utf8_lossy(&html[tag.span.end..end]).into_owned());
            }
        }
    }
    out
}

/// The encoding a page declares in a `<meta charset>` or
/// `<meta http-equiv=content-type content="...; charset=...">` within its
/// first 1024 bytes (the HTML prescan), as written. Pure.
fn declared_charset(html: &[u8]) -> Option<String> {
    let head = &html[..html.len().min(1024)];
    tags(head).into_iter().find_map(|t| {
        if t.is_end || t.name != "meta" {
            return None;
        }
        if let Some(c) = t.attr("charset") {
            return Some(c.to_string());
        }
        let is_content_type = t
            .attr("http-equiv")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("content-type"));
        let content = t.attr("content").filter(|_| is_content_type)?;
        let lower = content.to_ascii_lowercase();
        let at = lower.find("charset")? + "charset".len();
        let value = content[at..].trim_start().strip_prefix('=')?.trim_start();
        let value = value.trim_start_matches(['"', '\'']);
        let end = value
            .find(|c: char| c == ';' || c == '"' || c == '\'' || c.is_whitespace())
            .unwrap_or(value.len());
        Some(value[..end].to_string())
    })
}

/// `html` decoded the way a browser picks its encoding (a byte-order mark,
/// else the page's `<meta>` declaration, else UTF-8 when the bytes are valid
/// UTF-8, else Windows-1252) and returned as UTF-8 **with** a byte-order
/// mark. The mark makes every browser read the staged page as UTF-8 whatever
/// it declares, so the bytes Pluryprint checks are the bytes the renderer
/// parses (a UTF-16 page used to reach the browser fallback unchecked).
/// Idempotent. `None` when the page declares an encoding that cannot be
/// decoded faithfully (an unknown label, or one the WHATWG standard maps to
/// the replacement decoder): the caller refuses the page. Pure.
pub fn decode_page(html: &[u8]) -> Option<Vec<u8>> {
    let (encoding, body) = match encoding_rs::Encoding::for_bom(html) {
        Some((enc, bom_len)) => (enc, &html[bom_len..]),
        None => {
            let enc = match declared_charset(html) {
                Some(label) => {
                    let enc = encoding_rs::Encoding::for_label(label.trim().as_bytes())?;
                    if enc == encoding_rs::REPLACEMENT {
                        return None;
                    }
                    // HTML reads a declared UTF-16 as UTF-8, and
                    // x-user-defined as Windows-1252.
                    if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
                        encoding_rs::UTF_8
                    } else if enc == encoding_rs::X_USER_DEFINED {
                        encoding_rs::WINDOWS_1252
                    } else {
                        enc
                    }
                }
                None if std::str::from_utf8(html).is_ok() => encoding_rs::UTF_8,
                None => encoding_rs::WINDOWS_1252,
            };
            (enc, html)
        }
    };
    let (text, _) = encoding.decode_without_bom_handling(body);
    let mut out = Vec::with_capacity(text.len() + 3);
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    out.extend_from_slice(text.as_bytes());
    Some(out)
}

/// The text of a local stylesheet as the browser decodes it when the page is
/// UTF-8: a byte-order mark, else a leading `@charset "...";`, else UTF-8.
/// `None` when that cannot be done faithfully: an unknown or replacement
/// `@charset` label, or NUL bytes without a byte-order mark (a UTF-16
/// stylesheet the browser might decode by another hint), so the browser
/// fallback refuses rather than load a stylesheet it did not read. Pure.
pub fn decode_stylesheet(css: &[u8]) -> Option<String> {
    if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(css) {
        return Some(
            enc.decode_without_bom_handling(&css[bom_len..])
                .0
                .into_owned(),
        );
    }
    if css.contains(&0) {
        return None;
    }
    let mut encoding = encoding_rs::UTF_8;
    if let Some(rest) = css.strip_prefix(b"@charset \"") {
        let end = find(rest, b"\";", 0)?;
        let enc = encoding_rs::Encoding::for_label(&rest[..end])?;
        if enc == encoding_rs::REPLACEMENT {
            return None;
        }
        if enc != encoding_rs::UTF_16LE && enc != encoding_rs::UTF_16BE {
            encoding = enc;
        }
    }
    Some(encoding.decode_without_bom_handling(css).0.into_owned())
}

/// The local stylesheets a page (or a stylesheet) may load: the `href` of
/// every `<link>` tag and every `@import` target, anywhere in the text, with
/// and without its CSS comments (a superset, like [`neutralize_share_refs`]).
/// Decoded, not yet resolved.
pub fn stylesheet_refs(text: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(lt) = find(text, b"<", i) {
        i = lt + 1;
        if let Some(tag) = parse_tag(text, lt) {
            let is_stylesheet = tag
                .attr("rel")
                .is_some_and(|rel| rel.to_ascii_lowercase().contains("stylesheet"))
                || tag.attr("href").is_some_and(|href| {
                    let path = href.split(['?', '#']).next().unwrap_or_default();
                    path.to_ascii_lowercase().ends_with(".css")
                });
            if tag.name == "link" && is_stylesheet {
                if let Some(href) = tag.attr("href") {
                    out.push(href.to_string());
                }
            }
        }
    }
    let css = decode_css_escapes(&String::from_utf8_lossy(text));
    for target in import_targets(&css)
        .0
        .into_iter()
        .chain(import_targets(&strip_css_comments(&css)).0)
    {
        if !out.contains(&target) {
            out.push(target);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfr_security_tags_skip_comments_declarations_and_raw_text() {
        let html = b"<!-- <head><base href='//x/'> --><!DOCTYPE html><?xml x?>\
            <script>var s = '<base href=/s/>'</script><title><base href=t></title>\
            <!--> <base href=abrupt> <HEAD><BASE HREF=\"real/\"><p>";
        let names: Vec<String> = tags(html).into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"head".into()), "{names:?}");
        assert_eq!(own_base_href(html), Some("abrupt".into()));
        let html =
            b"<!-- <base href='//x/'> --><script>'<base href=/s/>'</script><base href=\"real/\">";
        assert_eq!(own_base_href(html), Some("real/".into()));
        assert_eq!(
            own_base_href(b"<!-- <base href=x>"),
            None,
            "unclosed comment"
        );
        assert_eq!(own_base_href(b"<base target=_blank><p>"), None);
    }

    #[test]
    fn nfr_security_insertion_point_is_never_in_a_comment() {
        assert_eq!(insertion_point(b"<!-- <head> --><html><head>"), Some(0));
        let doc = b"\xEF\xBB\xBF <!-- c --> <!DOCTYPE html><!-- <head> --><head>";
        let at = insertion_point(doc).unwrap();
        assert_eq!(&doc[..at], b"\xEF\xBB\xBF <!-- c --> <!DOCTYPE html>");
        assert_eq!(insertion_point(b"\xEF\xBB\xBF<p>"), Some(3));
        assert_eq!(insertion_point(b"<script>x</script><html>"), Some(0));
        // A doctype that never closes leaves no safe place: fail closed.
        assert_eq!(insertion_point(b"<!DOCTYPE html never closed"), None);
    }

    /// Security finding: a UTF-16 page reached the browser fallback's share
    /// filter undecoded (every `<` followed by a NUL), so nothing was
    /// neutralised. Pages are decoded by their real encoding first and staged
    /// as UTF-8 with a byte-order mark, which the browser then has to obey.
    #[test]
    fn nfr_security_pages_are_decoded_by_their_real_encoding() {
        let utf16 = |text: &str, le: bool| {
            let mut out = if le {
                vec![0xFF, 0xFE]
            } else {
                vec![0xFE, 0xFF]
            };
            for unit in text.encode_utf16() {
                out.extend_from_slice(&if le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            out
        };
        for le in [true, false] {
            let page = decode_page(&utf16("<img src=\"//host/x.png\">", le)).unwrap();
            assert_eq!(&page[..3], b"\xEF\xBB\xBF");
            assert_eq!(&page[3..], b"<img src=\"//host/x.png\">");
            assert_eq!(neutralize_share_refs(&page).removed, 1);
        }
        // A declared single-byte encoding is honoured...
        let page = b"<meta charset=windows-1252><p>caf\xE9";
        let out = decode_page(page).unwrap();
        assert!(String::from_utf8(out.clone()).unwrap().ends_with("café"));
        // ... decoding is idempotent (the mark wins over the declaration) ...
        assert_eq!(decode_page(&out).unwrap(), out);
        // ... a page that declares nothing and is not UTF-8 is Windows-1252 ...
        assert!(String::from_utf8(decode_page(b"<p>\xE9").unwrap())
            .unwrap()
            .ends_with("é"));
        // ... and an encoding that cannot be decoded faithfully is refused.
        assert_eq!(decode_page(b"<meta charset=x-made-up><p>x"), None);
        assert_eq!(decode_page(b"<meta charset=iso-2022-kr><p>x"), None);
        assert_eq!(
            decode_page(b"<meta http-equiv=Content-Type content='text/html; charset=bogus'>"),
            None
        );
        // A declared UTF-16 is read as UTF-8, as browsers do.
        assert_eq!(
            decode_page(b"<meta charset=utf-16><p>x").unwrap(),
            b"\xEF\xBB\xBF<meta charset=utf-16><p>x".to_vec()
        );
    }

    /// A stylesheet is decoded the way the browser decodes it; one that
    /// cannot be read faithfully is refused (`None`).
    #[test]
    fn nfr_security_stylesheets_are_decoded_or_refused() {
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "p{background:url(//host/x.png)}".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let css = decode_stylesheet(&utf16).unwrap();
        assert!(css_has_share_ref(&css), "{css}");
        assert_eq!(decode_stylesheet(&utf16[2..]), None, "UTF-16 without a BOM");
        assert_eq!(decode_stylesheet(b"@charset \"nope\"; p{}"), None);
        assert_eq!(
            decode_stylesheet(b"@charset \"windows-1252\"; p:after{content:'\xE9'}").unwrap(),
            "@charset \"windows-1252\"; p:after{content:'é'}"
        );
        assert_eq!(decode_stylesheet(b"p{}").unwrap(), "p{}");
    }

    /// Security finding: `@import/**/"x.css"` is a valid import the scanner
    /// did not see. Comments are stripped before looking for targets, and an
    /// `@import` whose target still cannot be read is reported.
    #[test]
    fn nfr_security_comment_obfuscated_imports_are_found() {
        let refs = stylesheet_refs(b"@import/**/\"evil.css\"; @import /* x */ url(b.css);");
        assert!(refs.contains(&"evil.css".to_string()), "{refs:?}");
        assert!(refs.contains(&"b.css".to_string()), "{refs:?}");
        assert!(!has_unreadable_import(
            "@import/**/'a.css'; @import url(b.css);"
        ));
        assert!(has_unreadable_import("@import foo;"));
        assert!(has_unreadable_import("@imp\\6frt bar;"));
        assert_eq!(strip_css_comments("a/*x*/b/*y"), "a b ");
        assert_eq!(
            style_blocks(b"<p><style>@import x;</style><STYLE media=x>b</STYLE>"),
            vec!["@import x;", "b"]
        );
    }

    #[test]
    fn nfr_security_char_and_css_escapes_decode() {
        assert_eq!(decode_char_refs("&#47;&#x2F;&sol;&bsol;host"), "///\\host");
        assert_eq!(decode_char_refs("&#47&#47host"), "//host");
        assert_eq!(
            decode_char_refs("a &amp; b &unknown; c"),
            "a & b &unknown; c"
        );
        assert_eq!(decode_css_escapes(r"\2f\2F host\/x"), "//host/x");
        assert_eq!(decode_css_escapes(r"\5c \5c host"), r"\\host");
    }

    #[test]
    fn nfr_security_browser_fallback_share_refs_neutralized() {
        for page in [
            r#"<img src="//host/x.png">"#,
            r#"<img src='\\host\x.png'>"#,
            r#"<img src=/\host/x.png>"#,
            r#"<img src="&#47;&#47;host/x.png">"#,
            r#"<img src="&sol;&bsol;host/x.png">"#,
            r#"<img src="file://host/x.png">"#,
            r#"<img src="file:////host/x.png">"#,
            r#"<img src="file:///%5C%5Chost/x.png">"#,
            "<img src=\"/\t/host/x.png\">",
            r#"<img srcset="a.png 1x, //host/b.png 2x">"#,
            r#"<link rel=stylesheet href="\\?\UNC\host\s.css">"#,
            r#"<div style="background:url(//host/x.png)">"#,
            r#"<div style="background:url(\2f\2fhost/x.png)">"#,
            r#"<svg><image href="\/host/x.png"/></svg>"#,
            r#"<meta http-equiv=refresh content="0; url=//host/x">"#,
            r#"<!-- <a title=" --><img src=//host/x.png>"#,
        ] {
            let out = neutralize_share_refs(page.as_bytes());
            let text = String::from_utf8(out.html).unwrap();
            assert!(out.removed >= 1, "{page} -> {text}");
            assert!(
                !contains_share_ref(&decode_char_refs(&text)),
                "{page} -> {text}"
            );
        }
        let style = "<style>@font-face{src:url( \"//host/f.woff\" )} p{color:red}</style><p>x";
        let out = neutralize_share_refs(style.as_bytes());
        assert_eq!(String::from_utf8(out.html).unwrap(), "<style></style><p>x");
        let style = r"<style>@import '\5c\5c host\s.css';</style>";
        assert_eq!(neutralize_share_refs(style.as_bytes()).removed, 1);

        // Local references, links to the web and ordinary text stay.
        let fine = "<img src=\"img/a.png\" srcset=\"a.png 1x, b.png 2x\"><a href=\"https://example.org/a//b\">\
                    x // y</a><p style=\"margin:0\">a/b</p><img src=\"file:///C:/x.png\">";
        let out = neutralize_share_refs(fine.as_bytes());
        assert_eq!(out.removed, 0);
        assert_eq!(out.html, fine.as_bytes());
    }

    #[test]
    fn stylesheet_refs_are_found_in_links_and_imports() {
        let refs = stylesheet_refs(
            b"<link rel=stylesheet href=\"a.css\"><style>@import url(b.css); @import 'c.css';</style>",
        );
        assert_eq!(refs, vec!["a.css", "b.css", "c.css"]);
    }
}
