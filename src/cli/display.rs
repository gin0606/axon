use chrono::{DateTime, FixedOffset, Local, Utc};
use std::fmt::Write;

/// Render untrusted text without allowing terminal control sequences to execute.
pub fn human_text(value: impl std::fmt::Display) -> String {
    let value = value.to_string();
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' => rendered.push('\n'),
            '\t' => rendered.push_str("\\t"),
            '\r' => rendered.push_str("\\r"),
            '\u{00}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                write!(rendered, "\\x{:02x}", character as u32).unwrap();
            }
            _ => rendered.push(character),
        }
    }
    rendered
}

pub fn timestamp(timestamp: &DateTime<Utc>) -> String {
    let local = timestamp.with_timezone(&Local);
    timestamp_at_offset(timestamp, *local.offset())
}

fn timestamp_at_offset(timestamp: &DateTime<Utc>, offset: FixedOffset) -> String {
    timestamp
        .with_timezone(&offset)
        .format("%Y-%m-%d %H:%M %:z")
        .to_string()
}

use clap::builder::styling::{AnsiColor, Color, Style, Styles};
use std::io::IsTerminal;
use std::sync::OnceLock;

static COLOR_DISABLED: OnceLock<bool> = OnceLock::new();

pub fn configure_color(no_color: bool) {
    let disabled = no_color
        || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
        || std::env::var_os("TERM").is_some_and(|value| value == "dumb");
    let _ = COLOR_DISABLED.set(disabled);
}

fn color_enabled(terminal: bool) -> bool {
    terminal && !COLOR_DISABLED.get().copied().unwrap_or(false)
}

const HEADING: Style = Style::new().bold();
const ID: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
const POSITIVE: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
const WAITING: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
const DECISION: Style = WAITING.bold();
const FAILURE: Style = Style::new()
    .bold()
    .fg_color(Some(Color::Ansi(AnsiColor::Red)));
const MUTED: Style = Style::new().dimmed();

fn paint(style: Style, text: impl std::fmt::Display, terminal: bool) -> String {
    if color_enabled(terminal) {
        format!("{style}{text}{style:#}")
    } else {
        text.to_string()
    }
}
/// A value printed inside one output line. A newline in it must not start a line of its own,
/// or stored text could imitate records and section headings.
pub fn line(value: impl std::fmt::Display) -> String {
    human_text(value).replace('\n', "\\n")
}
/// Multi-line stored text printed among structural lines. Structural lines start at column
/// zero and this text never does, so it cannot imitate a record heading or a section.
pub fn block(value: impl std::fmt::Display) -> String {
    human_text(value)
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("  {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn identity(text: impl std::fmt::Display) -> String {
    paint(ID, line(text), std::io::stdout().is_terminal())
}
pub fn muted(text: impl std::fmt::Display) -> String {
    paint(MUTED, text, std::io::stdout().is_terminal())
}
pub fn heading(text: impl std::fmt::Display) -> String {
    paint(HEADING, text, std::io::stdout().is_terminal())
}
pub fn positive(text: impl std::fmt::Display) -> String {
    paint(POSITIVE, text, std::io::stdout().is_terminal())
}
pub fn error_label() -> String {
    paint(FAILURE, "Error:", std::io::stderr().is_terminal())
}
pub fn situation(text: &str) -> String {
    let style = match text {
        "Undecided" => DECISION,
        "Ready" | "Confirmable" => POSITIVE,
        "Blocked" | "Unsurfaced" | "InProgress+Blocked" | "Empty" => WAITING,
        "InProgress" | "Started  InProgress" => ID,
        _ => MUTED,
    };
    paint(style, text, std::io::stdout().is_terminal())
}
/// Stdout as a terminal, which lines up rows in columns independently of color settings,
/// so output that programs read keeps one row per line.
pub struct Terminal {
    /// The terminal's width, read once; `None` when it cannot be read.
    pub columns: Option<usize>,
}
pub fn terminal() -> Option<Terminal> {
    use std::os::fd::AsRawFd;
    let stdout = std::io::stdout();
    if !stdout.is_terminal() {
        return None;
    }
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let read = unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == 0;
    Some(Terminal {
        columns: (read && size.ws_col > 0).then_some(usize::from(size.ws_col)),
    })
}
/// The columns `text` occupies in a terminal. Measure text before decoration.
pub fn width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}
/// Split `text` into pieces at most `columns` wide, as a terminal wraps it: by character, not
/// by word. A grapheme (a character with its combining marks, modifiers or joined emoji)
/// stays whole, so a wide character never splits across pieces.
pub fn wrap(text: &str, columns: usize) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut used = 0;
    for cluster in text.graphemes(true) {
        let cluster_width = width(cluster);
        if used > 0 && used + cluster_width > columns {
            pieces.push(std::mem::take(&mut piece));
            used = 0;
        }
        piece.push_str(cluster);
        used += cluster_width;
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}
pub fn cli_styles() -> Styles {
    Styles::styled()
        .header(HEADING)
        .error(FAILURE)
        .usage(HEADING)
        .literal(HEADING)
        .placeholder(Style::new())
        .valid(POSITIVE)
        .invalid(DECISION)
        .context(MUTED)
        .context_value(Style::new())
}

