//! The Markdown of descriptions and Notes as the detail pane renders it.
//!
//! Records carry no mark of their format, so every description and Note is read as Markdown.
//! Before rendering, the source is rewritten on the syntax tree's positions: a line ending
//! inside a paragraph is made a hard break, as a GitHub comment shows it, so text that the CLI
//! or an agent wrapped by hand reads as written; a tag the text view would drop without a trace,
//! such as the placeholder `<id>`, is escaped so it shows as written; and an HTML block opened
//! by an image tag shows the image's alternative text.
//!
//! Images are never loaded: the text view's plugin renders a Markdown image and an HTML image
//! tag in a paragraph as their alternative text, and every other image it would still draw gets
//! a source that loads nothing.

use gpui_kit::{
    App, ClickEvent, ImageCacheError, ImageSource, MouseButton, SharedString, SharedUri, Window,
    base::text::{
        InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    },
};
use markdown::{ParseOptions, mdast::Node};
use std::{cell::RefCell, collections::HashMap, ops::Range, sync::Arc};

/// What stands for an image without alternative text.
pub const IMAGE_WITHOUT_ALT: &str = "（画像）";

/// The tags written in a paragraph that the text view renders, or that stand for what an
/// author would mean by them. Any other tag is shown as written.
const INLINE_TAGS: &[&str] = &[
    "a", "abbr", "b", "br", "code", "del", "em", "i", "image", "img", "ins", "kbd", "mark", "q",
    "s", "samp", "small", "span", "strike", "strong", "sub", "sup", "u", "var", "wbr",
];

/// The tags that open an HTML block as an author would mean them, besides [`INLINE_TAGS`]:
/// those of CommonMark's HTML blocks.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "pre",
    "script",
    "search",
    "section",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// The source the text view renders for `source`.
pub fn prepare(source: &str) -> String {
    let mut prepared = source.to_string();
    // Rewriting the start of an HTML block makes the block a paragraph, which may start another
    // block, so blocks are rewritten until none changes. Each pass changes at least one; the
    // bound only guards against a case not foreseen.
    for _ in 0..16 {
        match rewrite(&prepared, Pass::Blocks) {
            Some(rewritten) => prepared = rewritten,
            None => break,
        }
    }
    rewrite(&prepared, Pass::Inline).unwrap_or(prepared)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// HTML blocks whose first line the text view would not show as an author means it.
    Blocks,
    /// Line endings in paragraphs and tags inside them.
    Inline,
}

/// Where a node stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Block,
    Paragraph,
    /// In other inline content: a heading or a table cell.
    Inline,
    /// In inline math that the text view renders as the Markdown it holds.
    Math,
}

/// `source` with the edits of `pass`, or `None` when it has none.
fn rewrite(source: &str, pass: Pass) -> Option<String> {
    let root = markdown::to_mdast(source, &options()).ok()?;
    let mut edits = Vec::new();
    collect(&root, source, 0, pass, Place::Block, &mut edits);
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let mut rewritten = String::with_capacity(source.len() + edits.len() * 2);
    let mut at = 0;
    for (range, replacement) in edits {
        rewritten.push_str(&source[at..range.start]);
        rewritten.push_str(&replacement);
        at = range.end;
    }
    rewritten.push_str(&source[at..]);
    Some(rewritten)
}

/// The constructs the text view parses with (GPUI Base's `MarkdownExtensions::parse_options`
/// without extensions), so the tree here has the blocks the rendered one has.
fn options() -> ParseOptions {
    let mut options = ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    options
}

