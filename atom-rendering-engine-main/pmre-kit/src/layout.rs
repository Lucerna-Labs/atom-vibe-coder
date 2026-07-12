//! The reduced layout core: box-model + block/flex solving, then box → draw commands,
//! plus interaction support (ids carried through to laid boxes, clip rects for scroll
//! regions, and hit-testing). Pure mechanism: deterministic, decision-free.
//! `solve` returns a flat pre-order list (parents before children = paint order).

use crate::geom::{Affine, Vec2};
use crate::paint::{Bounds, DrawCmd, Paint, Rgba, Shape};
use crate::ux::{Align, Dim, Dir, Edges, Justify, Role, Shadow, Span, Style, UxNode};
use pmre_transparency_core::TransparencyMaterial;

/// What a laid-out box paints as.
#[derive(Clone, Debug)]
pub enum Painted {
    Box {
        background: Option<Rgba>,
        radius: f32,
        border: Option<(f32, Rgba)>,
        shadow: Option<Shadow>,
        transparency: Option<TransparencyMaterial>,
    },
    Text {
        content: String,
        size: f32,
        color: Rgba,
    },
    /// A rich inline flow; the painter re-breaks the spans at the solved rect width.
    Rich { spans: Vec<Span>, align: Align },
}

/// A node with its solved device-space rectangle and interaction metadata.
#[derive(Clone, Debug)]
pub struct LaidBox {
    pub rect: Bounds,
    pub kind: Painted,
    pub id: Option<u32>,
    pub role: Role,
    /// Clip rectangle this box is confined to (set for descendants of a scroll region).
    pub clip: Option<Bounds>,
    /// For a `Scroll` box: the natural height of its content (for scrollbar + clamping).
    pub content_len: f32,
}

/// A scroll-offset lookup: given a scroll box id, return its current vertical offset.
pub type ScrollFn<'a> = dyn Fn(u32) -> f32 + 'a;

/// Solve layout for `root` inside `viewport`. `scroll` supplies each scroll region's offset.
pub fn solve(root: &UxNode, viewport: Bounds, scroll: &ScrollFn) -> Vec<LaidBox> {
    // the memo is keyed by node address — valid only while this tree is alive, so it
    // must be reset at every entry
    MEASURE_MEMO.with(|m| m.borrow_mut().clear());
    let mut out = Vec::new();
    layout_node(root, viewport, None, scroll, &mut out);
    out
}

