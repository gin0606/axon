//! Optional provenance from an allowlist of environment variables, independent of Axon's core.
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub struct Recorder {
    pub actor: String,
    pub data: BTreeMap<String, String>,
}

/// Reads only inherited environment values; never searches logs or checks session liveness.
pub fn detect() -> Option<Recorder> {
    detect_with(|key| std::env::var(key).ok())
}

fn detect_with(mut read: impl FnMut(&str) -> Option<String>) -> Option<Recorder> {
    let mut get = |key: &str| read(key).filter(|value| !value.trim().is_empty());
    let explicit = get("AXON_ACTOR");
    let (actor, session) = if let Some(actor) = explicit {
        (actor, get("AXON_SESSION_ID"))
    } else if let Some(session) = get("CODEX_THREAD_ID") {
        ("codex".into(), Some(session))
    } else if get("CODEX_SANDBOX").is_some() {
        ("codex".into(), None)
    } else if get("CLAUDECODE").is_some() || get("CLAUDE_CODE").is_some() {
        ("claude-code".into(), get("CLAUDE_CODE_SESSION_ID"))
    } else if let Some(actor) = get("AI_AGENT") {
        (actor, None)
    } else {
        (get("USER")?, None)
    };
    let mut data = BTreeMap::new();
    if let Some(session) = session {
        data.insert("session_id".into(), session);
    }
    Some(Recorder { actor, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(values: &[(&str, &str)]) -> Option<Recorder> {
        detect_with(|key| {
            values
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).into())
        })
    }

    #[test]
    fn absent_and_empty_information_is_optional() {
        assert_eq!(detect(&[]), None);
        assert_eq!(detect(&[("AXON_ACTOR", " "), ("USER", "")]), None);
        let r = detect(&[("CODEX_SANDBOX", "seatbelt")]).unwrap();
        assert_eq!(r.actor, "codex");
        assert!(r.data.is_empty());
    }

    #[test]
    fn automatic_session_and_precedence() {
        let r = detect(&[
            ("CODEX_THREAD_ID", "thread"),
            ("CLAUDECODE", "1"),
            ("USER", "human"),
        ])
        .unwrap();
        assert_eq!(r.actor, "codex");
        assert_eq!(r.data["session_id"], "thread");
        let r = detect(&[("AXON_ACTOR", "human"), ("CODEX_THREAD_ID", "thread")]).unwrap();
        assert_eq!(r.actor, "human");
        assert!(r.data.is_empty());
        let r = detect(&[("AXON_ACTOR", "custom"), ("AXON_SESSION_ID", "session")]).unwrap();
        assert_eq!(r.data["session_id"], "session");
    }

    #[test]
    fn claude_code_session_needs_claude_code_detection() {
        for key in ["CLAUDECODE", "CLAUDE_CODE"] {
            let r = detect(&[(key, "1"), ("CLAUDE_CODE_SESSION_ID", "session")]).unwrap();
            assert_eq!(r.actor, "claude-code");
            assert_eq!(r.data["session_id"], "session");
        }
        let r = detect(&[("CLAUDECODE", "1"), ("CLAUDE_CODE_SESSION_ID", " ")]).unwrap();
        assert_eq!(r.actor, "claude-code");
        assert!(r.data.is_empty());
        let r = detect(&[("CLAUDE_CODE_SESSION_ID", "session"), ("USER", "human")]).unwrap();
        assert_eq!(r.actor, "human");
        assert!(r.data.is_empty());
        for (key, value, actor, session) in [
            ("AXON_ACTOR", "human", "human", None),
            ("CODEX_THREAD_ID", "thread", "codex", Some("thread")),
        ] {
            let r = detect(&[
                (key, value),
                ("CLAUDECODE", "1"),
                ("CLAUDE_CODE_SESSION_ID", "session"),
            ])
            .unwrap();
            assert_eq!(r.actor, actor);
            assert_eq!(r.data.get("session_id").map(String::as_str), session);
        }
    }

    #[test]
    fn partial_and_generic_sources() {
        for (key, value, expected) in [
            ("CLAUDECODE", "1", "claude-code"),
            ("CLAUDE_CODE", "1", "claude-code"),
            ("AI_AGENT", "other", "other"),
            ("USER", "human", "human"),
        ] {
            let r = detect(&[(key, value), ("SECRET_TOKEN", "never copied")]).unwrap();
            assert_eq!(r.actor, expected);
            assert!(r.data.is_empty());
        }
    }
}
