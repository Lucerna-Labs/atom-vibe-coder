//! Reduced HTML/CSS front-end. Parses an HTML subset with inline `style` attributes into
//! the shared UXI box tree (`pmre_kit::ux`), which then flows through the same layout +
//! raster path as native UXI. This is the "reduce": the load-bearing core is the box
//! model, block/flex layout, inline text flow, and a CSS property subset; selectors,
//! external stylesheets, and the full cascade are the expansion, not the foundation.
//!
//! Supported structure: comments, doctype, `<script>`/`<style>` skipping, entities,
//! block elements (`div p h1-h4 ul ol li hr section header footer main nav article`),
//! and inline elements (`b strong i em u a span small code mark`) that coalesce into a
//! single word-wrapping rich flow — `<b>bold</b> and plain` stays on one line.
//!
//! Supported CSS (inline `style="..."`): display(flex|block|none), flex-direction, flex,
//! flex-grow, width/height (px/%/auto), padding/margin (+ per-side, 1-4 value shorthand),
//! gap, background(-color), border, border-radius, box-shadow, color, font-size,
//! font-weight, text-align, text-decoration, align-items, justify-content, opacity.
//! Colors: #rgb/#rrggbb/#rrggbbaa, rgb()/rgba(), hsl(), ~40 named colors.

use pmre_kit::paint::Rgba;
use pmre_kit::ux::{Align, Dim, Dir, Edges, Justify, Shadow, Span, Style, UxNode};

// ─── DOM ─────────────────────────────────────────────────────────────────────

enum Dom {
    Elem {
        tag: String,
        style_attr: Option<String>,
        kids: Vec<Dom>,
    },
    Text(String),
}

enum Tok {
    Open {
        tag: String,
        style_attr: Option<String>,
        self_close: bool,
    },
    Close(String),
    Text(String),
}

/// Inherited text properties (a minimal stand-in for CSS inheritance).
#[derive(Clone, Copy)]
struct Inherited {
    color: Rgba,
    font_size: f32,
    bold: bool,
    underline: bool,
    text_align: Align,
    opacity: f32,
}

impl Default for Inherited {
    fn default() -> Self {
        Inherited {
            color: Rgba::rgb8(228, 232, 240),
            font_size: 14.0,
            bold: false,
            underline: false,
            text_align: Align::Start,
            opacity: 1.0,
        }
    }
}

/// Parse an HTML document fragment into a single UXI root node.
pub fn parse(src: &str) -> UxNode {
    let toks = tokenize(src);
    let mut pos = 0usize;
    let roots = parse_nodes(&toks, &mut pos, None, 0);
    let kids = children_to_ux(&roots, Inherited::default(), None, false);
    if kids.len() == 1 {
        kids.into_iter().next().unwrap()
    } else {
        UxNode::Box {
            style: Style::col(),
            children: kids,
        }
    }
}

fn is_void(tag: &str) -> bool {
    matches!(tag, "br" | "img" | "hr" | "input" | "meta" | "link")
}

fn is_inline(tag: &str) -> bool {
    matches!(
        tag,
        "b" | "strong" | "i" | "em" | "u" | "a" | "span" | "small" | "code" | "mark" | "br"
    )
}

fn is_dropped(tag: &str) -> bool {
    matches!(tag, "img" | "input" | "meta" | "link" | "head" | "title")
}

// ─── Tokenizer ───────────────────────────────────────────────────────────────

/// Collapse whitespace runs to single spaces, **preserving** boundary spaces so inline
/// runs keep their word gaps (`<b>bold</b> text` needs the space before "text").
fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