pub fn cli_color(stderr: bool) -> clap::ColorChoice {
    let terminal = if stderr {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    if color_enabled(terminal) {
        clap::ColorChoice::Always
    } else {
        clap::ColorChoice::Never
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn timestamp_uses_the_given_offset_and_numeric_offset_format() {
        let timestamp = "2026-09-01T17:55:00Z".parse::<DateTime<Utc>>().unwrap();
        let offset = FixedOffset::east_opt(9 * 60 * 60).unwrap();

        assert_eq!(
            timestamp_at_offset(&timestamp, offset),
            "2026-09-02 02:55 +09:00"
        );
    }

    #[test]
    fn wrap_splits_by_display_width_without_breaking_characters() {
        assert_eq!(wrap("abcdef", 4), ["abcd", "ef"]);
        // A wide character that does not fit moves whole to the next piece.
        assert_eq!(wrap("abc日本", 4), ["abc", "日本"]);
        // Combining marks, joined emoji and flag pairs stay with the character they follow.
        assert_eq!(wrap("abce\u{301}f", 4), ["abce\u{301}", "f"]);
        assert_eq!(
            wrap("ab\u{1f469}\u{200d}\u{1f4bb}c", 3),
            ["ab", "\u{1f469}\u{200d}\u{1f4bb}c"]
        );
        assert_eq!(
            wrap("ab\u{1f44d}\u{1f3fd}c", 3),
            ["ab", "\u{1f44d}\u{1f3fd}c"]
        );
        assert_eq!(
            wrap("a\u{1f1ef}\u{1f1f5}\u{1f1fa}\u{1f1f8}", 3),
            ["a\u{1f1ef}\u{1f1f5}", "\u{1f1fa}\u{1f1f8}"]
        );
        assert!(wrap("", 4).is_empty());
    }

    proptest! {
        #[test]
        fn generated_text_escapes_controls_without_losing_other_characters(
            characters in prop::collection::vec(any::<char>(), 0..80)
        ) {
            let input = format!("normal 日本語\n\t\r\0\u{1b}\u{7f}\u{85}\u{9f}{}", characters.iter().collect::<String>());
            let prefix = "normal 日本語\n\\t\\r\\x00\\x1b\\x7f\\x85\\x9f";
            let expected: String = characters.iter().map(|c| match c {
                '\n' => "\n".to_string(),
                '\t' => "\\t".to_string(),
                '\r' => "\\r".to_string(),
                '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => format!("\\x{:02x}", *c as u32),
                _ => c.to_string(),
            }).collect();
            let expected = format!("{prefix}{expected}");
            prop_assert_eq!(human_text(&input), expected.as_str());
            prop_assert_eq!(line(&input), expected.replace('\n', "\\n"));
            let block_expected = expected.split('\n').map(|line| {
                if line.is_empty() { String::new() } else { format!("  {line}") }
            }).collect::<Vec<_>>().join("\n");
            prop_assert_eq!(block(&input), block_expected);
        }
    }
}