thread_local! {
    /// Per-solve memo for `measure`: without it, the flex-row second pass measures
    /// children twice per level, which goes exponential on deeply nested rows.
    static MEASURE_MEMO: std::cell::RefCell<std::collections::HashMap<(usize, u32), (f32, f32)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Topmost interactive box containing the point (respecting clip), as `(id, role)`.
pub fn hit_test(boxes: &[LaidBox], x: f32, y: f32) -> Option<(u32, Role)> {
    let mut found = None;
    for b in boxes {
        let Some(id) = b.id else { continue };
        if !contains(b.rect, x, y) {
            continue;
        }
        if let Some(clip) = b.clip {
            if !contains(clip, x, y) {
                continue;
            }
        }
        found = Some((id, b.role)); // later in pre-order = drawn on top
    }
    found
}

fn contains(b: Bounds, x: f32, y: f32) -> bool {
    x >= b.min.x && x < b.max.x && y >= b.min.y && y < b.max.y
}

/// Text advance width — single source of truth shared with the glyph rasterizer.
pub fn text_width(content: &str, size: f32) -> f32 {
    crate::text::advance(content, size)
}

// ── Rich inline flow ─────────────────────────────────────────────────────────

/// One placed fragment of a wrapped rich line: a same-style run at `x` within the line.
#[derive(Clone, Debug)]
pub struct RichPiece {
    pub text: String,
    pub size: f32,
    pub color: Rgba,
    pub background: Option<Rgba>,
    pub bold: bool,
    pub underline: bool,
    pub x: f32,
    pub width: f32,
}

/// One wrapped line of a rich flow.
#[derive(Clone, Debug)]
pub struct RichLine {
    pub pieces: Vec<RichPiece>,
    pub width: f32,
}

/// One same-style fragment of a flow word: `si` indexes the span it came from.
struct RichFrag {
    si: usize,
    text: String,
    width: f32,
    overlay: bool,
}

/// A wrap-atomic word: consecutive non-whitespace fragments, possibly spanning styles.
struct RichWord {
    space_before: bool,
    frags: Vec<RichFrag>,
    width: f32,
}

/// Group styled spans into wrap-atomic words. Whitespace anywhere separates words; a
/// span boundary does not, so "hel" + "lo" from adjacent spans wraps as one word.
/// Overlay spans attach as zero-width fragments at their flow position.
fn rich_words(spans: &[Span]) -> Vec<RichWord> {
    let mut words: Vec<RichWord> = Vec::new();
    let mut open: Option<RichWord> = None;
    let mut pending_space = false;
    for (si, span) in spans.iter().enumerate() {
        if span.overlay {
            let frag = RichFrag {
                si,
                text: span.text.clone(),
                width: 0.0,
                overlay: true,
            };
            match open.as_mut() {
                Some(word) => word.frags.push(frag),
                None => {
                    open = Some(RichWord {
                        space_before: pending_space,
                        frags: vec![frag],
                        width: 0.0,
                    });
                    pending_space = false;
                }
            }
            continue;
        }
        let mut rest = span.text.as_str();
        while !rest.is_empty() {
            let ws = rest.starts_with(char::is_whitespace);
            let run_end = rest
                .find(|c: char| c.is_whitespace() != ws)
                .unwrap_or(rest.len());
            let run = &rest[..run_end];
            rest = &rest[run_end..];
            if ws {
                if let Some(word) = open.take() {
                    words.push(word);
                }
                pending_space = true;
            } else {
                let width = crate::text::advance_styled(run, span.size, span.bold);
                let word = open.get_or_insert_with(|| {
                    let space_before = pending_space;
                    pending_space = false;
                    RichWord {
                        space_before,
                        frags: Vec::new(),
                        width: 0.0,
                    }
                });
                word.frags.push(RichFrag {
                    si,
                    text: run.to_string(),
                    width,
                    overlay: false,
                });
                word.width += width;
            }
        }
    }
    if let Some(word) = open.take() {
        words.push(word);
    }
    words
}

/// Break styled spans into wrapped lines (greedy, word-granular). Returns the lines and
/// the uniform line height (1.3× the largest span size). Spans flow inline: a word from
/// a bold span continues the same line as the plain words before it, and a word split
/// across spans never breaks at the span boundary. A word wider than the line is split
/// at character granularity. Overlay spans place zero-width pieces that never re-wrap.
pub fn rich_lines(spans: &[Span], max_width: Option<f32>) -> (Vec<RichLine>, f32) {
    let max_size = spans.iter().map(|s| s.size).fold(1.0f32, f32::max);
    let line_h = max_size * 1.3;
    let max_w = max_width.filter(|w| *w > 0.0).unwrap_or(f32::INFINITY);

    let mut lines: Vec<RichLine> = Vec::new();
    let mut cur = RichLine {
        pieces: Vec::new(),
        width: 0.0,
    };
    // The span index of the piece currently being extended, for run merging.
    let mut cur_span: Option<usize> = None;
    let new_line = || RichLine {
        pieces: Vec::new(),
        width: 0.0,
    };
    let piece_for = |frag: &RichFrag, spans: &[Span], x: f32, text: &str, width: f32| {
        let span = &spans[frag.si];
        RichPiece {
            text: text.to_string(),
            size: span.size,
            color: span.color,
            background: span.background,
            bold: span.bold,
            underline: span.underline,
            x,
            width,
        }
    };

    for word in rich_words(spans) {
        let first_text = word.frags.iter().find(|f| !f.overlay);
        let lead = first_text.unwrap_or(&word.frags[0]);
        let space_w = if word.space_before && !cur.pieces.is_empty() {
            crate::text::advance_styled(" ", spans[lead.si].size, spans[lead.si].bold)
        } else {
            0.0
        };
        if first_text.is_none() {
            // overlay-only word (e.g. a caret between words): zero width, never wraps
            for frag in &word.frags {
                cur.pieces
                    .push(piece_for(frag, spans, cur.width + space_w, &frag.text, 0.0));
            }
            cur_span = None;
            continue;
        }
        if word.width > max_w + crate::text::WRAP_SLACK {
            // wider than a whole line: flush, then split at character granularity. A line
            // holding only zero-width overlays (a caret at the word start) counts as empty
            // so the overlay never forces an extra flush.
            if cur.pieces.iter().any(|p| p.width > 0.0) {
                lines.push(std::mem::replace(&mut cur, new_line()));
            }
            for frag in &word.frags {
                if frag.overlay {
                    cur.pieces
                        .push(piece_for(frag, spans, cur.width, &frag.text, 0.0));
                    continue;
                }
                let span = &spans[frag.si];
                let mut rest = frag.text.as_str();
                while !rest.is_empty() {
                    let avail = max_w - cur.width;
                    let n = crate::text::fit_chars_styled(rest, span.size, span.bold, avail);
                    if n == 0 && cur.pieces.iter().any(|p| p.width > 0.0) {
                        // a positive-width prefix already sits on this line — flush and
                        // retry; a line with only a zero-width caret is treated as empty
                        // so the caret overlay never spawns a phantom line.
                        lines.push(std::mem::replace(&mut cur, new_line()));
                        continue;
                    }
                    let n = n.max(1);
                    let cut = rest
                        .char_indices()
                        .nth(n)
                        .map(|(i, _)| i)
                        .unwrap_or(rest.len());
                    let chunk = &rest[..cut];
                    let chunk_w = crate::text::advance_styled(chunk, span.size, span.bold);
                    cur.pieces
                        .push(piece_for(frag, spans, cur.width, chunk, chunk_w));
                    cur.width += chunk_w;
                    rest = &rest[cut..];
                }
            }
            cur_span = None;
            continue;
        }
        if !cur.pieces.is_empty()
            && cur.width + space_w + word.width > max_w + crate::text::WRAP_SLACK
        {
            lines.push(std::mem::replace(&mut cur, new_line()));
            cur_span = None;
        }
        let space_w = if cur.pieces.is_empty() { 0.0 } else { space_w };
        if word.frags.len() == 1 && cur_span == Some(lead.si) && !cur.pieces.is_empty() {
            // same style run continues on this line — extend the piece
            let piece = cur.pieces.last_mut().unwrap();
            if space_w > 0.0 {
                piece.text.push(' ');
            }
            piece.text.push_str(&lead.text);
            piece.width += space_w + lead.width;
            cur.width += space_w + lead.width;
        } else {
            let mut x = cur.width + space_w;
            for frag in &word.frags {
                if frag.overlay {
                    cur.pieces.push(piece_for(frag, spans, x, &frag.text, 0.0));
                } else {
                    cur.pieces
                        .push(piece_for(frag, spans, x, &frag.text, frag.width));
                    x += frag.width;
                }
            }
            cur.width = x;
            cur_span = word
                .frags
                .iter()
                .rev()
                .find(|f| !f.overlay)
                .filter(|_| word.frags.iter().filter(|f| !f.overlay).count() == 1)
                .map(|f| f.si);
        }
    }
    if !cur.pieces.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(RichLine {
            pieces: Vec::new(),
            width: 0.0,
        });
    }
    (lines, line_h)
}