/// Decode the common HTML entities in already-collapsed text.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let b: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == '&' {
            // an entity name is short — bound the scan so '&' runs stay O(n)
            let window = &b[i + 1..(i + 33).min(b.len())];
            if let Some(semi) = window.iter().position(|&c| c == ';') {
                let name: String = b[i + 1..i + 1 + semi].iter().collect();
                let decoded = match name.as_str() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    "middot" => Some('·'),
                    "bull" => Some('•'),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "hellip" => Some('…'),
                    "copy" => Some('©'),
                    "times" => Some('×'),
                    "eacute" => Some('é'),
                    "egrave" => Some('è'),
                    "agrave" => Some('à'),
                    "uuml" => Some('ü'),
                    "ouml" => Some('ö'),
                    "auml" => Some('ä'),
                    "deg" => Some('°'),
                    "rarr" => Some('→'),
                    "larr" => Some('←'),
                    _ => {
                        if let Some(num) = name.strip_prefix("#x").or(name.strip_prefix("#X")) {
                            u32::from_str_radix(num, 16).ok().and_then(char::from_u32)
                        } else if let Some(num) = name.strip_prefix('#') {
                            num.parse::<u32>().ok().and_then(char::from_u32)
                        } else {
                            None
                        }
                    }
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i += semi + 2;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn tokenize(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < b.len() {
        if b[i] == b'<' {
            // comments and doctype
            if src[i..].starts_with("<!--") {
                i = match src[i + 4..].find("-->") {
                    Some(end) => i + 4 + end + 3,
                    None => b.len(),
                };
                continue;
            }
            if src[i..].starts_with("<!") || src[i..].starts_with("<?") {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'>' {
                    j += 1;
                }
                i = j + 1;
                continue;
            }
            let mut j = i + 1;
            let mut quote: u8 = 0;
            while j < b.len() {
                let c = b[j];
                if quote != 0 {
                    if c == quote {
                        quote = 0;
                    }
                } else if c == b'"' || c == b'\'' {
                    quote = c;
                } else if c == b'>' {
                    break;
                }
                j += 1;
            }
            let inner = src[i + 1..j.min(b.len())].trim();
            if let Some(rest) = inner.strip_prefix('/') {
                out.push(Tok::Close(rest.trim().to_ascii_lowercase()));
            } else {
                let self_close = inner.ends_with('/');
                let inner = inner.trim_end_matches('/').trim();
                let (tag, style_attr) = parse_open(inner);
                // raw-content elements: skip everything until the matching close tag,
                // scanning in place (no copy/lowercase of the whole remainder)
                if tag == "script" || tag == "style" {
                    let close = format!("</{tag}");
                    i = match find_ascii_ci(b, j + 1, close.as_bytes()) {
                        Some(after) => match src[after..].find('>') {
                            Some(g) => after + g + 1,
                            None => b.len(),
                        },
                        None => b.len(),
                    };
                    continue;
                }
                let self_close = self_close || is_void(&tag);
                out.push(Tok::Open {
                    tag,
                    style_attr,
                    self_close,
                });
            }
            i = j + 1;
        } else {
            let start = i;
            while i < b.len() && b[i] != b'<' {
                i += 1;
            }
            let text = decode_entities(&collapse_ws(&src[start..i]));
            if !text.is_empty() {
                out.push(Tok::Text(text));
            }
        }
    }
    out
}

/// Case-insensitive ASCII substring search over bytes starting at `from`;
/// returns the index of the first match.
fn find_ascii_ci(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len())
        .find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// Pull the tag name and the `style="..."` value out of an opening-tag body,
/// scanning attributes properly (quoted values may contain spaces and `=`).
fn parse_open(inner: &str) -> (String, Option<String>) {
    let mut it = inner.splitn(2, char::is_whitespace);
    let tag = it.next().unwrap_or("").to_ascii_lowercase();
    let attrs = it.next().unwrap_or("");
    (tag, find_attr(attrs, "style"))
}

fn find_attr(attrs: &str, want: &str) -> Option<String> {
    let b: Vec<char> = attrs.chars().collect();
    let mut i = 0usize;
    while i < b.len() {
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        // attribute name
        let name_start = i;
        while i < b.len() && b[i] != '=' && !b[i].is_whitespace() {
            i += 1;
        }
        let name: String = b[name_start..i]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == '=' {
            i += 1;
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == '"' || b[i] == '\'') {
                let q = b[i];
                i += 1;
                while i < b.len() && b[i] != q {
                    value.push(b[i]);
                    i += 1;
                }
                i += 1; // closing quote
            } else {
                while i < b.len() && !b[i].is_whitespace() {
                    value.push(b[i]);
                    i += 1;
                }
            }
        }
        if name == want {
            return Some(value);
        }
        if name.is_empty() {
            i += 1; // guard against pathological input
        }
    }
    None
}