/// Collects the edits of `pass` for `node`, whose positions count from `base` in the source,
/// and for what it holds. `source` is the text from `base`.
fn collect(
    node: &Node,
    source: &str,
    base: usize,
    pass: Pass,
    place: Place,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    let Some(span) = node
        .position()
        .map(|position| position.start.offset..position.end.offset)
    else {
        return;
    };
    let start = base + span.start;
    match (node, pass) {
        (Node::Text(_), Pass::Inline) if matches!(place, Place::Paragraph | Place::Math) => {
            for ending in line_endings(&source[span]) {
                edits.push((start + ending.start..start + ending.end, "  ".into()));
            }
        }
        (Node::InlineMath(_), Pass::Inline) if place == Place::Paragraph => {
            let literal = &source[span];
            for child in unclaimed_math(literal) {
                collect(&child, literal, start, pass, Place::Math, edits);
            }
        }
        (Node::Html(_), Pass::Blocks) if place == Place::Block => {
            if let Some((range, replacement)) = block_start(&source[span]) {
                edits.push((start + range.start..start + range.end, replacement));
            }
        }
        // Escaped inside math, the tag would turn the math back into its literal.
        (Node::Html(_), Pass::Inline) if matches!(place, Place::Paragraph | Place::Inline) => {
            if tag_name(&source[span]).is_some_and(|name| !is_one_of(&name, INLINE_TAGS)) {
                edits.push((start..start, "\\".into()));
            }
        }
        _ => {
            let place = match node {
                Node::Paragraph(_) => Place::Paragraph,
                Node::Heading(_) | Node::TableCell(_) => Place::Inline,
                _ => place,
            };
            for child in node.children().into_iter().flatten() {
                collect(child, source, base, pass, place, edits);
            }
        }
    }
}

/// The edit for an HTML block `html` whose first line is a single tag that the text view would
/// not show as an author means it: an image tag becomes its alternative text, and a tag that is
/// no HTML an author writes, such as `<id>`, is escaped. Either makes the block a paragraph.
fn block_start(html: &str) -> Option<(Range<usize>, String)> {
    let line = html
        .split(['\n', '\r'])
        .next()
        .unwrap_or_default()
        .trim_end();
    // The block holds the indentation of its first line.
    let indent = line.len() - line.trim_start_matches(' ').len();
    let first_line = &line[indent..];
    if let Some((end, alt)) = image_tag(first_line, 0)
        && end == first_line.len()
    {
        return Some((indent..indent + end, literal_text(&decode_entities(&alt))));
    }
    let name = tag_name(first_line)?;
    (first_line.ends_with('>') && !is_one_of(&name, INLINE_TAGS) && !is_one_of(&name, BLOCK_TAGS))
        .then(|| (indent..indent, "\\".into()))
}

/// The name of the open or closing tag `html` starts with, or `None` for a comment, a
/// declaration or anything else.
fn tag_name(html: &str) -> Option<String> {
    let rest = html.strip_prefix('<')?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        .then_some(name)
}

fn is_one_of(name: &str, tags: &[&str]) -> bool {
    tags.iter().any(|tag| tag.eq_ignore_ascii_case(name))
}

/// `text` on one line, written so that it renders as exactly that text in Markdown: every
/// ASCII punctuation character is a character reference, so none starts markup or a block.
/// An empty text is the stand-in for an image.
fn literal_text(text: &str) -> String {
    let mut escaped = String::new();
    for c in shown(text).chars() {
        if c.is_ascii_punctuation() {
            escaped.push_str(&format!("&#{};", c as u32));
        } else {
            escaped.push(c);
        }
    }
    escaped
}

/// The white space before each line ending in `text`, up to where the ending begins. Two
/// spaces in its place make the ending a hard break; a tab there would keep it soft.
fn line_endings(text: &str) -> Vec<Range<usize>> {
    text.match_indices(['\n', '\r'])
        // A CRLF ending is one line ending, broken before its CR.
        .filter(|(ix, ending)| !(*ending == "\n" && text[..*ix].ends_with('\r')))
        .map(|(ix, _)| text[..ix].trim_end_matches([' ', '\t']).len()..ix)
        .collect()
}

