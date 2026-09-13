use crate::{Result, require};
use serde::{
    Deserialize,
    de::{MapAccess, Visitor},
};
use serde_json::{Value, value::RawValue};
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
pub fn read(path: &Path) -> Result<Vec<u8>> {
    require(
        fs::symlink_metadata(path)?.file_type().is_file(),
        format!("not a regular file: {}", path.display()),
    )?;
    Ok(fs::read(path)?)
}
pub fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
pub fn save_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_dir(path.parent().ok_or("missing parent")?)
}
pub fn lock(path: &Path) -> Result<File> {
    if path.symlink_metadata().is_ok() {
        read(path)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(e) => return Err(format!("busy lock {}: {e}", path.display()).into()),
        }
    }
    Ok(file)
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    require(
        path.is_absolute(),
        format!("use an absolute path: {}", path.display()),
    )?;
    let actual = fs::canonicalize(path)?;
    require(
        actual == path,
        format!("use the canonical path: {}", actual.display()),
    )?;
    Ok(actual)
}
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub fn human(text: impl fmt::Display) -> String {
    text.to_string()
        .chars()
        .flat_map(|c| {
            if c.is_control() && c != '\n' {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

struct Object(BTreeMap<String, Box<RawValue>>);
impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object without duplicate keys")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Object, M::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON key: {key}"
                        )));
                    }
                }
                Ok(Object(values))
            }
        }
        d.deserialize_map(V)
    }
}
pub fn json(text: &str) -> Result<Value> {
    let raw: Box<RawValue> = serde_json::from_str(text)?;
    match raw.get().as_bytes().first() {
        Some(b'{') => {
            let object: Object = serde_json::from_str(raw.get())?;
            Ok(Value::Object(
                object
                    .0
                    .into_iter()
                    .map(|(k, v)| Ok((k, json(v.get())?)))
                    .collect::<Result<_>>()?,
            ))
        }
        Some(b'[') => {
            let array: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
            Ok(Value::Array(
                array
                    .into_iter()
                    .map(|v| json(v.get()))
                    .collect::<Result<_>>()?,
            ))
        }
        _ => Ok(serde_json::from_str(raw.get())?),
    }
}
pub fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_value(json(std::str::from_utf8(&read(
        path,
    )?)?)?)?)
}
pub fn keys(value: &Value, expected: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or("expected an object")?;
    require(
        object.len() == expected.len() && expected.iter().all(|k| object.contains_key(*k)),
        format!(
            "unexpected or missing fields: expected {expected:?}, found {:?}",
            object.keys().collect::<Vec<_>>()
        ),
    )
}
