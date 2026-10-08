//! The link through which `axon gui` asks the desktop app to open a management root:
//! `me.gin0606.axon://open?root=<percent-encoded absolute path>`. The scheme is the app's
//! bundle identifier, which other applications have no reason to claim; macOS does not stop
//! one from doing so, so `axon gui` names the app by its bundle identifier as well.
use crate::error::{Result, invalid};
use std::path::{Path, PathBuf};

/// The URL scheme the desktop app registers.
pub const SCHEME: &str = "me.gin0606.axon";
/// The desktop app's bundle identifier, which `axon gui` hands links to.
pub const BUNDLE_ID: &str = "me.gin0606.axon";
const OPEN: &str = "://open?root=";

/// The link that opens `root`, an absolute path. A path that is not UTF-8 cannot be
/// registered by the app and is refused.
pub fn open_link(root: &Path) -> Result<String> {
    if !root.is_absolute() {
        return Err(invalid(format!("not an absolute path: {}", root.display())));
    }
    let Some(text) = root.to_str() else {
        return Err(invalid(format!(
            "the Axon app cannot open a path that is not UTF-8: {}",
            root.display()
        )));
    };
    let mut link = format!("{SCHEME}{OPEN}");
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            link.push(byte as char);
        } else {
            link.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(link)
}

/// The absolute path an [`open_link`] link names, or `None` for any other text.
pub fn parse_open_link(link: &str) -> Option<PathBuf> {
    let (scheme, rest) = link.split_at_checked(SCHEME.len())?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let encoded = rest.strip_prefix(OPEN)?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut input = encoded.bytes();
    while let Some(byte) = input.next() {
        bytes.push(match byte {
            b'%' => {
                let high = (input.next()? as char).to_digit(16)?;
                let low = (input.next()? as char).to_digit(16)?;
                (high * 16 + low) as u8
            }
            b'&' | b'#' => return None,
            byte => byte,
        });
    }
    let path = PathBuf::from(String::from_utf8(bytes).ok()?);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_the_path_it_was_made_from() {
        for path in ["/", "/Users/me/my repo", "/tmp/日本語/a%b&c#d?e+f"] {
            let link = open_link(Path::new(path)).unwrap();
            assert!(link.starts_with("me.gin0606.axon://open?root=/"));
            assert!(!link[SCHEME.len() + OPEN.len()..].contains(['&', '#', '?', ' ', '+']));
            assert_eq!(parse_open_link(&link).unwrap(), Path::new(path));
        }
    }

    #[test]
    fn other_links_and_relative_paths_are_refused() {
        assert!(open_link(Path::new("relative")).is_err());
        for link in [
            "",
            "axon://open?root=/a",
            "me.gin0606.axon://show?root=/a",
            "me.gin0606.axon://open?root=a",
            "me.gin0606.axon://open?root=/a%2",
            "me.gin0606.axon://open?root=/a%zz",
            "me.gin0606.axon://open?root=/a&root=/b",
            "me.gin0606.axon://open?root=/%FF",
        ] {
            assert_eq!(parse_open_link(link), None, "{link}");
        }
        assert_eq!(
            parse_open_link("ME.GIN0606.AXON://open?root=/a").unwrap(),
            Path::new("/a")
        );
    }
}