/// The inline nodes the text view renders inline math `literal` as when nothing renders it as
/// math, or none when it keeps the literal as written. This follows GPUI Base's
/// `flatten_unclaimed_math`: the literal is parsed again without math, and replaces the math
/// when it parses to one paragraph that holds more than text.
fn unclaimed_math(literal: &str) -> Vec<Node> {
    let may_hold_markup = literal.bytes().any(|byte| {
        matches!(
            byte,
            b'<' | b'*' | b'_' | b'[' | b'`' | b'~' | b'\\' | b'!' | b'&'
        )
    }) || literal.contains("://")
        || literal.contains("www.");
    if !may_hold_markup {
        return Vec::new();
    }
    let Ok(Node::Root(mut root)) = markdown::to_mdast(literal, &ParseOptions::gfm()) else {
        return Vec::new();
    };
    match root.children.as_mut_slice() {
        [Node::Paragraph(paragraph)]
            if !paragraph
                .children
                .iter()
                .all(|child| matches!(child, Node::Text(_))) =>
        {
            std::mem::take(&mut paragraph.children)
        }
        _ => Vec::new(),
    }
}

/// Markdown that shows `text` as written, in a fenced code block whose fence is longer than
/// any run of backticks in it.
pub fn code_block(text: &str) -> String {
    let longest = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}\n{text}\n{fence}")
}

/// Renders an image in a paragraph, from Markdown or a lone HTML tag, as its alternative text,
/// which is selected and copied with the text around it.
pub struct ImageAlt;

impl MarkdownPlugin for ImageAlt {
    fn name(&self) -> &str {
        "axon-image-alt"
    }

    fn parse(&self, node: &Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let alt = alt_text(node)?;
        Some(MarkdownNode::new("axon-image-alt", ()).text(alt))
    }

    /// The text itself, laid out with the text around it.
    fn render_inline(
        &self,
        _: &MarkdownNode,
        _: &InlineRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<InlineElement> {
        None
    }
}

/// The alternative text of an image node of a paragraph, or of an HTML node that is a single
/// `<img>` tag.
fn alt_text(node: &Node) -> Option<String> {
    let alt = match node {
        Node::Image(image) => image.alt.clone(),
        Node::ImageReference(image) => image.alt.clone(),
        Node::Html(html) => {
            let tag = html.value.trim();
            let (end, alt) = image_tag(tag, 0)?;
            if end != tag.len() {
                return None;
            }
            decode_entities(&alt)
        }
        _ => return None,
    };
    Some(shown(&alt))
}

/// The alternative text on one line, or the stand-in for an image without one.
fn shown(alt: &str) -> String {
    let alt = alt.split_whitespace().collect::<Vec<_>>().join(" ");
    if alt.is_empty() {
        IMAGE_WITHOUT_ALT.to_string()
    } else {
        alt
    }
}

/// The end of the `<img>` or `<image>` tag (the same tag to an HTML parser) starting at
/// `start`, and the raw value of its first `alt` attribute (empty without one), or `None` when
/// no such tag starts there or it never closes.
fn image_tag(html: &str, start: usize) -> Option<(usize, String)> {
    let bytes = html.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return None;
    }
    let name = ["image", "img"].into_iter().find(|name| {
        let end = start + 1 + name.len();
        bytes
            .get(start + 1..end)
            .is_some_and(|found| found.eq_ignore_ascii_case(name.as_bytes()))
            && bytes
                .get(end)
                .is_none_or(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>'))
    })?;
    let mut at = start + 1 + name.len();
    let mut alt = None;
    loop {
        while bytes
            .get(at)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b'/')
        {
            at += 1;
        }
        if *bytes.get(at)? == b'>' {
            return Some((at + 1, alt.unwrap_or_default()));
        }
        let name_start = at;
        while bytes
            .get(at)
            .is_some_and(|c| !c.is_ascii_whitespace() && !matches!(c, b'=' | b'>' | b'/'))
        {
            at += 1;
        }
        let name = &html[name_start..at];
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let value = match *bytes.get(at)? {
            quote @ (b'"' | b'\'') => {
                let value_start = at + 1;
                let length = html[value_start..].find(quote as char)?;
                at = value_start + length + 1;
                &html[value_start..value_start + length]
            }
            _ => {
                let value_start = at;
                while bytes
                    .get(at)
                    .is_some_and(|c| !c.is_ascii_whitespace() && *c != b'>')
                {
                    at += 1;
                }
                &html[value_start..at]
            }
        };
        // An HTML parser keeps the first of a repeated attribute.
        if name.eq_ignore_ascii_case("alt") && alt.is_none() {
            alt = Some(value.to_string());
        }
    }
}

