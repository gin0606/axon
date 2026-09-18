use super::{
    Output,
    args::{Backend, Cli, Docs, Merge, Storage},
    display, output,
    render::{actor as actor_text, file_init_output},
    store::context,
};
use axon::{
    Result,
    lifecycle::Snapshot,
    location::{InitResult, Location},
};
use clap::CommandFactory;

pub(super) fn actor() -> Result<Output> {
    Ok(output(format!("{}\n", actor_text(&context())), false))
}
pub(super) fn docs(command: Option<Docs>) -> Result<Output> {
    let text = match command {
        None => include_str!("../docs/lifecycle.txt").into(),
        Some(Docs::Declaration { example: false }) => {
            include_str!("../docs/declaration.txt").into()
        }
        Some(Docs::Declaration { example: true }) => axon::declaration::example()
            .serialize(&Snapshot::empty())
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
pub(super) fn init(prefix: Option<String>, backend: Backend) -> Result<Output> {
    let cwd = std::env::current_dir()?;
    let location = Location::discover(&cwd, true)?;
    let default_root = if matches!(backend, Backend::File) {
        &location.root
    } else {
        location
            .sqlite
            .parent()
            .and_then(|p| p.parent())
            .expect("management root")
    };
    let prefix = match prefix {
        Some(explicit) => explicit,
        None => default_prefix(default_root)?,
    };
    let text = match location.init_backend(&prefix, matches!(backend, Backend::File))? {
        InitResult::Sqlite => format!(
            "Initialized SQLite at {}\n",
            display::human_text(location.sqlite.display())
        ),
        InitResult::File(files) => file_init_output(&location, &files),
    };
    Ok(output(text, true))
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
pub(super) fn storage(command: Storage) -> Result<Output> {
    let _cwd = std::env::current_dir()?;
    let Storage::Check { snapshot } = command;
    axon::file::decode(&std::fs::read(snapshot)?)?;
    Ok(output("Valid snapshot\n".into(), false))
}
pub(super) fn merge(command: Merge) -> Result<Output> {
    let _cwd = std::env::current_dir()?;
    let saved = matches!(command, Merge::Apply { .. } | Merge::Driver { .. });
    match command {
        Merge::Prepare {
            base,
            ours,
            theirs,
            output,
            workspace,
        } => axon::file_merge::prepare(&base, &ours, &theirs, &output, &workspace, context())?,
        Merge::Check { workspace } => axon::file_merge::check(&workspace)?,
        Merge::Apply { workspace } => axon::file_merge::apply(&workspace)?,
        Merge::Driver { base, ours, theirs } => {
            axon::file_merge::driver(&base, &ours, &theirs, context())?
        }
    }
    Ok(output("Merge operation succeeded\n".into(), saved))
}