/// Recursion guard: DOM nesting past this depth is flattened (children parsed as
/// siblings) instead of overflowing the stack on adversarial input.
const MAX_DOM_DEPTH: usize = 192;

fn parse_nodes(toks: &[Tok], pos: &mut usize, stop: Option<&str>, depth: usize) -> Vec<Dom> {
    let mut nodes = Vec::new();
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Close(name) => {
                if Some(name.as_str()) == stop {
                    *pos += 1;
                    return nodes;
                }
                *pos += 1; // stray close: skip
            }
            Tok::Text(t) => {
                nodes.push(Dom::Text(t.clone()));
                *pos += 1;
            }
            Tok::Open {
                tag,
                style_attr,
                self_close,
            } => {
                let tag = tag.clone();
                let style_attr = style_attr.clone();
                let self_close = *self_close || depth >= MAX_DOM_DEPTH;
                *pos += 1;
                let kids = if self_close {
                    Vec::new()
                } else {
                    parse_nodes(toks, pos, Some(&tag), depth + 1)
                };
                nodes.push(Dom::Elem {
                    tag,
                    style_attr,
                    kids,
                });
            }
        }
    }
    nodes
}

// ─── DOM → UXI ───────────────────────────────────────────────────────────────

fn tag_font(tag: &str, base: f32) -> f32 {
    match tag {
        "h1" => 30.0,
        "h2" => 24.0,
        "h3" => 18.0,
        "h4" => 16.0,
        "small" => (base * 0.85).max(8.0),
        "code" => (base * 0.95).max(8.0),
        _ => base,
    }
}

fn tag_default_style(tag: &str) -> Style {
    let mut s = Style::col();
    match tag {
        "p" => s.margin = Edges::xy(0.0, 6.0),
        "h1" => s.margin = Edges::xy(0.0, 10.0),
        "h2" => s.margin = Edges::xy(0.0, 8.0),
        "h3" | "h4" => s.margin = Edges::xy(0.0, 6.0),
        "ul" | "ol" => {
            s.margin = Edges::xy(0.0, 6.0);
            s.padding = Edges {
                l: 8.0,
                t: 0.0,
                r: 0.0,
                b: 0.0,
            };
            s.gap = 4.0;
        }
        "li" => s.gap = 2.0,
        _ => {}
    }
    s
}

/// Convert a list of sibling DOM nodes, coalescing text and inline elements into
/// shared `Rich` flows so mixed-style words wrap on the same lines. Inside a
/// `display:flex` row (`flex_row`), CSS makes every child its own flex item instead,
/// so inline elements stay separate boxes and the container's `gap` applies.
fn children_to_ux(
    kids: &[Dom],
    inh: Inherited,
    li_prefix: Option<&str>,
    flex_row: bool,
) -> Vec<UxNode> {
    let mut out: Vec<UxNode> = Vec::new();
    let mut run: Vec<Span> = Vec::new();
    let mut first_flush = true;

    let flush = |run: &mut Vec<Span>, out: &mut Vec<UxNode>, first: &mut bool| {
        let has_content = run.iter().any(|s| !s.text.trim().is_empty());
        if has_content {
            let mut spans = std::mem::take(run);
            if *first {
                if let Some(prefix) = li_prefix {
                    let mut bullet = Span::new(prefix, inh.font_size, inh.color);
                    bullet.bold = false;
                    spans.insert(0, bullet);
                }
            }
            *first = false;
            out.push(UxNode::Rich {
                spans,
                align: inh.text_align,
            });
        } else {
            run.clear();
        }
    };

    for k in kids {
        match k {
            Dom::Text(t) => {
                run.push(make_span(t, inh));
                if flex_row {
                    flush(&mut run, &mut out, &mut first_flush);
                }
            }
            Dom::Elem { tag, .. } if tag == "br" => {
                flush(&mut run, &mut out, &mut first_flush);
            }
            Dom::Elem {
                tag,
                style_attr,
                kids: inner,
            } if is_inline(tag) => {
                inline_spans(tag, style_attr.as_deref(), inner, inh, &mut run);
                if flex_row {
                    flush(&mut run, &mut out, &mut first_flush);
                }
            }
            Dom::Elem {
                tag,
                style_attr,
                kids: inner,
            } => {
                flush(&mut run, &mut out, &mut first_flush);
                let prefix = (tag == "li").then(|| "• ".to_string());
                if let Some(node) = elem_to_ux(tag, style_attr.as_deref(), inner, inh, prefix) {
                    out.push(node);
                }
            }
        }
    }
    flush(&mut run, &mut out, &mut first_flush);
    out
}

