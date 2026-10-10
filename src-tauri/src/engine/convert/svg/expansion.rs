//! How much work usvg would do to build a drawing, measured on the very
//! document it is handed, with a hard budget (security notes 4 and 8).
//!
//! The source-size, entity and node caps bound the parsed document, but usvg
//! then multiplies it:
//!
//! - every `<use>` copies its target (nested `<use>` grows exponentially);
//! - every vertex of a shape with a marker gets its own copy of the marker
//!   (markers that hold marked shapes multiply again);
//! - a pattern is cloned for every shape it paints;
//! - a `<mask>`, `<clipPath>` or `<filter>` is paid for again for **every**
//!   element that references it: usvg converts the `objectBoundingBox` ones
//!   per use, and even the `userSpaceOnUse` ones it shares are written out by
//!   svg2pdf once per use, so masks or clip paths whose content is masked by
//!   the previous one grow tenfold per level: a 3.7 KB drawing builds a
//!   million masks, and one with default-unit clip paths writes a 78 MB PDF
//!   (round 7);
//! - a `<feImage>` that names an element renders a copy of it.
//!
//! [`check_expansion`] walks that expansion and charges **every step it
//! takes** to one budget: each node with its attributes (and the length of
//! their values, so re-reading a long `d` or `style` is paid for), each
//! property declaration read, each reference considered (also those skipped
//! because they would recurse), each style-sheet rule indexed. When the budget
//! runs out it stops with `svg_too_large`, so its own cost is bounded by the
//! budget, whatever the drawing.
//!
//! It over-estimates on purpose: a style-sheet rule is applied to every
//! element its last simple selector could match (`g .a` to every `.a`,
//! anything it cannot read to every element), every vertex is assumed to
//! carry a marker, patterns are never shared, and every mask, clip path or
//! filter is charged in full for every reference, whatever its units. Its
//! subtree is walked once; later references charge what that walk cost
//! without walking it again, so the walk itself stays cheap.
//!
//! This is defence in depth: the boundary is the worker's memory cap and the
//! conversion timeout ([`super::convert_with_worker`]). It exists so that an
//! oversized drawing fails fast with the right reason instead of exhausting
//! the worker.

use std::collections::{HashMap, HashSet};

use usvg::roxmltree::{Document, Node, NodeId};

use super::too_large;
use crate::engine::EngineError;

/// Most work the drawing may expand to, across the drawing and every SVG it
/// embeds as an `<image>` (see the module docs for what one unit is).
pub(crate) const MAX_EXPANDED_WEIGHT: u64 = 4_000_000;

/// Most `<image>` elements the drawing may expand to: each one is read and
/// decoded again for every copy.
pub(crate) const MAX_EXPANDED_IMAGES: u64 = 1_000;

/// Deepest nesting, `<use>` and marker/pattern/mask instances included. usvg
/// refuses past 1024 levels itself; checking first keeps this walk bounded.
const MAX_EXPANDED_DEPTH: usize = 1024;

/// Bytes of attribute values (or style sheet) that cost one unit of work.
const BYTES_PER_UNIT: u64 = 64;

/// The work `doc` expands to, at most `budget` units; `svg_too_large` past it.
pub(crate) fn check_expansion(doc: &Document, budget: u64) -> Result<u64, EngineError> {
    let mut walk = Expansion {
        ids: HashMap::new(),
        css: CssIndex::default(),
        weight: 0,
        budget,
        images: 0,
        effect_cost: HashMap::new(),
    };
    for node in doc.descendants() {
        walk.add(1)?;
        if let Some(id) = node.attribute("id") {
            walk.ids.entry(id).or_insert(node);
        }
    }
    for node in doc.descendants().filter(|n| n.has_tag_name("style")) {
        for text in node.children().filter_map(|c| c.text()) {
            walk.index_style_sheet(text)?;
        }
    }
    walk.visit(doc.root(), &Inherited::default(), &mut HashSet::new())?;
    Ok(walk.weight)
}

/// The `#id` targets of every `url(#id)` in `text`.
fn url_refs(text: &str) -> impl Iterator<Item = &str> {
    text.split("url(").skip(1).filter_map(|rest| {
        let inner = rest.split(')').next()?.trim().trim_matches(['"', '\'']);
        inner.trim().strip_prefix('#').map(str::trim)
    })
}