fn extent(b: Bounds) -> (f32, f32) {
    (b.max.x - b.min.x, b.max.y - b.min.y)
}

fn inset(rect: Bounds, p: Edges, border: f32) -> Bounds {
    Bounds {
        min: Vec2::new(rect.min.x + p.l + border, rect.min.y + p.t + border),
        max: Vec2::new(rect.max.x - p.r - border, rect.max.y - p.b - border),
    }
}

fn clip_to(parent: Option<Bounds>, inner: Bounds) -> Option<Bounds> {
    match parent {
        None => Some(inner),
        Some(p) => Some(Bounds {
            min: Vec2::new(p.min.x.max(inner.min.x), p.min.y.max(inner.min.y)),
            max: Vec2::new(p.max.x.min(inner.max.x), p.max.y.min(inner.max.y)),
        }),
    }
}

fn node_dim(node: &UxNode, want_width: bool) -> Dim {
    match node {
        UxNode::Box { style, .. } => {
            if want_width {
                style.width
            } else {
                style.height
            }
        }
        UxNode::Text { .. } | UxNode::Rich { .. } => Dim::Auto,
    }
}

/// Margin edges of a node (only boxes carry style).
fn node_margin(node: &UxNode) -> Edges {
    match node {
        UxNode::Box { style, .. } => style.margin,
        _ => Edges::default(),
    }
}