fn make_span(text: &str, inh: Inherited) -> Span {
    Span {
        text: text.to_string(),
        size: inh.font_size,
        color: inh.color.with_alpha(inh.color.a * inh.opacity),
        bold: inh.bold,
        underline: inh.underline,
    }
}

/// Flatten an inline element (possibly nested) into styled spans appended to `run`.
fn inline_spans(
    tag: &str,
    style_attr: Option<&str>,
    kids: &[Dom],
    inh: Inherited,
    run: &mut Vec<Span>,
) {
    let mut inh = inh;
    inh.font_size = tag_font(tag, inh.font_size);
    match tag {
        "b" | "strong" => inh.bold = true,
        "u" => inh.underline = true,
        "a" => {
            inh.underline = true;
            inh.color = Rgba::rgb8(96, 165, 250); // blue-400 — link affordance
        }
        "code" | "mark" => {
            inh.color = Rgba::rgb8(251, 191, 96); // amber — stands in for a code face
        }
        _ => {}
    }
    if let Some(css) = style_attr {
        let mut scratch = Style::col();
        apply_css(&mut scratch, &mut inh, css);
    }
    for k in kids {
        match k {
            Dom::Text(t) => run.push(make_span(t, inh)),
            Dom::Elem {
                tag,
                style_attr,
                kids,
            } if is_inline(tag) && tag != "br" => {
                inline_spans(tag, style_attr.as_deref(), kids, inh, run)
            }
            _ => {} // block inside inline: out of subset, dropped
        }
    }
}

fn elem_to_ux(
    tag: &str,
    style_attr: Option<&str>,
    kids: &[Dom],
    inh: Inherited,
    li_prefix: Option<String>,
) -> Option<UxNode> {
    if is_dropped(tag) {
        return None;
    }
    if tag == "hr" {
        let mut s = Style::col().h(Dim::Px(1.0)).bg(Rgba::rgb8(63, 63, 70));
        s.margin = Edges::xy(0.0, 8.0);
        return Some(UxNode::Box {
            style: s,
            children: Vec::new(),
        });
    }
    let mut style = tag_default_style(tag);
    let mut inh2 = inh;
    inh2.font_size = tag_font(tag, inh.font_size);
    if matches!(tag, "h1" | "h2" | "h3" | "h4") {
        inh2.bold = true;
    }
    if let Some(css) = style_attr {
        if !apply_css(&mut style, &mut inh2, css) {
            return None; // display:none
        }
    }
    let flex_row = matches!(style.dir, Dir::Row);
    let children = children_to_ux(kids, inh2, li_prefix.as_deref(), flex_row);
    Some(UxNode::Box { style, children })
}

// ─── CSS ─────────────────────────────────────────────────────────────────────

