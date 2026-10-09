//! The Markdown of descriptions and Notes as the detail pane renders it.
//!
//! Records carry no mark of their format, so every description and Note is read as Markdown.
//! Before rendering, the source is rewritten on the syntax tree's positions: a line ending
//! inside a paragraph is made a hard break, as a GitHub comment shows it, so text that the CLI
//! or an agent wrapped by hand reads as written; a tag the text view would drop without a trace,
//! such as the placeholder `<id>`, is escaped so it shows as written; and an image tag that
//! opens an HTML block, or stands inside one, is replaced by the image's alternative text.
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
        // The blocks no longer change, so the images left in one stay in HTML.
        (Node::Html(html), Pass::Inline) if place == Place::Block => {
            for (range, replacement) in block_images(&html.value, &source[span]) {
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
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
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

/// The elements whose content an HTML parser reads as text, so no tag stands in it.
const TEXT_ELEMENTS: &[&str] = &[
    "iframe",
    "noembed",
    "noframes",
    "noscript",
    "plaintext",
    "script",
    "style",
    "textarea",
    "title",
    "xmp",
];

/// The edits that replace each image tag in the HTML block `value` with its alternative text,
/// as positions in `written`, the block as written in the source. `value` is `written` without
/// the markers of the quotes and lists it stands in; a block where it is not, line by line,
/// gets no edits.
fn block_images(value: &str, written: &str) -> Vec<(Range<usize>, String)> {
    let images = html_images(value);
    if images.is_empty() {
        return Vec::new();
    }
    let value_lines = lines(value);
    let written_lines = lines(written);
    // In a list item, the block may hold blank lines after it that its text leaves out.
    if written_lines.len() < value_lines.len()
        || written_lines[value_lines.len()..]
            .iter()
            .any(|line| !written[line.clone()].trim().is_empty())
    {
        return Vec::new();
    }
    // Where each line of `value` starts in `written`.
    let mut starts = Vec::with_capacity(value_lines.len());
    for (value_line, written_line) in value_lines.iter().zip(&written_lines) {
        let value_line = &value[value_line.clone()];
        let line = &written[written_line.clone()];
        if !line.ends_with(value_line) {
            return Vec::new();
        }
        starts.push(written_line.end - value_line.len());
    }
    let position = |at: usize| {
        let line = value_lines.partition_point(|line| line.start <= at) - 1;
        starts[line] + at - value_lines[line].start
    };
    // An image that opens the block is left to `block_start`: replaced here, the block would
    // turn into a paragraph after the paragraphs were rewritten.
    let opening = value.len() - value.trim_start().len();
    images
        .into_iter()
        .filter(|(range, _)| range.start != opening)
        .map(|(range, alt)| (position(range.start)..position(range.end), alt))
        .collect()
}

/// The range of each line of `text`, without its line ending.
fn lines(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                lines.push(start..at);
                start = at + 1;
            }
            b'\r' => {
                lines.push(start..at);
                if bytes.get(at + 1) == Some(&b'\n') {
                    at += 1;
                }
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    lines.push(start..text.len());
    lines
}

/// Each image tag in the HTML block `html`, as an HTML parser reads it, and its alternative
/// text written as HTML. Tags in comments, in attribute values and in elements read as text
/// are no images.
fn html_images(html: &str) -> Vec<(Range<usize>, String)> {
    let mut images = Vec::new();
    if find_ignoring_case(html, "<im", 0).is_none() {
        return images;
    }
    let block_ends = block_ends(html);
    let mut at = 0;
    while let Some(found) = html[at..].find('<') {
        at += found;
        let rest = &html[at..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            at = at + 4 + comment_end(comment);
        } else if rest.starts_with("<!")
            || rest.starts_with("<?")
            || (rest.starts_with("</") && !rest[2..].starts_with(|c: char| c.is_ascii_alphabetic()))
        {
            // A bogus comment, up to the next `>`.
            at = rest[1..]
                .find('>')
                .map_or(html.len(), |end| at + 1 + end + 1);
        } else if let Some((tag_end, alt)) = image_tag(html, at) {
            let tag = &html[at..tag_end];
            images.push((at..tag_end, alt_html(&alt, &ends_in(tag, block_ends))));
            at = tag_end;
        } else if rest
            .as_bytes()
            .get(1 + usize::from(rest.starts_with("</")))
            .is_some_and(u8::is_ascii_alphabetic)
        {
            let closing = rest.starts_with("</");
            // The name runs on as the parser reads it, as in `title.x`.
            let name_start = at + 1 + usize::from(closing);
            let mut name_end = name_start;
            while !ends_name(html, name_end) {
                name_end += 1;
            }
            let name = &html[name_start..name_end];
            let Some((end, _)) = tag_attributes(html, name_end) else {
                break;
            };
            at = end;
            if !closing && is_one_of(name, TEXT_ELEMENTS) {
                at = text_element_end(html, at, name);
            }
        } else {
            at += 1;
        }
    }
    images
}

/// The length of the comment whose text starts `comment`, up to and with what ends it, as an
/// HTML parser reads it: `<!-->` and `<!--->` are empty, `--!>` ends one as `-->` does, and
/// one never ended runs to the end.
fn comment_end(comment: &str) -> usize {
    if comment.starts_with('>') {
        return 1;
    }
    if comment.starts_with("->") {
        return 2;
    }
    let mut at = 0;
    while let Some(found) = comment[at..].find("--") {
        let dashes = at + found + 2;
        let after = &comment[dashes..];
        if after.starts_with('>') {
            return dashes + 1;
        }
        if after.starts_with("!>") {
            return dashes + 2;
        }
        at = at + found + 1;
    }
    comment.len()
}

/// Where the closing tag of the element `name` read as text, whose content starts at `at`,
/// begins, or the end of `html` when nothing closes it.
fn text_element_end(html: &str, mut at: usize, name: &str) -> usize {
    if name.eq_ignore_ascii_case("plaintext") {
        return html.len();
    }
    let close = format!("</{name}");
    while let Some(found) = find_ignoring_case(html, &close, at) {
        if ends_name(html, found + close.len()) {
            return found;
        }
        at = found + close.len();
    }
    html.len()
}

/// Whether a tag name in `html` ends at `at`, as an HTML parser reads it.
fn ends_name(html: &str, at: usize) -> bool {
    html.as_bytes()
        .get(at)
        .is_none_or(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>'))
}

/// Where `needle`, in ASCII letters of either case, first stands in `haystack` from `from`.
fn find_ignoring_case(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let needle = needle.as_bytes();
    haystack.as_bytes()[from..]
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|at| from + at)
}

/// The texts that end the HTML block `html` on the line holding them, when it is one of
/// CommonMark's kinds that do not end at a blank line, as its first line tells. The `>` that
/// ends a declaration is left out, as the text that replaces a tag holds one too.
fn block_ends(html: &str) -> &'static [&'static str] {
    let html = html.trim_start();
    let starts = |prefix: &str| {
        html.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    let raw = ["pre", "script", "style", "textarea"]
        .into_iter()
        .any(|name| {
            starts(&format!("<{name}"))
                && html
                    .as_bytes()
                    .get(name.len() + 1)
                    .is_none_or(|c| c.is_ascii_whitespace() || *c == b'>')
        });
    if raw {
        &["</pre>", "</script>", "</style>", "</textarea>"]
    } else if starts("<!--") {
        &["-->"]
    } else if starts("<?") {
        &["?>"]
    } else if starts("<![CDATA[") {
        &["]]>"]
    } else {
        &[]
    }
}

/// Each of `ends` that `tag` holds, as written.
fn ends_in<'a>(tag: &'a str, ends: &[&str]) -> Vec<&'a str> {
    let mut found_ends = Vec::new();
    for end in ends {
        let mut at = 0;
        while let Some(found) = find_ignoring_case(tag, end, at) {
            found_ends.push(&tag[found..found + end.len()]);
            at = found + end.len();
        }
    }
    found_ends
}

