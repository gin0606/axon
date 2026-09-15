//! Declaration publication shares the file backend's atomic replacement boundary.
use crate::sqlite::Result;
use std::path::Path;

pub fn rewrite(path: &Path, before: &[u8], bytes: &[u8]) -> Result<()> {
    let path = std::path::absolute(path)?;
    crate::file::publish(&path, Some(before), bytes, || Ok(()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn declaration_publication_distinguishes_drift_and_sync_failure() {
        let directory =
            std::env::temp_dir().join(format!("axon-declaration-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("declaration.yaml");
        std::fs::write(&path, b"original").unwrap();
        let error = crate::file::publish_with(
            &path,
            Some(b"original"),
            b"prepared",
            || {
                std::fs::write(&path, b"editor")?;
                Ok(())
            },
            || Ok(()),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("not applied")
                && error.contains("destination changed")
                && error.contains("retained"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"editor");
        let error = crate::file::publish_with(
            &path,
            Some(b"editor"),
            b"prepared",
            || Ok(()),
            || Err(std::io::Error::other("injected sync failure").into()),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("result unknown after publication"),
            "{error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"prepared");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
