use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordKind {
    Note,
    Revision,
    Decision,
    Progress,
    Store,
}
impl RecordKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Revision => "rev",
            Self::Decision => "decision",
            Self::Progress => "progress",
            Self::Store => "store",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordId {
    kind: RecordKind,
    value: u128,
}
impl RecordId {
    pub fn new(kind: RecordKind) -> Self {
        Self {
            kind,
            value: rand::random(),
        }
    }

    pub fn deterministic(kind: RecordKind, input: &[u8]) -> Self {
        let mut hash = blake3::Hasher::new_derive_key("axon stable records v12");
        hash.update(kind.prefix().as_bytes());
        hash.update(input);
        Self {
            kind,
            value: u128::from_be_bytes(hash.finalize().as_bytes()[..16].try_into().unwrap()),
        }
    }

    pub fn kind(self) -> RecordKind {
        self.kind
    }

    pub fn matches(self, reference: &str) -> bool {
        let full = self.to_string();
        full == reference
            || (!reference.is_empty()
                && reference.len() >= 4
                && (full.starts_with(reference)
                    || full.split_once('-').unwrap().1.starts_with(reference)))
    }
}
impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{:032x}", self.kind.prefix(), self.value)
    }
}
impl FromStr for RecordId {
    type Err = std::io::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid =
            || std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid stable record ID");
        let (prefix, hex) = s.split_once('-').ok_or_else(invalid)?;
        let kind = match prefix {
            "note" => RecordKind::Note,
            "rev" => RecordKind::Revision,
            "decision" => RecordKind::Decision,
            "progress" => RecordKind::Progress,
            "store" => RecordKind::Store,
            _ => return Err(invalid()),
        };
        if hex.len() != 32
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        Ok(Self {
            kind,
            value: u128::from_str_radix(hex, 16).map_err(|_| invalid())?,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_round_trip_and_reject_noncanonical_values() {
        for kind in [
            RecordKind::Note,
            RecordKind::Revision,
            RecordKind::Decision,
            RecordKind::Progress,
            RecordKind::Store,
        ] {
            let id = RecordId::new(kind);
            assert_eq!(id.to_string().parse::<RecordId>().unwrap(), id);
            assert_ne!(id, RecordId::new(kind));
            assert_eq!(
                RecordId::deterministic(kind, b"fixed"),
                RecordId::deterministic(kind, b"fixed")
            );
        }
        for invalid in [
            "1",
            "note-1",
            "rev-ABCDEF00000000000000000000000000",
            "other-00000000000000000000000000000000",
        ] {
            assert!(invalid.parse::<RecordId>().is_err());
        }
    }
}