/// The alternative text `raw`, an attribute value as written, as HTML on one line, followed by
/// comments that hold the `ends` of the HTML block that the tag it replaces held, so that the
/// block ends where it did. The text view drops comments.
fn alt_html(raw: &str, ends: &[&str]) -> String {
    let text = shown(&decode_entities(raw))
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    // An element of its own, so the text does not join what stands around it into markup.
    let mut html = format!("<span>{text}</span>");
    for end in ends {
        // `<!---->` is an empty comment that holds `-->`.
        if *end == "-->" {
            html.push_str("<!---->");
        } else {
            html.push_str(&format!("<!--{end}-->"));
        }
    }
    html
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
            && ends_name(html, end)
    })?;
    tag_attributes(html, start + 1 + name.len())
}

/// The end of the tag whose attributes start at `at` in `html`, and the raw value of its first
/// `alt` attribute (empty without one), or `None` when the tag never closes.
fn tag_attributes(html: &str, mut at: usize) -> Option<(usize, String)> {
    let bytes = html.as_bytes();
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
        // An `=` where a name starts is part of the name.
        if bytes[at] == b'=' {
            at += 1;
        }
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
            // An attribute without a value has an empty one.
            if name.eq_ignore_ascii_case("alt") && alt.is_none() {
                alt = Some(String::new());
            }
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

/// The names of the character references that an HTML parser reads without their `;`.
const LEGACY_REFERENCES: &[&str] = &[
    "AElig", "AMP", "Aacute", "Acirc", "Agrave", "Aring", "Atilde", "Auml", "COPY", "Ccedil",
    "ETH", "Eacute", "Ecirc", "Egrave", "Euml", "GT", "Iacute", "Icirc", "Igrave", "Iuml", "LT",
    "Ntilde", "Oacute", "Ocirc", "Ograve", "Oslash", "Otilde", "Ouml", "QUOT", "REG", "THORN",
    "Uacute", "Ucirc", "Ugrave", "Uuml", "Yacute", "aacute", "acirc", "acute", "aelig", "agrave",
    "amp", "aring", "atilde", "auml", "brvbar", "ccedil", "cedil", "cent", "copy", "curren", "deg",
    "divide", "eacute", "ecirc", "egrave", "eth", "euml", "frac12", "frac14", "frac34", "gt",
    "iacute", "icirc", "iexcl", "igrave", "iquest", "iuml", "laquo", "lt", "macr", "micro",
    "middot", "nbsp", "not", "ntilde", "oacute", "ocirc", "ograve", "ordf", "ordm", "oslash",
    "otilde", "ouml", "para", "plusmn", "pound", "quot", "raquo", "reg", "sect", "shy", "sup1",
    "sup2", "sup3", "szlig", "thorn", "times", "uacute", "ucirc", "ugrave", "uml", "uuml",
    "yacute", "yen", "yuml",
];

/// The attribute value `raw` with its character references decoded as an HTML parser decodes
/// them in an attribute.
fn decode_entities(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        match reference(rest) {
            Some((text, length)) => {
                decoded.push_str(&text);
                rest = &rest[length..];
            }
            None => decoded.push('&'),
        }
    }
    decoded.push_str(rest);
    decoded
}

