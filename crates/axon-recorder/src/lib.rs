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
    use proptest::prelude::*;

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

    proptest! {
        #[test]
        fn source_precedence_and_metadata(
            sources in prop::collection::vec("[a-z]{1,8}", 7),
            sessions in prop::collection::vec(
                prop_oneof![
                    Just(None),
                    Just(Some(String::new())),
                    Just(Some(" \t".into())),
                    "[a-z]{1,8}".prop_map(Some),
                ],
                2,
            ),
            absent in prop::collection::vec(any::<bool>(), 7),
            unrelated in "[a-z]{1,8}",
        ) {
            const KEYS: [&str; 7] = [
                "AXON_ACTOR", "CODEX_THREAD_ID", "CODEX_SANDBOX", "CLAUDECODE",
                "CLAUDE_CODE", "AI_AGENT", "USER",
            ];

            for selected in 0..=KEYS.len() {
                let mut values = Vec::from([
                    ("AXON_SESSION_ID", sessions[0].as_deref()),
                    ("CLAUDE_CODE_SESSION_ID", sessions[1].as_deref()),
                    ("SECRET_TOKEN", Some(unrelated.as_str())),
                ]);
                for (index, key) in KEYS.iter().enumerate() {
                    let value = if index < selected {
                        if absent[index] { None } else { Some(" \t") }
                    } else {
                        Some(sources[index].as_str())
                    };
                    values.push((key, value));
                }

                let actual = detect_with(|key| {
                    values.iter().find(|(name, _)| *name == key)
                        .and_then(|(_, value)| value.map(str::to_owned))
                });
                let expected = if selected == KEYS.len() {
                    None
                } else {
                    let actor = match selected {
                        0 | 5 | 6 => sources[selected].clone(),
                        1 | 2 => "codex".into(),
                        3 | 4 => "claude-code".into(),
                        _ => unreachable!(),
                    };
                    let session = match selected {
                        0 => sessions[0].as_deref(),
                        1 => Some(sources[1].as_str()),
                        3 | 4 => sessions[1].as_deref(),
                        _ => None,
                    }.filter(|value| !value.trim().is_empty());
                    let data = session.map(|value| BTreeMap::from([("session_id".into(), value.into())]))
                        .unwrap_or_default();
                    Some(Recorder { actor, data })
                };
                prop_assert_eq!(actual, expected);
            }
        }
    }
}