/// The common character references of an attribute value decoded; others stay as written.
fn decode_entities(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let reference = rest
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| Some((reference(&rest[1..end])?, end)));
        match reference {
            Some((c, end)) => {
                decoded.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn reference(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code).filter(|c| *c != '\0')
        }
    }
}

/// The source of every image the text view would still draw: one that never loads.
pub fn no_image(_: &SharedUri) -> ImageSource {
    ImageSource::Custom(Arc::new(|_, _| {
        Some(Err(ImageCacheError::Asset("画像は読み込みません".into())))
    }))
}

/// Whether a click on a link to `url` opens it in the default browser: a primary or middle
/// click on an `http` or `https` URL. Other schemes could open local files or applications.
pub fn opens_link(url: &str, event: &ClickEvent) -> bool {
    let ClickEvent::Mouse(click) = event else {
        return false;
    };
    // A Control-click is the secondary click on macOS.
    let secondary = click.up.button == MouseButton::Left && click.up.modifiers.control;
    matches!(click.up.button, MouseButton::Left | MouseButton::Middle)
        && !secondary
        && is_web_url(url)
}

fn is_web_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && !rest.is_empty()
}

/// The prepared sources of the descriptions and Notes on screen, so a frame does not parse
/// them again and the text view gets the same string back. What the last frame did not show
/// is forgotten.
#[derive(Default)]
pub struct Prepared(RefCell<Frames>);

#[derive(Default)]
struct Frames {
    current: HashMap<String, SharedString>,
    last: HashMap<String, SharedString>,
}

impl Prepared {
    /// Starts a frame: what the frame before did not use is dropped.
    pub fn next_frame(&self) {
        let mut frames = self.0.borrow_mut();
        frames.last = std::mem::take(&mut frames.current);
    }

    pub fn get(&self, source: &str) -> SharedString {
        let mut frames = self.0.borrow_mut();
        if let Some(prepared) = frames.current.get(source) {
            return prepared.clone();
        }
        let (source, prepared) = frames
            .last
            .remove_entry(source)
            .unwrap_or_else(|| (source.to_string(), prepare(source).into()));
        frames.current.insert(source, prepared.clone());
        prepared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, MouseClickEvent, MouseDownEvent, MouseUpEvent, point, px};

    /// Every node of the tree of `source` as the text view parses it.
    fn nodes(source: &str) -> Vec<Node> {
        fn visit(node: &Node, found: &mut Vec<Node>) {
            found.push(node.clone());
            for child in node.children().into_iter().flatten() {
                visit(child, found);
            }
        }
        let mut found = Vec::new();
        visit(&markdown::to_mdast(source, &options()).unwrap(), &mut found);
        found
    }

    fn breaks(source: &str) -> usize {
        nodes(source)
            .iter()
            .filter(|node| matches!(node, Node::Break(_)))
            .count()
    }

    #[test]
    fn a_line_ending_in_a_paragraph_breaks_the_line() {
        let source = "一行目\n二行目 **強調\nの中** [リンク\nの中](https://example.com)\r\n最後";
        let prepared = prepare(source);
        assert_eq!(
            prepared,
            "一行目  \n二行目 **強調  \nの中** [リンク  \nの中](https://example.com)  \r\n最後"
        );
        assert_eq!(breaks(&prepared), 4);
        assert_eq!(prepare("CR だけの\r改行"), "CR だけの  \r改行");
        // Inside quotes and lists too.
        assert_eq!(prepare("> 引用\n> の続き"), "> 引用  \n> の続き");
        assert_eq!(prepare("- 項目\n  の続き"), "- 項目  \n  の続き");
        // White space before the ending gives way to the break.
        assert_eq!(prepare("a\t\nb \nc"), "a  \nb  \nc");
        assert_eq!(breaks(&prepare("a\t\nb \nc")), 2);
        // A hard break already there and paragraphs apart stay.
        assert_eq!(prepare("a  \nb\\\nc\n\nd"), "a  \nb\\\nc\n\nd");
    }