/// Apply inline declarations to `style`/`inh`. Returns `false` for `display:none`.
fn apply_css(style: &mut Style, inh: &mut Inherited, css: &str) -> bool {
    let mut visible = true;
    for decl in css.split(';') {
        let mut kv = decl.splitn(2, ':');
        let key = kv.next().unwrap_or("").trim().to_ascii_lowercase();
        let val = match kv.next() {
            Some(v) => v.trim(),
            None => continue,
        };
        let lval = val.to_ascii_lowercase();
        match key.as_str() {
            "display" => match lval.as_str() {
                "none" => visible = false,
                "flex" => style.dir = Dir::Row,
                _ => style.dir = Dir::Column,
            },
            "flex-direction" => {
                style.dir = if lval.starts_with("row") {
                    Dir::Row
                } else {
                    Dir::Column
                };
            }
            "flex" | "flex-grow" => {
                if lval == "none" {
                    style.width = Dim::Auto;
                    style.height = Dim::Auto;
                } else if let Some(n) = val
                    .split_whitespace()
                    .next()
                    .and_then(parse_f32)
                    .or((lval == "auto").then_some(1.0))
                {
                    style.width = Dim::Flex(n);
                    style.height = Dim::Flex(n);
                }
                // any other non-numeric value: leave sizing untouched
            }
            "width" => style.width = parse_dim(val),
            "height" => style.height = parse_dim(val),
            "padding" => style.padding = parse_edges(val).unwrap_or(style.padding),
            "padding-left" => set_edge(&mut style.padding, val, 'l'),
            "padding-top" => set_edge(&mut style.padding, val, 't'),
            "padding-right" => set_edge(&mut style.padding, val, 'r'),
            "padding-bottom" => set_edge(&mut style.padding, val, 'b'),
            "margin" => style.margin = parse_edges(val).unwrap_or(style.margin),
            "margin-left" => set_edge(&mut style.margin, val, 'l'),
            "margin-top" => set_edge(&mut style.margin, val, 't'),
            "margin-right" => set_edge(&mut style.margin, val, 'r'),
            "margin-bottom" => set_edge(&mut style.margin, val, 'b'),
            "gap" => {
                if let Some(p) = parse_px(val) {
                    style.gap = p;
                }
            }
            "background" | "background-color" => {
                if let Some(c) = parse_color(val) {
                    style.background = Some(c);
                }
            }
            "border-radius" => {
                if let Some(p) = parse_px(val) {
                    style.radius = p;
                }
            }
            "border" => {
                if lval == "none" {
                    style.border = None;
                } else {
                    let parts: Vec<&str> = val.split_whitespace().collect();
                    if parts.len() >= 3 {
                        if let (Some(w), Some(c)) =
                            (parse_px(parts[0]), parse_color(&parts[2..].join(" ")))
                        {
                            style.border = Some((w, c));
                        }
                    }
                }
            }
            "box-shadow" => {
                if lval == "none" {
                    style.shadow = None;
                } else if let Some(sh) = parse_shadow(val) {
                    style.shadow = Some(sh);
                }
            }
            "color" => {
                if let Some(c) = parse_color(val) {
                    inh.color = c;
                }
            }
            "font-size" => {
                if let Some(p) = parse_px(val) {
                    inh.font_size = p;
                }
            }
            "font-weight" => {
                inh.bold = lval == "bold"
                    || lval == "bolder"
                    || lval.parse::<f32>().map(|n| n >= 600.0).unwrap_or(false);
            }
            "text-decoration" | "text-decoration-line" => {
                inh.underline = lval.contains("underline");
            }
            "text-align" => {
                inh.text_align = match lval.as_str() {
                    "center" => Align::Center,
                    "right" | "end" => Align::End,
                    _ => Align::Start,
                };
            }
            "opacity" => {
                if let Some(o) = parse_f32(val) {
                    inh.opacity = o.clamp(0.0, 1.0);
                }
            }
            "align-items" => {
                style.align = match lval.as_str() {
                    "center" => Align::Center,
                    "flex-end" | "end" => Align::End,
                    "flex-start" | "start" => Align::Start,
                    _ => Align::Stretch,
                };
            }
            "justify-content" => {
                style.justify = match lval.as_str() {
                    "center" => Justify::Center,
                    "flex-end" | "end" => Justify::End,
                    "space-between" => Justify::SpaceBetween,
                    _ => Justify::Start,
                };
            }
            _ => {}
        }
    }
    // opacity dims the box's own paint as well as inherited text color
    if inh.opacity < 1.0 {
        if let Some(bg) = &mut style.background {
            bg.a *= inh.opacity;
        }
        if let Some((_, c)) = &mut style.border {
            c.a *= inh.opacity;
        }
        if let Some(sh) = &mut style.shadow {
            sh.color.a *= inh.opacity;
        }
    }
    visible
}