/// Intrinsic (content) size of a node — the node's **outer** size, margins included.
/// When `avail_w` is given, text wraps to it and the returned height reflects the
/// wrapped line count (so a column reserves the right height). Memoized per solve.
fn measure(node: &UxNode, avail_w: Option<f32>) -> (f32, f32) {
    let key = (
        node as *const UxNode as usize,
        avail_w.map(f32::to_bits).unwrap_or(u32::MAX),
    );
    if let Some(v) = MEASURE_MEMO.with(|m| m.borrow().get(&key).copied()) {
        return v;
    }
    let v = measure_inner(node, avail_w);
    MEASURE_MEMO.with(|m| m.borrow_mut().insert(key, v));
    v
}

fn measure_inner(node: &UxNode, avail_w: Option<f32>) -> (f32, f32) {
    match node {
        UxNode::Text { content, size, .. } => {
            let single = text_width(content, *size);
            let line_h = size * 1.3;
            match avail_w {
                Some(w) if w > 0.0 && single > w => {
                    let lines = crate::text::wrap(content, *size, w).len().max(1);
                    (w, lines as f32 * line_h)
                }
                _ => (single, line_h),
            }
        }
        UxNode::Rich { spans, .. } => {
            let (lines, line_h) = rich_lines(spans, avail_w.filter(|w| *w > 0.0));
            let w = lines.iter().map(|l| l.width).fold(0.0f32, f32::max);
            (w, lines.len() as f32 * line_h)
        }
        UxNode::Box { style, children } => {
            let bw = style.border.map(|(w, _)| w).unwrap_or(0.0);
            let pad_w = style.padding.l + style.padding.r + 2.0 * bw;
            let pad_h = style.padding.t + style.padding.b + 2.0 * bw;
            // When this box's width is knowable, propagate it so nested text wraps and
            // the measured height reflects the real wrapped line count.
            let m_lr = style.margin.l + style.margin.r;
            // `avail_w` is margin-box space offered by the parent; `self_w` is this box's
            // border-box width when knowable. Pct resolves against the parent content
            // extent exactly like the flex solver does, so measure and layout agree.
            let self_w = match style.width {
                Dim::Px(v) => Some(v),
                Dim::Pct(p) => avail_w.map(|w| (w * p / 100.0).max(0.0)),
                _ => avail_w.map(|w| (w - m_lr).max(0.0)),
            };
            let child_avail = match style.dir {
                Dir::Column => self_w.map(|w| (w - pad_w).max(0.0)),
                Dir::Row => None,
            };
            let n = children.len();
            let firsts: Vec<(f32, f32)> =
                children.iter().map(|ch| measure(ch, child_avail)).collect();
            let mut main = 0.0f32;
            let mut cross = 0.0f32;
            for (i, &(cw, chh)) in firsts.iter().enumerate() {
                let (cm, cc) = match style.dir {
                    Dir::Row => (cw, chh),
                    Dir::Column => (chh, cw),
                };
                main += cm;
                if i + 1 < n {
                    main += style.gap;
                }
                cross = cross.max(cc);
            }
            // Second pass for rows with a known width: give each child its real main-axis
            // share (mirroring the flex solver) and re-measure, so wrapped text inside
            // flex items reports its true height instead of a single-line estimate.
            // First-pass results are reused wherever the allotted width equals the
            // intrinsic width, keeping the recursion linear in the tree size.
            if matches!(style.dir, Dir::Row) && n > 0 {
                if let Some(w) = self_w {
                    let inner = (w - pad_w).max(0.0);
                    let mut bases = Vec::with_capacity(n);
                    let mut weights = Vec::with_capacity(n);
                    let (mut sum_base, mut sum_flex) = (0.0f32, 0.0f32);
                    for (i, ch) in children.iter().enumerate() {
                        let m = node_margin(ch);
                        let mm = m.l + m.r;
                        let (base, wt) = match node_dim(ch, true) {
                            Dim::Px(v) => (v + mm, 0.0),
                            Dim::Pct(p) => (p / 100.0 * inner + mm, 0.0),
                            Dim::Auto => (firsts[i].0, 0.0),
                            Dim::Flex(f) => (mm, f),
                        };
                        bases.push(base);
                        weights.push(wt);
                        sum_base += base;
                        sum_flex += wt;
                    }
                    let gaps = style.gap * (n as f32 - 1.0);
                    let free = (inner - sum_base - gaps).max(0.0);
                    let mut tallest = 0.0f32;
                    for (i, ch) in children.iter().enumerate() {
                        let auto = matches!(node_dim(ch, true), Dim::Auto);
                        let ch_h = if auto && weights[i] == 0.0 {
                            // allotted == intrinsic width — the first pass already
                            // measured this child at exactly that width
                            firsts[i].1
                        } else {
                            let allotted = bases[i]
                                + if sum_flex > 0.0 {
                                    free * weights[i] / sum_flex
                                } else {
                                    0.0
                                };
                            // `allotted` is margin-box space, which is what measure takes
                            measure(ch, Some(allotted)).1
                        };
                        tallest = tallest.max(ch_h);
                    }
                    cross = tallest;
                }
            }
            let (iw, ih) = match style.dir {
                Dir::Row => (main, cross),
                Dir::Column => (cross, main),
            };
            let w = match style.width {
                Dim::Px(v) => v,
                _ => iw + pad_w,
            };
            let h = match style.height {
                Dim::Px(v) => v,
                _ => ih + pad_h,
            };
            (
                w + style.margin.l + style.margin.r,
                h + style.margin.t + style.margin.b,
            )
        }
    }
}

