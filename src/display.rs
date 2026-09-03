use chrono::{DateTime, FixedOffset, Local, Utc};

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
}