    #[test]
    fn blocks_other_than_paragraphs_keep_their_line_endings() {
        let unchanged = [
            "```\nfenced\ncode\n```",
            "    indented\n    code",
            "<div>\nHTML\nblock\n</div>",
            "| a | b |\n| - | - |\n| 1 | 2 |\n| 3 | 4 |",
            "Setext\nheading\n===",
            "# ATX\n\n## heading",
            "$$\nmath\nflow\n$$",
        ];
        for source in unchanged {
            assert_eq!(prepare(source), source, "{source}");
        }
        // Next to a paragraph, only the paragraph changes.
        assert_eq!(
            prepare("段落\nの続き\n\n```\nコード\nの続き\n```\n\n    字下げ\n    コード"),
            "段落  \nの続き\n\n```\nコード\nの続き\n```\n\n    字下げ\n    コード"
        );
    }

    #[test]
    fn the_tree_after_the_rewrite_keeps_every_block() {
        let source = "# 見出し\n\n本文の\n段落\n\n- 箇条\n- 書き\n\n> 引用\n\n```rust\nlet a = 1;\n```\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n*強調* と `コード`";
        let kinds = |source: &str| -> Vec<String> {
            nodes(source)
                .iter()
                .filter(|node| !matches!(node, Node::Break(_) | Node::Text(_)))
                .map(|node| format!("{node:?}").split(' ').next().unwrap().to_string())
                .collect()
        };
        assert_eq!(kinds(&prepare(source)), kinds(source));
    }