fn parse_f32(s: &str) -> Option<f32> {
    s.trim().parse().ok()
}

fn parse_px(s: &str) -> Option<f32> {
    let s = s.trim();
    let s = s
        .strip_suffix("px")
        .or_else(|| s.strip_suffix("pt"))
        .unwrap_or(s)
        .trim();
    s.parse().ok()
}

fn parse_dim(s: &str) -> Dim {
    let s = s.trim();
    if s == "auto" {
        Dim::Auto
    } else if let Some(p) = s.strip_suffix('%').and_then(|v| v.trim().parse().ok()) {
        Dim::Pct(p)
    } else if let Some(p) = parse_px(s) {
        Dim::Px(p)
    } else {
        Dim::Auto
    }
}

/// CSS 1-4 value shorthand → TRBL edges.
fn parse_edges(s: &str) -> Option<Edges> {
    let v: Vec<f32> = s.split_whitespace().filter_map(parse_px).collect();
    match v.len() {
        1 => Some(Edges::all(v[0])),
        2 => Some(Edges::xy(v[1], v[0])),
        3 => Some(Edges {
            t: v[0],
            r: v[1],
            b: v[2],
            l: v[1],
        }),
        4 => Some(Edges {
            t: v[0],
            r: v[1],
            b: v[2],
            l: v[3],
        }),
        _ => None,
    }
}

fn set_edge(e: &mut Edges, val: &str, side: char) {
    if let Some(p) = parse_px(val) {
        match side {
            'l' => e.l = p,
            't' => e.t = p,
            'r' => e.r = p,
            _ => e.b = p,
        }
    }
}

/// `box-shadow: dx dy [blur [spread]] color` — the color may come first or last.
/// A parenthesized function color (`rgba(...)`, `hsl(...)`) is extracted whole so its
/// internal spaces and commas can't be mistaken for lengths.
fn parse_shadow(s: &str) -> Option<Shadow> {
    let mut rest = s.trim().to_string();
    let mut color_str = String::new();
    // pull out a functional color first, wherever it sits
    if let (Some(open), Some(close)) = (rest.find('('), rest.rfind(')')) {
        if close > open {
            let start = rest[..open]
                .rfind(char::is_whitespace)
                .map(|i| i + 1)
                .unwrap_or(0);
            color_str = rest[start..=close].to_string();
            rest.replace_range(start..=close, " ");
        }
    }
    let mut nums: Vec<f32> = Vec::new();
    for p in rest.split_whitespace() {
        let numeric_start = p
            .chars()
            .next()
            .map(|c| c.is_ascii_digit() || c == '-' || c == '.')
            .unwrap_or(false);
        match parse_px(p) {
            Some(v) if numeric_start => nums.push(v),
            _ => {
                // a bare keyword/hex color token, before or after the lengths
                if !color_str.is_empty() {
                    color_str.push(' ');
                }
                color_str.push_str(p);
            }
        }
    }
    if nums.len() < 2 {
        return None;
    }
    let color = parse_color(color_str.trim()).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 0.35));
    Some(Shadow {
        dx: nums[0],
        dy: nums[1],
        blur: nums.get(2).copied().unwrap_or(0.0).max(0.5),
        color,
    })
}