fn layout_node(
    node: &UxNode,
    rect: Bounds,
    clip: Option<Bounds>,
    scroll: &ScrollFn,
    out: &mut Vec<LaidBox>,
) {
    match node {
        UxNode::Text {
            content,
            size,
            color,
        } => {
            out.push(LaidBox {
                rect,
                kind: Painted::Text {
                    content: content.clone(),
                    size: *size,
                    color: *color,
                },
                id: None,
                role: Role::None,
                clip,
                content_len: 0.0,
            });
        }
        UxNode::Rich { spans, align } => {
            out.push(LaidBox {
                rect,
                kind: Painted::Rich {
                    spans: spans.clone(),
                    align: *align,
                },
                id: None,
                role: Role::None,
                clip,
                content_len: 0.0,
            });
        }
        UxNode::Box { style, children } => {
            // the given rect is the outer (margin) box; margins are outside the border
            let rect = inset(rect, style.margin, 0.0);
            let bw = style.border.map(|(w, _)| w).unwrap_or(0.0);
            let content = inset(rect, style.padding, bw);

            if style.role == Role::Scroll {
                let (cw, _) = extent(content);
                let content_len = scroll_content_height(children, cw, style.gap);
                out.push(LaidBox {
                    rect,
                    kind: Painted::Box {
                        background: style.background,
                        radius: style.radius,
                        border: style.border,
                        shadow: style.shadow,
                        transparency: style.transparency,
                    },
                    id: style.id,
                    role: style.role,
                    clip,
                    content_len,
                });
                let inner_clip = clip_to(clip, rect);
                // Re-clamp the stored offset against the *current* content height, so a
                // list that shrinks while scrolled (e.g. items deleted) never renders
                // scrolled past its own end.
                let view_h = rect.max.y - rect.min.y;
                let off = style
                    .id
                    .map(scroll)
                    .unwrap_or(0.0)
                    .clamp(0.0, (content_len - view_h).max(0.0));
                let mut cursor = content.min.y - off;
                for ch in children {
                    let (_, chh) = measure(ch, Some(cw));
                    let child_rect = Bounds {
                        min: Vec2::new(content.min.x, cursor),
                        max: Vec2::new(content.min.x + cw, cursor + chh),
                    };
                    layout_node(ch, child_rect, inner_clip, scroll, out);
                    cursor += chh + style.gap;
                }
            } else {
                out.push(LaidBox {
                    rect,
                    kind: Painted::Box {
                        background: style.background,
                        radius: style.radius,
                        border: style.border,
                        shadow: style.shadow,
                        transparency: style.transparency,
                    },
                    id: style.id,
                    role: style.role,
                    clip,
                    content_len: 0.0,
                });
                layout_children(style, children, content, clip, scroll, out);
            }
        }
    }
}