/// The character of the numeric character reference `code`, as an HTML parser reads it.
fn numeric_reference(code: u32) -> char {
    // The C1 controls are read as Windows-1252.
    const WINDOWS_1252: [u32; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    let code = match code {
        0x80..=0x9f => WINDOWS_1252[(code - 0x80) as usize],
        _ => code,
    };
    char::from_u32(code)
        .filter(|c| *c != '\0')
        .unwrap_or('\u{fffd}')
}

/// The text of the character reference that `rest`, what follows an `&` in an attribute
/// value, starts with, and the length it takes.
fn reference(rest: &str) -> Option<(String, usize)> {
    if let Some(number) = rest.strip_prefix('#') {
        let (radix, digits) = match number.strip_prefix(['x', 'X']) {
            Some(hex) => (16, hex),
            None => (10, number),
        };
        let length = digits
            .find(|c: char| !c.is_digit(radix))
            .unwrap_or(digits.len());
        if length == 0 {
            return None;
        }
        let code = u32::from_str_radix(&digits[..length], radix).unwrap_or(u32::MAX);
        let text = numeric_reference(code).to_string();
        let prefix = rest.len() - digits.len();
        let semicolon = usize::from(digits[length..].starts_with(';'));
        return Some((text, prefix + length + semicolon));
    }
    let name_length = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    let name = &rest[..name_length];
    if rest[name_length..].starts_with(';')
        && let Some(text) = markdown::decode_named(name, true)
    {
        return Some((text, name_length + 1));
    }
    // Without its `;`, a reference is read only when nothing that could go on with a name
    // follows it.
    let legacy = LEGACY_REFERENCES
        .iter()
        .filter(|legacy| name.starts_with(**legacy))
        .max_by_key(|legacy| legacy.len())?;
    let next = rest[legacy.len()..].chars().next();
    if next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '=') {
        return None;
    }
    Some((markdown::decode_named(legacy, true)?, legacy.len()))
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
            (
                r#"前 <img alt="&mdash;&notit; &copy=1"> 後"#,
                "—&notit; &copy=1",
            ),
            (r#"前 <img alt="&Tab;"> 後"#, IMAGE_WITHOUT_ALT),
            (
                r#"前 <img alt="It&#146;s &#x96;"> 後"#,
                "It\u{2019}s \u{2013}",
            ),
            (r#"前 <img alt="first" alt="second"> 後"#, "first"),
            (r#"前 <img alt alt="second"> 後"#, IMAGE_WITHOUT_ALT),
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
        assert_eq!(prepare("<!-- <img> -->"), "<!-- <img> -->");
    }

    #[test]
    fn images_inside_other_html_blocks_show_their_alternative_text() {
        let cases = [
            ("<p><img src=a.png alt=図></p>", "<p><span>図</span></p>"),
            (
                "<div>\n<img src=a.png>\n*x* <IMAGE alt='a < b &amp; c'/>\n</div>",
                "<div>\n<span>（画像）</span>\n*x* <span>a &lt; b &amp; c</span>\n</div>",
            ),
            // In a quote or a list, the tag may run over lines.
            (
                "> <div>\n> <img\n> src=a.png alt=\"x\n> y\">\n> </div>",
                "> <div>\n> <span>x y</span>\n> </div>",
            ),
            (
                "- <div>\n  <img src=a.png alt=z></div>",
                "- <div>\n  <span>z</span></div>",
            ),
            // Character references are read as in an attribute, not as in text.
            (
                "<p><img alt=\"a &mdash; b ?x&copy=1 &notit; &copy 2024 &#169\"></p>",
                "<p><span>a — b ?x&amp;copy=1 &amp;notit; © 2024 ©</span></p>",
            ),
            // The text does not join a `<` before it into a tag.
            ("<p>a <<img alt=b>c</p>", "<p>a <<span>b</span>c</p>"),
            // Comments end where the HTML parser ends them.
            (
                "<div><!--> <img alt=x></div>",
                "<div><!--> <span>x</span></div>",
            ),
            (
                "<div><!---> <img alt=x></div>",
                "<div><!---> <span>x</span></div>",
            ),
            (
                "<div><!-- a --!> <img alt=x></div>",
                "<div><!-- a --!> <span>x</span></div>",
            ),
            // So does a bogus comment, from `</` and no name to the next `>`.
            (
                "<div>I </3 <img alt=x> <img alt=y></div>",
                "<div>I </3 <img alt=x> <span>y</span></div>",
            ),
            (
                "<div><textarea></textarea\n><img alt=x></div>",
                "<div><textarea></textarea\n><span>x</span></div>",
            ),
            (
                "<div><title.x><img alt=x></div>",
                "<div><title.x><span>x</span></div>",
            ),
            (
                "<pre>\n<img alt=\"</pretty>\">\n</pre>",
                "<pre>\n<span>&lt;/pretty&gt;</span>\n</pre>",
            ),
            (
                "<!DOCTYPE html><img alt=x>",
                "<!DOCTYPE html><span>x</span>",
            ),
            // A tag name runs to a space, `/` or `>`.
            (
                "<div>\n<b.='><img src=x alt=y>'\n</div>",
                "<div>\n<b.='><span>y</span>'\n</div>",
            ),
            // An `=` where an attribute name starts is part of the name.
            (
                "<div>\n<p class=a =\"b>\n<img alt=図>\n</div>",
                "<div>\n<p class=a =\"b>\n<span>図</span>\n</div>",
            ),
            // An end of an HTML block that the tag held stays on its line, in a comment.
            (
                "<pre>\n<img alt=\"</pre>\">\n**x**",
                "<pre>\n<span>&lt;/pre&gt;</span><!--</pre>-->\n**x**",
            ),
            (
                "<!-- a --> <img alt=\"b-->\">",
                "<!-- a --> <span>b--&gt;</span><!---->",
            ),
            // In a block that ends at a blank line, it is only text.
            (
                "<div><img alt=\"A --> B </pre>\"></div>",
                "<div><span>A --&gt; B &lt;/pre&gt;</span></div>",
            ),
            // In a list item, the blank lines inside the block are in the item.
            (
                "- <pre>\n  <img src=a.png alt=図>\n\n  </pre>",
                "- <pre>\n  <span>図</span>\n\n  </pre>",
            ),
            (">\t<img alt=x>", ">\tx"),
        ];
        for (source, prepared) in cases {
            assert_eq!(prepare(source), prepared, "{source}");
        }
        let strong = |source: &str| {
            nodes(&prepare(source))
                .iter()
                .any(|node| matches!(node, Node::Strong(_)))
        };
        assert!(strong("<pre>\n<img alt=\"</pre>\">\n**x**"));
        // An end in a tag running over lines ends the block on the tag's last line, which the
        // text replacing it joins.
        assert_eq!(
            prepare("<pre>\n<img\nalt=\"</pre>\">\n**x**"),
            "<pre>\n<span>&lt;/pre&gt;</span><!--</pre>-->\n**x**"
        );
        assert!(strong("<pre>\n<img\nalt=\"</pre>\">\n**x**"));
        // Tags in comments, attribute values and text elements are no images.
        for source in [
            "<div>\n<!-- <img alt=x> -->\n</div>",
            "<div>\n<!-- <img alt=x>\n</div>",
            "<div title=\"<img alt=x>\">\n</div>",
            "<div>\n<textarea><img alt=x></textarea>\n</div>",
            "<div><textarea>a </textareax><img alt=x></textarea></div>",
            "<div>\n<plaintext></plaintext><img alt=x>\n</div>",
            "<script>\nlet a = '<img alt=x>';\n</script>",
            "<div>\n<imgx alt=x>\n</div>",
            "<div><xmp><img alt=x></xmp></div>",
        ] {
            assert_eq!(prepare(source), source, "{source}");
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

#[cfg(test)]
mod rendering {
    use super::prepare;
    use gpui_kit::{AppContext, TestAppContext, base::text::TextViewState};

    /// The text the text view shows for `source` once prepared.
    fn rendered(source: &str, cx: &mut TestAppContext) -> String {
        let prepared = prepare(source);
        let state = cx.new(|cx| TextViewState::markdown(&prepared, cx));
        cx.run_until_parked();
        cx.read(|cx| state.read(cx).rendered_text().as_str().to_string())
    }

    #[gpui_kit::test]
    fn images_in_html_blocks_render_as_their_alternative_text(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        for (source, shown) in [
            ("<p><img src=a.png alt=図></p>", "図"),
            ("<div>\n前 <img src=a.png> 後\n</div>", "前 （画像） 後"),
            (
                "<p><img src=a.png alt=\"入力 --> 出力\"></p>",
                "入力 --> 出力",
            ),
            ("<pre>\n<img src=a.png alt=\"</pre>\">\n</pre>", "</pre>"),
        ] {
            assert_eq!(rendered(source, cx).trim(), shown, "{source}");
        }
    }
}