/// The properties whose `url(#id)` values make usvg build something per use,
/// in their canonical spelling.
const EXPANDING: [&str; 9] = [
    "fill",
    "stroke",
    "marker",
    "marker-start",
    "marker-mid",
    "marker-end",
    "mask",
    "clip-path",
    "filter",
];

/// `name` as one of [`EXPANDING`], if it is one. Pure.
fn expanding(name: &str) -> Option<&'static str> {
    let name = name.trim();
    EXPANDING.into_iter().find(|p| *p == name)
}

/// The paint servers and markers a node inherits.
#[derive(Clone, Default)]
struct Inherited<'a, 'input> {
    fill: Option<Node<'a, 'input>>,
    stroke: Option<Node<'a, 'input>>,
    markers: [Option<Node<'a, 'input>>; 3],
}

/// Which elements a style-sheet rule may apply to, from its last simple
/// selector: always a superset of the elements it really matches.
#[derive(Debug, PartialEq, Eq)]
enum Selects<'a> {
    Every,
    Tag(&'a str),
    Id(&'a str),
    Class(&'a str),
}

/// What [`Selects`] a selector reads as. Pure.
fn selects(selector: &str) -> Selects<'_> {
    let last = selector
        .trim()
        .rsplit(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~' | '{' | '}'))
        .next()
        .unwrap_or("");
    let compound = last.split([':', '[']).next().unwrap_or("");
    let ident = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    if let Some((_, id)) = compound.split_once('#') {
        let id = id.split('.').next().unwrap_or("");
        if ident(id) {
            return Selects::Id(id);
        }
    } else if let Some((_, class)) = compound.split_once('.') {
        let class = class.split('.').next().unwrap_or("");
        if ident(class) {
            return Selects::Class(class);
        }
    } else if ident(compound) {
        return Selects::Tag(compound);
    }
    Selects::Every
}

/// `text` without its `/* ... */` comments (an unclosed one runs to the end,
/// as in CSS). Pure.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let body = &rest[start + 2..];
        rest = body.find("*/").map_or("", |end| &body[end + 2..]);
    }
    out.push_str(rest);
    out
}