fn scroll_content_height(children: &[UxNode], width: f32, gap: f32) -> f32 {
    let n = children.len();
    let mut h = 0.0;
    for (i, ch) in children.iter().enumerate() {
        h += measure(ch, Some(width)).1;
        if i + 1 < n {
            h += gap;
        }
    }
    h
}

fn layout_children(
    style: &Style,
    children: &[UxNode],
    content: Bounds,
    clip: Option<Bounds>,
    scroll: &ScrollFn,
    out: &mut Vec<LaidBox>,
) {
    let n = children.len();
    if n == 0 {
        return;
    }
    let (cw, chh) = extent(content);
    let main_is_width = matches!(style.dir, Dir::Row);
    let (main_extent, cross_extent) = if main_is_width { (cw, chh) } else { (chh, cw) };
    let (main_start, cross_start) = if main_is_width {
        (content.min.x, content.min.y)
    } else {
        (content.min.y, content.min.x)
    };
    let avail_for_child = if main_is_width {
        None
    } else {
        Some(cross_extent)
    };

    let mut bases = Vec::with_capacity(n);
    let mut weights = Vec::with_capacity(n);
    let mut sum_base = 0.0f32;
    let mut sum_flex = 0.0f32;
    for ch in children {
        let (mw, mh) = measure(ch, avail_for_child);
        let measured_main = if main_is_width { mw } else { mh };
        let m = node_margin(ch);
        let m_main = if main_is_width { m.l + m.r } else { m.t + m.b };
        let (base, weight) = match node_dim(ch, main_is_width) {
            Dim::Px(v) => (v + m_main, 0.0),
            Dim::Pct(p) => (p / 100.0 * main_extent + m_main, 0.0),
            Dim::Auto => (measured_main, 0.0),
            // `flex:N` is `flex-basis: 0` — size by grow share alone, so items fit (and shrink).
            Dim::Flex(w) => (m_main, w),
        };
        bases.push(base);
        weights.push(weight);
        sum_base += base;
        sum_flex += weight;
    }
    let gaps = style.gap * (n as f32 - 1.0);
    let free = (main_extent - sum_base - gaps).max(0.0);
    let mains: Vec<f32> = (0..n)
        .map(|i| {
            if sum_flex > 0.0 {
                bases[i] + free * (weights[i] / sum_flex)
            } else {
                bases[i]
            }
        })
        .collect();

    let leftover = if sum_flex > 0.0 { 0.0 } else { free };
    let (lead, between_extra) = justify_offsets(style.justify, leftover, n);

    let mut cursor = main_start + lead;
    for (i, ch) in children.iter().enumerate() {
        let cm = mains[i];
        // In a row, re-measure the child at its solved width so wrapping content
        // reports its true height for non-Stretch alignment.
        let cross_avail = if main_is_width {
            Some(cm)
        } else {
            avail_for_child
        };
        let (mw, mh) = measure(ch, cross_avail);
        let measured_cross = if main_is_width { mh } else { mw };
        let m = node_margin(ch);
        let m_cross = if main_is_width { m.t + m.b } else { m.l + m.r };
        let cc = match node_dim(ch, !main_is_width) {
            Dim::Px(v) => v + m_cross,
            Dim::Pct(p) => p / 100.0 * cross_extent + m_cross,
            _ => {
                if matches!(style.align, Align::Stretch) {
                    cross_extent
                } else {
                    measured_cross
                }
            }
        };
        let cross_pos = align_pos(style.align, cross_start, cross_extent, cc);
        let rect = if main_is_width {
            Bounds {
                min: Vec2::new(cursor, cross_pos),
                max: Vec2::new(cursor + cm, cross_pos + cc),
            }
        } else {
            Bounds {
                min: Vec2::new(cross_pos, cursor),
                max: Vec2::new(cross_pos + cc, cursor + cm),
            }
        };
        layout_node(ch, rect, clip, scroll, out);
        cursor += cm + style.gap + between_extra;
    }
}

