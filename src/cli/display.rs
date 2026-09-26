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
    if terminal && std::env::var_os("NO_COLOR").is_none() {
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

pub fn cli_color() -> clap::ColorChoice {
    if std::env::var_os("NO_COLOR").is_some() {
        clap::ColorChoice::Never
    } else {
        clap::ColorChoice::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn human_text_preserves_newlines_and_unicode_but_escapes_terminal_controls() {
        assert_eq!(
            human_text("normal 日本語\n\t\r\0\u{1b}\u{7f}\u{85}\u{9f}"),
            "normal 日本語\n\\t\\r\\x00\\x1b\\x7f\\x85\\x9f"
        );
    }
}
