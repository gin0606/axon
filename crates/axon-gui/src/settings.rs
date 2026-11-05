//! Display preferences, independent of the disposable window session.

use crate::{
    Language,
    project::{AppData, InstanceLock},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    sync::{Arc, Mutex},
};

pub const SETTINGS_FILE: &str = "settings.json";

pub(crate) struct Initial(pub Result<LanguagePreference, LoadError>);
impl gpui_kit::Global for Initial {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "en")]
    English,
}

impl LanguagePreference {
    pub fn resolve(self, system: Language) -> Language {
        match self {
            Self::System => system,
            Self::Japanese => Language::Japanese,
            Self::English => Language::English,
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Unreadable(io::Error),
    Invalid(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(error) => write!(f, "cannot read {SETTINGS_FILE}: {error}"),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    language: LanguagePreference,
}

/// Read-only, including when another instance owns the data directory.
pub fn load(data: &AppData) -> Result<LanguagePreference, LoadError> {
    let bytes = match fs::read(data.dir().join(SETTINGS_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LanguagePreference::System);
        }
        Err(error) => return Err(LoadError::Unreadable(error)),
    };
    let file: File =
        serde_json::from_slice(&bytes).map_err(|error| LoadError::Invalid(error.to_string()))?;
    if file.format != 1 {
        return Err(LoadError::Invalid(format!(
            "Unsupported settings format: {}",
            file.format
        )));
    }
    Ok(file.language)
}

/// Serializes writes through this handle and keeps the instance lock alive. Generations are recorded
/// before replacement, since even a failed directory sync may have published the file.
pub struct SettingsFile {
    lock: Arc<InstanceLock>,
    attempted: Mutex<u64>,
}

impl SettingsFile {
    pub fn new(lock: Arc<InstanceLock>) -> Self {
        Self {
            lock,
            attempted: Mutex::new(0),
        }
    }

    pub fn load(&self) -> Result<LanguagePreference, LoadError> {
        load(self.lock.data())
    }

    pub fn save(&self, generation: u64, language: LanguagePreference) -> io::Result<()> {
        let mut attempted = self
            .attempted
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if generation <= *attempted {
            return Ok(());
        }
        *attempted = generation;
        let mut bytes = serde_json::to_vec_pretty(&File {
            format: 1,
            language,
        })
        .expect("a language preference always serializes");
        bytes.push(b'\n');
        self.lock.data().replace(SETTINGS_FILE, &bytes)
    }
}