fn justify_offsets(j: Justify, free: f32, n: usize) -> (f32, f32) {
    match j {
        Justify::Start => (0.0, 0.0),
        Justify::Center => (free / 2.0, 0.0),
        Justify::End => (free, 0.0),
        Justify::SpaceBetween => {
            if n > 1 {
                (0.0, free / (n as f32 - 1.0))
            } else {
                (0.0, 0.0)
            }
        }
    }
}

fn align_pos(a: Align, cross_start: f32, cross_extent: f32, item_cross: f32) -> f32 {
    match a {
        Align::Start | Align::Stretch => cross_start,
        Align::Center => cross_start + (cross_extent - item_cross) / 2.0,
        Align::End => cross_start + (cross_extent - item_cross),
    }
}

fn center_half(r: Bounds) -> (Vec2, Vec2) {
    (
        Vec2::new((r.min.x + r.max.x) / 2.0, (r.min.y + r.max.y) / 2.0),
        Vec2::new((r.max.x - r.min.x) / 2.0, (r.max.y - r.min.y) / 2.0),
    )
}

/// Emit the draw commands for one laid-out box (background + border), appending to `out`.
pub fn cmds_for(b: &LaidBox, out: &mut Vec<DrawCmd>) {
    let (center, half) = center_half(b.rect);
    if half.x <= 0.0 || half.y <= 0.0 {
        return;
    }
    let at = Affine::translate(center.x, center.y);
    if let Painted::Box {
        background,
        radius,
        border,
        shadow,
        ..
    } = &b.kind
    {
        let r = radius.min(half.x).min(half.y).max(0.0);
        if let Some(sh) = shadow {
            // Soft falloff via a wide AA band over the same rounded-rect SDF.
            out.push(DrawCmd {
                shape: Shape::RoundedRect { half, radius: r },
                paint: Paint::Solid(sh.color),
                transform: Affine::translate(center.x + sh.dx, center.y + sh.dy),
                soft: sh.blur.max(0.5),
            });
        }
        match border {
            Some((bw, bc)) => {
                out.push(DrawCmd::new(
                    Shape::RoundedRect { half, radius: r },
                    Paint::Solid(*bc),
                    at,
                ));
                if let Some(bg) = background {
                    let inner = Vec2::new((half.x - bw).max(0.0), (half.y - bw).max(0.0));
                    out.push(DrawCmd::new(
                        Shape::RoundedRect {
                            half: inner,
                            radius: (r - bw).max(0.0),
                        },
                        Paint::Solid(*bg),
                        at,
                    ));
                }
            }
            None => {
                if let Some(bg) = background {
                    out.push(DrawCmd::new(
                        Shape::RoundedRect { half, radius: r },
                        Paint::Solid(*bg),
                        at,
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod rich_flow_tests {
    use super::*;
    use crate::paint::Rgba;

    fn plain(text: &str) -> Span {
        Span::new(text, 14.0, Rgba::rgb8(255, 255, 255))
    }

    #[test]
    fn caret_overlay_never_changes_line_breaks() {
        let text = "the quick brown fox jumps over the lazy dog again and again";
        let width = 180.0;
        let (base, _) = rich_lines(&[plain(text)], Some(width));
        assert!(base.len() > 1, "probe text must wrap for this test to bite");
        for caret in 0..=text.chars().count() {
            let before: String = text.chars().take(caret).collect();
            let after: String = text.chars().skip(caret).collect();
            let mut spans = Vec::new();
            if !before.is_empty() {
                spans.push(plain(before.as_str()));
            }
            spans.push(plain("|").overlay());
            if !after.is_empty() {
                spans.push(plain(after.as_str()));
            }
            let (lines, _) = rich_lines(&spans, Some(width));
            assert_eq!(lines.len(), base.len(), "caret at {caret} changed the wrap");
            for (with_caret, plain_line) in lines.iter().zip(&base) {
                assert!(
                    (with_caret.width - plain_line.width).abs() < 1e-3,
                    "caret at {caret} changed a line width"
                );
            }
        }
    }

    #[test]
    fn caret_overlay_adds_no_line_even_when_a_glyph_exceeds_the_width() {
        // Degenerate width narrower than a single glyph: the char-split path must still
        // treat a line holding only the zero-width caret as empty, so the overlay never
        // spawns a phantom line relative to the caret-less layout.
        let text = "widecharsWWWW";
        for tenths in 1..60 {
            let width = tenths as f32 * 0.5; // 0.5 .. 29.5 px, crossing the glyph width
            let base = rich_lines(&[plain(text)], Some(width)).0.len();
            for caret in 0..=text.chars().count() {
                let before: String = text.chars().take(caret).collect();
                let after: String = text.chars().skip(caret).collect();
                let mut spans = Vec::new();
                if !before.is_empty() {
                    spans.push(plain(before.as_str()));
                }
                spans.push(plain("|").overlay());
                if !after.is_empty() {
                    spans.push(plain(after.as_str()));
                }
                let with = rich_lines(&spans, Some(width)).0.len();
                assert_eq!(
                    with, base,
                    "caret at {caret} changed line count ({base} -> {with}) at width {width}"
                );
            }
        }
    }

    #[test]
    fn words_split_across_spans_wrap_as_one_word() {
        // "unbrea" + "kable" must wrap exactly like the single-span "unbreakable"
        let joined = rich_lines(&[plain("fill fill fill unbreakable")], Some(120.0)).0;
        let split = rich_lines(
            &[plain("fill fill fill unbrea"), plain("kable")],
            Some(120.0),
        )
        .0;
        assert_eq!(joined.len(), split.len());
        for (a, b) in joined.iter().zip(&split) {
            assert!((a.width - b.width).abs() < 1e-3);
        }
    }

    #[test]
    fn long_words_split_instead_of_overflowing() {
        let long = "x".repeat(300);
        let (lines, _) = rich_lines(&[plain(long.as_str())], Some(100.0));
        assert!(lines.len() > 1, "a 300-char word must wrap");
        let mut total_chars = 0usize;
        for line in &lines {
            assert!(line.width <= 100.0 + 1e-3, "no line may overflow");
            total_chars += line
                .pieces
                .iter()
                .map(|p| p.text.chars().count())
                .sum::<usize>();
        }
        assert_eq!(total_chars, 300, "splitting must not drop characters");
    }
}
