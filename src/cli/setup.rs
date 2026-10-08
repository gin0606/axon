use super::{
    Output,
    args::{Cli, Docs, Storage},
    display, output,
    render::{actor as actor_text, init_output, storage_report},
    store::context,
};
use axon::{Result, lifecycle::record::Store as Records, location::Location, read};
use clap::CommandFactory;

pub(super) fn actor() -> Result<Output> {
    Ok(output(format!("{}\n", actor_text(&context())), false))
}
/// `axon gui`: hands the discovered management root to the desktop app, which registers it if
/// needed and shows it. Only a store that opens here is handed over; what the app then does is
/// not reported back.
#[cfg(target_os = "macos")]
pub(super) fn gui() -> Result<Output> {
    let (location, _) = super::store::open()?;
    let link = axon::app_link::open_link(&location.root)?;
    // `-b` hands the link to Axon.app alone, even if another app claims the scheme; `-u`
    // keeps a file that happens to share the link's name from being opened instead.
    let sent = std::process::Command::new("/usr/bin/open")
        .args(["-b", axon::app_link::BUNDLE_ID, "-u", &link])
        .output()
        .map_err(|error| axon::Error::Invalid(format!("cannot run /usr/bin/open: {error}")))?;
    if !sent.status.success() {
        let stderr = String::from_utf8_lossy(&sent.stderr);
        // `open` names the function that found no app with the bundle identifier.
        if stderr.contains("LSCopyApplicationURLsForBundleIdentifier") {
            return Err(axon::Error::Invalid(
                "macOS does not know Axon.app; open it once so that macOS registers it".into(),
            ));
        }
        return Err(axon::Error::Invalid(format!(
            "/usr/bin/open did not hand the link to the Axon app: {}",
            stderr.lines().next().unwrap_or("no message").trim()
        )));
    }
    Ok(output(
        format!(
            "Sent {} to the Axon app\n",
            display::line(location.root.display())
        ),
        false,
    ))
}
#[cfg(not(target_os = "macos"))]
pub(super) fn gui() -> Result<Output> {
    Err(axon::Error::Invalid("supported only on macOS".into()))
}
pub(super) fn docs(command: Option<Docs>) -> Result<Output> {
    let text = match command {
        None => include_str!("../docs/lifecycle.txt").into(),
        Some(Docs::Declaration { example: false }) => {
            include_str!("../docs/declaration.txt").into()
        }
        Some(Docs::Declaration { example: true }) => axon::declaration::example()
            .serialize(&Records::new().view()?)
            .map_err(|e| axon::Error::Invalid(e.to_string()))?,
    };
    Ok(output(text, false))
}
pub(super) fn completion(shell: clap_complete::Shell) -> Result<Output> {
    let mut bytes = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "axon", &mut bytes);
    Ok(output(
        String::from_utf8(bytes).expect("UTF-8 completion"),
        false,
    ))
}
pub(super) fn init(prefix: Option<String>) -> Result<Output> {
    let cwd = std::env::current_dir()?;
    let location = Location::discover(&cwd, true)?;
    let prefix = match prefix {
        Some(explicit) => explicit,
        None => default_prefix(&location.root)?,
    };
    let initialized = location.init(&prefix)?;
    Ok(output(init_output(&location, &initialized), true))
}
/// The management root directory name, lowercased, when it is a usable ID prefix.
fn default_prefix(root: &std::path::Path) -> Result<String> {
    let Some(name) = root.file_name().and_then(|name| name.to_str()) else {
        return Err(axon::Error::Invalid(format!(
            "cannot read the management root directory name as an ID prefix; pass one as axon init <PREFIX>, using {}",
            axon::PREFIX_RULE
        )));
    };
    let prefix = name.to_ascii_lowercase();
    axon::validate_prefix(&prefix).map_err(|_| {
        axon::Error::Invalid(format!(
            "cannot use the directory name {} as an ID prefix; pass one as axon init <PREFIX>, using {}",
            display::human_text(name),
            axon::PREFIX_RULE
        ))
    })?;
    Ok(prefix)
}
/// `axon storage check`: corrupt files, conflicts, violations and gaps, one line each.
/// Corruption, conflicts and violations fail the command; gaps are information.
pub(super) fn storage(command: Storage) -> Result<Output> {
    let Storage::Check { root } = command;
    // Header problems are corruption too, reported in the same form as record files.
    let corrupt_header = |root: &std::path::Path, reason: String| {
        axon::Error::Invalid(format!(
            "1 corrupt files under {}; records are not derived until they are repaired\nCorrupt: {}: {}",
            display::human_text(root.join(".axon").display()),
            axon::file::HEADER_FILE,
            display::human_text(reason)
        ))
    };
    // An unmerged index is reported alone; the header and damage are checked once Git resolves it.
    let reported = |error: axon::Error| match error {
        axon::Error::NotAStore { root, .. } => corrupt_header(&root, "missing".into()),
        other => other,
    };
    let cwd = std::env::current_dir()?;
    let location = match root {
        Some(root) => Location::explicit(&root)?,
        // Discovery stops at a `.axon` without a header; for the check that directory is
        // the store to report on.
        None => Location::discover(&cwd, false).map_err(reported)?,
    };
    let store = location.open().map_err(reported)?;
    let loaded = store.load().map_err(|error| match error {
        axon::Error::Invalid(text) if text.starts_with(axon::file::CORRUPT_HEADER) => {
            corrupt_header(&location.root, text)
        }
        other => other,
    })?;
    if !loaded.is_intact() {
        let lines: Vec<_> = loaded
            .corruption_lines("Corrupt: ")
            .into_iter()
            .map(display::human_text)
            .collect();
        return Err(axon::Error::Invalid(format!(
            "{} corrupt files under {}; records are not derived until they are repaired\n{}",
            loaded.corruption.len(),
            display::human_text(store.records_path().display()),
            lines.join("\n")
        )));
    }
    let derived = loaded.view.expect("an intact store is derived");
    let view = read::View::new(&loaded.records, &derived);
    let report = storage_report(&view);
    if report.failing > 0 {
        return Err(axon::Error::Invalid(format!(
            "{} problems in the store\n{}",
            report.failing,
            report.text.trim_end()
        )));
    }
    if report.text.is_empty() {
        return Ok(Output {
            text: String::new(),
            publication: super::Publication::None,
            diagnostic: format!(
                "Store is consistent: {} Entities, no conflicts, violations or missing records.\n",
                view.entities().len()
            ),
        });
    }
    Ok(output(report.text, false))
}
