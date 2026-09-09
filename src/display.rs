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