fn parse_color(s: &str) -> Option<Rgba> {
    // CSS colors are case-insensitive; normalize once so RGB(...) and #ABC work
    let s = s.trim().to_ascii_lowercase();
    let s = s.as_str();
    if let Some(hex) = s.strip_prefix('#') {
        if !hex.is_ascii() {
            return None; // byte-indexed below; multibyte input must not slice mid-char
        }
        let dup = |i: usize| u8::from_str_radix(&hex[i..i + 1].repeat(2), 16).ok();
        let two = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return match hex.len() {
            3 => Some(Rgba::rgb8(dup(0)?, dup(1)?, dup(2)?)),
            4 => Some(Rgba::rgb8(dup(0)?, dup(1)?, dup(2)?).with_alpha(dup(3)? as f32 / 255.0)),
            6 => Some(Rgba::rgb8(two(0)?, two(2)?, two(4)?)),
            8 => Some(Rgba::rgb8(two(0)?, two(2)?, two(4)?).with_alpha(two(6)? as f32 / 255.0)),
            _ => None,
        };
    }
    let inner = |prefix: &str| -> Option<Vec<String>> {
        s.strip_prefix(prefix)
            .and_then(|x| x.strip_suffix(')'))
            .map(|x| {
                x.split([',', '/'])
                    .flat_map(|p| p.split_whitespace())
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
    };
    if let Some(parts) = inner("rgba(").or_else(|| inner("rgb(")) {
        if parts.len() >= 3 {
            let ch = |p: &str| -> Option<f32> {
                if let Some(pc) = p.strip_suffix('%') {
                    pc.parse::<f32>().ok().map(|v| v / 100.0 * 255.0)
                } else {
                    p.parse::<f32>().ok()
                }
            };
            let r = ch(&parts[0])?;
            let g = ch(&parts[1])?;
            let b = ch(&parts[2])?;
            let a = match parts.get(3) {
                Some(p) if p.ends_with('%') => p.trim_end_matches('%').parse::<f32>().ok()? / 100.0,
                Some(p) => p.parse::<f32>().ok()?,
                None => 1.0,
            };
            return Some(Rgba::new(
                (r / 255.0).clamp(0.0, 1.0),
                (g / 255.0).clamp(0.0, 1.0),
                (b / 255.0).clamp(0.0, 1.0),
                a.clamp(0.0, 1.0),
            ));
        }
    }
    if let Some(parts) = inner("hsla(").or_else(|| inner("hsl(")) {
        if parts.len() >= 3 {
            let h: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
            let sa: f32 = parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let l: f32 = parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let a = parts
                .get(3)
                .and_then(|p| p.trim_end_matches('%').parse::<f32>().ok())
                .map(|v| {
                    if parts[3].ends_with('%') {
                        v / 100.0
                    } else {
                        v
                    }
                })
                .unwrap_or(1.0);
            return Some(hsl_to_rgba(h, sa, l, a));
        }
    }
    named_color(s)
}

fn hsl_to_rgba(h: f32, s: f32, l: f32, a: f32) -> Rgba {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c * 0.5;
    Rgba::new(r + m, g + m, b + m, a)
}

fn named_color(s: &str) -> Option<Rgba> {
    let (r, g, b, a) = match s {
        "transparent" => (0, 0, 0, 0u8),
        "white" => (255, 255, 255, 255),
        "black" => (0, 0, 0, 255),
        "gray" | "grey" => (128, 128, 128, 255),
        "silver" => (192, 192, 192, 255),
        "lightgray" | "lightgrey" => (211, 211, 211, 255),
        "darkgray" | "darkgrey" => (169, 169, 169, 255),
        "dimgray" | "dimgrey" => (105, 105, 105, 255),
        "slategray" | "slategrey" => (112, 128, 144, 255),
        "whitesmoke" => (245, 245, 245, 255),
        "red" => (220, 70, 70, 255),
        "darkred" => (139, 0, 0, 255),
        "crimson" => (220, 20, 60, 255),
        "salmon" => (250, 128, 114, 255),
        "coral" => (255, 127, 80, 255),
        "orange" => (255, 165, 0, 255),
        "gold" => (255, 215, 0, 255),
        "yellow" => (250, 204, 21, 255),
        "khaki" => (240, 230, 140, 255),
        "green" => (60, 190, 120, 255),
        "darkgreen" => (0, 100, 0, 255),
        "lime" => (132, 204, 22, 255),
        "olive" => (128, 128, 0, 255),
        "teal" => (20, 184, 166, 255),
        "cyan" | "aqua" => (34, 211, 238, 255),
        "turquoise" => (64, 224, 208, 255),
        "blue" => (80, 140, 230, 255),
        "navy" => (0, 0, 128, 255),
        "royalblue" => (65, 105, 225, 255),
        "skyblue" => (135, 206, 235, 255),
        "steelblue" => (70, 130, 180, 255),
        "indigo" => (99, 102, 241, 255),
        "purple" => (168, 85, 247, 255),
        "violet" => (238, 130, 238, 255),
        "magenta" | "fuchsia" => (232, 121, 249, 255),
        "pink" => (244, 114, 182, 255),
        "orchid" => (218, 112, 214, 255),
        "plum" => (221, 160, 221, 255),
        "brown" => (165, 42, 42, 255),
        "maroon" => (128, 0, 0, 255),
        "tan" => (210, 180, 140, 255),
        "beige" => (245, 245, 220, 255),
        "ivory" => (255, 255, 240, 255),
        "rebeccapurple" => (102, 51, 153, 255),
        _ => return None,
    };
    Some(Rgba::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pmre_kit::ux::UxNode;

    fn count_rich(node: &UxNode) -> usize {
        match node {
            UxNode::Rich { .. } => 1,
            UxNode::Text { .. } => 0,
            UxNode::Box { children, .. } => children.iter().map(count_rich).sum(),
        }
    }

    #[test]
    fn inline_elements_coalesce_into_one_flow() {
        let doc = "<p>plain <b>bold</b> and <a>linked</a> words</p>";
        let root = parse(doc);
        assert_eq!(count_rich(&root), 1, "one paragraph = one rich flow");
        // find the flow and check the span styles
        fn find(node: &UxNode) -> Option<&Vec<pmre_kit::ux::Span>> {
            match node {
                UxNode::Rich { spans, .. } => Some(spans),
                UxNode::Box { children, .. } => children.iter().find_map(find),
                _ => None,
            }
        }
        let spans = find(&root).expect("rich flow exists");
        assert!(spans.iter().any(|s| s.bold && s.text.contains("bold")));
        assert!(spans
            .iter()
            .any(|s| s.underline && s.text.contains("linked")));
        assert!(spans.iter().any(|s| !s.bold && s.text.contains("plain")));
    }

    #[test]
    fn comments_scripts_and_entities() {
        let doc = "<!-- c --><div><script>var x = '<div>';</script>a &amp; b &lt;ok&gt;</div>";
        let root = parse(doc);
        fn text_of(node: &UxNode, out: &mut String) {
            match node {
                UxNode::Rich { spans, .. } => {
                    for s in spans {
                        out.push_str(&s.text);
                    }
                }
                UxNode::Text { content, .. } => out.push_str(content),
                UxNode::Box { children, .. } => children.iter().for_each(|c| text_of(c, out)),
            }
        }
        let mut t = String::new();
        text_of(&root, &mut t);
        assert!(t.contains("a & b <ok>"), "got {t:?}");
        assert!(!t.contains("var x"), "script content must be dropped");
    }

    #[test]
    fn colors_and_shadows_parse() {
        assert!(parse_color("#abc").is_some());
        assert!(parse_color("#aabbccdd").map(|c| c.a < 1.0).unwrap_or(false));
        let c = parse_color("rgba(255, 0, 0, 0.5)").unwrap();
        assert!(c.r > 0.99 && (c.a - 0.5).abs() < 0.01);
        assert!(parse_color("hsl(120, 50%, 50%)")
            .map(|c| c.g > c.r)
            .unwrap_or(false));
        assert!(parse_color("rebeccapurple").is_some());
        let sh = parse_shadow("0 4px 12px rgba(0,0,0,0.4)").unwrap();
        assert!((sh.dy - 4.0).abs() < 0.01 && (sh.blur - 12.0).abs() < 0.01);
    }

    #[test]
    fn margins_percent_and_display_none() {
        let doc = r#"<div>
            <div style="width:50%; margin:10px 20px"></div>
            <div style="display:none">hidden</div>
        </div>"#;
        let root = parse(doc);
        let UxNode::Box { children, .. } = &root else {
            panic!("root is a box")
        };
        assert_eq!(children.len(), 1, "display:none child dropped");
        let UxNode::Box { style, .. } = &children[0] else {
            panic!("child is a box")
        };
        assert!(matches!(style.width, Dim::Pct(p) if (p - 50.0).abs() < 0.01));
        assert!((style.margin.l - 20.0).abs() < 0.01 && (style.margin.t - 10.0).abs() < 0.01);
    }
}