    #[test]
    fn a_code_block_holds_any_text_as_written() {
        for text in ["test -f a", "```\n*b*\n```", "a\n\n  b ``` c"] {
            let nodes = nodes(&code_block(text));
            let codes: Vec<_> = nodes
                .iter()
                .filter_map(|node| match node {
                    Node::Code(code) => Some(code.value.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(codes, [text], "{text}");
        }
    }

    #[test]
    fn line_endings_in_math_follow_how_it_renders() {
        // Math nothing renders as math shows the Markdown it holds, line endings and all.
        assert_eq!(prepare("a $x\n*y*$ b"), "a $x  \n*y*$ b");
        // Math kept as written keeps them as written.
        assert_eq!(prepare("a $x\ny$ b"), "a $x\ny$ b");
    }

    /// The text an inline image of `source`, the first node of its first paragraph, shows.
    fn alt_of(source: &str) -> Option<String> {
        let node = nodes(source).into_iter().find(|node| {
            matches!(
                node,
                Node::Image(_) | Node::ImageReference(_) | Node::Html(_)
            )
        })?;
        alt_text(&node)
    }

    #[test]
    fn images_show_their_alternative_text() {
        let cases = [
            ("前 ![図_1 *#2*](https://example.com/a.png) 後", "図_1 #2"),
            ("![](a.png)", IMAGE_WITHOUT_ALT),
            ("![ref][r]\n\n[r]: https://example.com/b.png", "ref"),
            (
                r#"前 <img src="https://example.com/a.png" alt="A &amp; *B*"> 後"#,
                "A & *B*",
            ),
            ("前 <IMAGE src=a.png alt='A'/> 後", "A"),
            ("前 <img src=a.png> 後", IMAGE_WITHOUT_ALT),
            (r#"前 <img alt="first" alt="second"> 後"#, "first"),
            // In a quote, the tag is read without the quote's markers.
            ("> 前 <img\n> src=x.png alt=y> 後", "y"),
        ];
        for (source, shown) in cases {
            assert_eq!(alt_of(source).as_deref(), Some(shown), "{source}");
        }
        // Other HTML is not an image.
        for source in ["前 <b> 後", "前 <imgx> 後", "前 <!-- <img alt=x> --> 後"] {
            assert_eq!(alt_of(source), None, "{source}");
        }
    }

    #[test]
    fn an_html_block_opened_by_an_image_shows_its_alternative_text() {
        assert_eq!(
            prepare("<img src=a.png alt=画面>\n上の図の *とおり*"),
            "画面  \n上の図の *とおり*"
        );
        // The images after it are then in a paragraph, where the plugin shows them.
        assert_eq!(
            prepare("> <img src=a.png alt='a.b'>\n> <img src=b.png>"),
            "> a&#46;b  \n> <img src=b.png>"
        );
        assert_eq!(prepare("<img src=a.png>"), IMAGE_WITHOUT_ALT);
        assert_eq!(prepare("   <img src=a.png alt=図>"), "   図");
        assert_eq!(prepare("  <id>"), "  \\<id>");
        assert_eq!(prepare("<id>\rfoo"), "\\<id>  \rfoo");
        assert_eq!(prepare("<form>\n*x*\n</form>"), "<form>\n*x*\n</form>");
        // Other HTML blocks stay HTML.
        for source in [
            "<div>\n<img src=a.png>\n</div>",
            "<p><img src=a.png></p>",
            "<!-- <img> -->",
        ] {
            assert_eq!(prepare(source), source);
        }
    }

    #[test]
    fn a_tag_that_would_vanish_shows_as_written() {
        assert_eq!(prepare("axon show <id> で確認"), "axon show \\<id> で確認");
        assert_eq!(prepare("# <Entity ID> を開く"), "# \\<Entity ID> を開く");
        assert_eq!(prepare("<group>\n続き"), "\\<group>  \n続き");
        assert_eq!(
            prepare("--label <label> と </label>"),
            "--label \\<label> と \\</label>"
        );
        // HTML an author means stays HTML, and so do comments.
        for source in [
            "<b>太字</b> と <a href=x>リンク</a><br>",
            "前 <!-- メモ --> 後",
            "<details>\n<summary>詳細</summary>\n</details>",
            "`<id>` と <https://example.com>",
        ] {
            assert_eq!(prepare(source), source);
        }
        // In math, escaping would turn the math back into its literal.
        assert_eq!(prepare("a $<id> *b*$ c"), "a $<id> *b*$ c");
    }

    fn click(button: MouseButton) -> ClickEvent {
        let position = point(px(0.), px(0.));
        ClickEvent::Mouse(MouseClickEvent {
            down: MouseDownEvent {
                button,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            },
            up: MouseUpEvent {
                button,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            },
        })
    }

    #[test]
    fn only_web_links_open_and_only_with_the_left_or_middle_button() {
        for url in [
            "https://example.com/a",
            "http://example.com",
            "HTTPS://example.com",
        ] {
            assert!(opens_link(url, &click(MouseButton::Left)), "{url}");
            assert!(opens_link(url, &click(MouseButton::Middle)), "{url}");
            assert!(!opens_link(url, &click(MouseButton::Right)), "{url}");
            let ClickEvent::Mouse(mut control) = click(MouseButton::Left) else {
                unreachable!()
            };
            control.up.modifiers.control = true;
            assert!(!opens_link(url, &ClickEvent::Mouse(control)), "{url}");
        }
        for url in [
            "file:///etc/passwd",
            "mailto:a@example.com",
            "javascript:alert(1)",
            "me.gin0606.axon://open?root=/tmp",
            "ftp://example.com",
            "/relative/path",
            "example.com",
            "https://",
            "https:example.com",
        ] {
            assert!(!opens_link(url, &click(MouseButton::Left)), "{url}");
        }
    }
}