/// One expanding declaration: the property and the elements its `url()`s
/// name.
type Declaration<'a, 'input> = (&'static str, Vec<Node<'a, 'input>>);

/// The expanding declarations of the drawing's style sheets, by what they may
/// apply to.
#[derive(Default)]
struct CssIndex<'a, 'input> {
    every: Vec<Declaration<'a, 'input>>,
    tag: HashMap<String, Vec<Declaration<'a, 'input>>>,
    id: HashMap<String, Vec<Declaration<'a, 'input>>>,
    class: HashMap<String, Vec<Declaration<'a, 'input>>>,
}

/// The state of [`check_expansion`]'s walk.
struct Expansion<'a, 'input> {
    ids: HashMap<&'a str, Node<'a, 'input>>,
    css: CssIndex<'a, 'input>,
    weight: u64,
    budget: u64,
    images: u64,
    /// What walking each mask, clip path or filter once cost, so a later
    /// reference charges the same again without re-walking it (round 7:
    /// every reference is paid for, shared units or not).
    effect_cost: HashMap<NodeId, u64>,
}

impl<'a, 'input> Expansion<'a, 'input> {
    fn add(&mut self, weight: u64) -> Result<(), EngineError> {
        self.weight = self.weight.saturating_add(weight);
        if self.weight > self.budget {
            return Err(too_large("the SVG expands to too many elements"));
        }
        Ok(())
    }

    /// The nodes the `url(#id)`s of a property value name.
    fn urls(&self, value: &str) -> Vec<Node<'a, 'input>> {
        url_refs(value)
            .filter_map(|id| self.ids.get(id).copied())
            .collect()
    }

    /// Index the expanding declarations of one `<style>` text: rules are
    /// `selectors { declarations }`; a nested block (`@media x { .a { ... }
    /// }`) leaves `@media x { .a` as the selector text, whose last simple
    /// selector is still `.a`. Every rule, declaration, selector and
    /// reference read is charged.
    fn index_style_sheet(&mut self, text: &str) -> Result<(), EngineError> {
        self.add(text.len() as u64 / BYTES_PER_UNIT)?;
        let text = strip_comments(text);
        for chunk in text.split('}') {
            self.add(1)?;
            let Some((selectors, body)) = chunk.rsplit_once('{') else {
                continue;
            };
            for decl in body.split(';') {
                self.add(1)?;
                let Some((name, value)) = decl.split_once(':') else {
                    continue;
                };
                let Some(name) = expanding(name) else {
                    continue;
                };
                let targets = self.urls(value);
                if targets.is_empty() {
                    continue;
                }
                for selector in selectors.split(',') {
                    self.add(1 + targets.len() as u64)?;
                    let entry = (name, targets.clone());
                    match selects(selector) {
                        Selects::Every => self.css.every.push(entry),
                        Selects::Tag(t) => self.css.tag.entry(t.into()).or_default().push(entry),
                        Selects::Id(i) => self.css.id.entry(i.into()).or_default().push(entry),
                        Selects::Class(c) => {
                            self.css.class.entry(c.into()).or_default().push(entry)
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The node an `href` / `xlink:href` (`#id`) names, as usvg finds it.
    fn href(&self, node: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
        let value = node
            .attribute(("http://www.w3.org/1999/xlink", "href"))
            .or_else(|| node.attribute("href"))?;
        self.ids
            .get(value.trim().strip_prefix('#')?.trim())
            .copied()
    }

    /// `node`'s expanding declarations, in cascade order: presentation
    /// attributes, style-sheet rules that may match it, its `style`
    /// attribute. Every declaration and reference read is charged.
    fn declarations(
        &mut self,
        node: Node<'a, 'input>,
    ) -> Result<Vec<Declaration<'a, 'input>>, EngineError> {
        let mut out = Vec::new();
        for attr in node.attributes() {
            if let Some(name) = expanding(attr.name()) {
                out.push((name, self.urls(attr.value())));
            }
        }
        let mut matched = 0u64;
        {
            let mut sheets: Vec<&Vec<Declaration>> = vec![&self.css.every];
            if let Some(decls) = self.css.tag.get(node.tag_name().name()) {
                sheets.push(decls);
            }
            if let Some(decls) = node.attribute("id").and_then(|id| self.css.id.get(id)) {
                sheets.push(decls);
            }
            if let Some(classes) = node.attribute("class") {
                for class in classes.split_whitespace() {
                    matched += 1;
                    if let Some(decls) = self.css.class.get(class) {
                        sheets.push(decls);
                    }
                }
            }
            for decls in sheets {
                for (name, targets) in decls {
                    matched += 1 + targets.len() as u64;
                    if matched > self.budget {
                        break;
                    }
                    out.push((*name, targets.clone()));
                }
            }
        }
        self.add(matched)?;
        if let Some(style) = node.attribute("style") {
            for decl in style.split(';') {
                self.add(1)?;
                if let Some((name, value)) = decl.split_once(':') {
                    if let Some(name) = expanding(name) {
                        out.push((name, self.urls(value)));
                    }
                }
            }
        }
        Ok(out)
    }

    fn visit(
        &mut self,
        node: Node<'a, 'input>,
        ctx: &Inherited<'a, 'input>,
        chain: &mut HashSet<NodeId>,
    ) -> Result<(), EngineError> {
        if !node.is_element() && !node.is_root() {
            return self.add(1);
        }
        if chain.len() >= MAX_EXPANDED_DEPTH {
            return Err(too_large("the SVG nests too deeply"));
        }
        self.add(node_weight(node))?;
        let mut ctx = ctx.clone();
        let mut effects = Vec::new();
        for (name, targets) in self.declarations(node)? {
            self.add(targets.len() as u64)?;
            let first = targets.first().copied();
            match name {
                "fill" => ctx.fill = first,
                "stroke" => ctx.stroke = first,
                "marker-start" => ctx.markers[0] = first,
                "marker-mid" => ctx.markers[1] = first,
                "marker-end" => ctx.markers[2] = first,
                "marker" => ctx.markers = [first; 3],
                // `mask`, `clip-path`, `filter` (which may list several).
                _ => effects.extend(targets),
            }
        }
        let name = node.tag_name().name();
        chain.insert(node.id());
        let fresh = Inherited::default();

        for target in effects {
            self.add(1)?;
            let kind = target.tag_name().name();
            if !matches!(kind, "mask" | "clipPath" | "filter") || chain.contains(&target.id()) {
                continue;
            }
            if let Some(&cost) = self.effect_cost.get(&target.id()) {
                self.add(cost)?;
                continue;
            }
            let before = self.weight;
            self.visit(target, &fresh, chain)?;
            self.effect_cost.insert(target.id(), self.weight - before);
        }
        if matches!(name, "use" | "feImage") {
            if let Some(target) = self.href(node) {
                self.add(1)?;
                if !chain.contains(&target.id()) {
                    let inherited = if name == "use" { &ctx } else { &fresh };
                    self.visit(target, inherited, chain)?;
                }
            }
        }
        if name == "image" {
            self.images += 1;
            if self.images > MAX_EXPANDED_IMAGES {
                return Err(too_large("the SVG expands to too many images"));
            }
        }
        if matches!(
            name,
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "text"
        ) {
            for pattern in [ctx.fill, ctx.stroke].into_iter().flatten() {
                let mut content = Some(pattern);
                // A pattern with no content of its own uses the one it links.
                while let Some(p) = content {
                    self.add(1)?;
                    if !p.has_tag_name("pattern") || chain.contains(&p.id()) {
                        break;
                    }
                    self.visit(p, &fresh, chain)?;
                    if p.children().any(|c| c.is_element()) {
                        break;
                    }
                    content = self.href(p).filter(|t| t != &p);
                }
            }
            if matches!(name, "path" | "line" | "polyline" | "polygon") {
                let vertices = vertex_bound(node);
                for marker in ctx.markers.into_iter().flatten() {
                    self.add(1)?;
                    if !marker.has_tag_name("marker") || chain.contains(&marker.id()) {
                        continue;
                    }
                    let before = self.weight;
                    self.visit(marker, &fresh, chain)?;
                    let once = self.weight - before;
                    self.add(once.saturating_mul(vertices.saturating_sub(1)))?;
                }
            }
        }
        for child in node.children() {
            self.visit(child, &ctx, chain)?;
        }
        chain.remove(&node.id());
        Ok(())
    }
}

/// The weight of one node: itself, its attributes and the bytes of their
/// values (so every re-read of a long `d`, `points` or `style` is paid for).
/// Pure.
fn node_weight(node: Node) -> u64 {
    let bytes: u64 = node.attributes().map(|a| a.value().len() as u64).sum();
    1 + node.attributes().len() as u64 + bytes / BYTES_PER_UNIT
}

/// An upper bound of the vertices of a shape that can carry markers: every
/// run of digits and every command letter of its `d` or `points` could start
/// one. Pure.
fn vertex_bound(node: Node) -> u64 {
    let data = node
        .attribute("d")
        .or_else(|| node.attribute("points"))
        .unwrap_or("");
    let mut count = 2u64;
    let mut in_digits = false;
    for c in data.bytes() {
        let digit = c.is_ascii_digit();
        if (digit && !in_digits) || (c.is_ascii_alphabetic() && c != b'e' && c != b'E') {
            count += 1;
        }
        in_digits = digit;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_keys_are_supersets() {
        assert_eq!(selects(".cls-1"), Selects::Class("cls-1"));
        assert_eq!(selects("g .a"), Selects::Class("a"));
        assert_eq!(selects("svg > rect.b"), Selects::Class("b"));
        assert_eq!(selects("#x"), Selects::Id("x"));
        assert_eq!(selects("path"), Selects::Tag("path"));
        assert_eq!(selects("rect:first-child"), Selects::Tag("rect"));
        assert_eq!(selects("@media print { .a"), Selects::Class("a"));
        assert_eq!(selects("*"), Selects::Every);
        assert_eq!(selects("[fill]"), Selects::Every);
        assert_eq!(selects(""), Selects::Every);
        assert_eq!(strip_comments("a/* x */b/* open"), "ab");
    }
}
