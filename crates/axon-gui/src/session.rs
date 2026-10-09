//! Where the last run left off: the window's place, the selected root, the filter and the
//! layout, which the next start restores. Nothing here depends on GPUI; the window converts
//! its bounds to and from [`Placement`].
//!
//! The file is a convenience, not a record: a file that cannot be read, decoded or is of
//! another format starts the application from the defaults, and the next change overwrites it.
//! The search text, the open Entity and the Conflicted choice are not kept.

use crate::board::{Filter, Layout, State};
use crate::project::{AppData, ProjectRoot, data::SESSION_FILE};
use axon::lifecycle::{Kind, Label};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, io, sync::Mutex};

/// The version of the session file this build reads and writes.
pub const FORMAT: u32 = 1;

/// Where the window was as a plain window: the top left corner of its frame as GPUI reports it
/// (on macOS, relative to its display), the size of its content, the display it was on, and
/// whether it was maximized. A maximized or full screen window keeps the place it had before.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The UUID of the display the window was on, when the platform reports one.
    pub display: Option<String>,
    pub maximized: bool,
}

impl Placement {
    /// Whether the values can place a window: finite, with a size.
    fn is_usable(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width > 0.
            && self.height > 0.
    }
}

/// What the next start restores.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub placement: Option<Placement>,
    /// The root last selected. One no longer registered opens the first registered root.
    pub root: Option<ProjectRoot>,
    /// The states chosen in the filter, other than Conflicted, which every start chooses.
    pub states: BTreeSet<State>,
    pub kinds: BTreeSet<Kind>,
    /// The labels left out, so that a label added later shows up chosen.
    pub hidden_labels: BTreeSet<Label>,
    pub layout: Layout,
}

impl Default for Session {
    fn default() -> Self {
        Self::of(None, None, &Filter::default(), Layout::default())
    }
}

impl Session {
    pub fn of(
        placement: Option<Placement>,
        root: Option<ProjectRoot>,
        filter: &Filter,
        layout: Layout,
    ) -> Self {
        Self {
            placement,
            root,
            states: filter
                .states
                .iter()
                .copied()
                .filter(|state| *state != State::Conflicted)
                .collect(),
            kinds: filter.kinds.clone(),
            hidden_labels: Label::ALL
                .into_iter()
                .filter(|label| !filter.labels.contains(label))
                .collect(),
            layout,
        }
    }

    /// The filter to start with: the choices kept, Conflicted chosen and no search.
    pub fn filter(&self) -> Filter {
        let mut states = self.states.clone();
        states.insert(State::Conflicted);
        Filter {
            states,
            kinds: self.kinds.clone(),
            labels: Label::ALL
                .into_iter()
                .filter(|label| !self.hidden_labels.contains(label))
                .collect(),
            query: String::new(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let file = File {
            format: FORMAT,
            window: self.placement.clone().filter(Placement::is_usable),
            root: self.root.clone(),
            states: self.states.iter().map(|state| state_name(*state)).collect(),
            kinds: self.kinds.iter().map(|kind| kind_name(*kind)).collect(),
            hidden_labels: self
                .hidden_labels
                .iter()
                .map(|label| label.name())
                .collect(),
            layout: match self.layout {
                Layout::Tree => "tree",
                Layout::Flat => "flat",
            },
        };
        let mut bytes = serde_json::to_vec_pretty(&file).expect("a session always serializes");
        bytes.push(b'\n');
        bytes
    }

    /// The session in `bytes`, or `None` for anything this build does not read as one. A
    /// window place that cannot place a window is dropped alone.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let file: File<String> = serde_json::from_slice(bytes).ok()?;
        if file.format != FORMAT {
            return None;
        }
        let states = file
            .states
            .iter()
            .map(|name| {
                State::ALL
                    .into_iter()
                    .filter(|state| *state != State::Conflicted)
                    .find(|state| state_name(*state) == name)
            })
            .collect::<Option<_>>()?;
        let kinds = file
            .kinds
            .iter()
            .map(|name| {
                [Kind::Issue, Kind::Group]
                    .into_iter()
                    .find(|kind| kind_name(*kind) == name)
            })
            .collect::<Option<_>>()?;
        let hidden_labels = file
            .hidden_labels
            .iter()
            .map(|name| Label::from_name(name).ok())
            .collect::<Option<_>>()?;
        let layout = match file.layout.as_str() {
            "tree" => Layout::Tree,
            "flat" => Layout::Flat,
            _ => return None,
        };
        Some(Self {
            placement: file.window.filter(Placement::is_usable),
            root: file.root,
            states,
            kinds,
            hidden_labels,
            layout,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File<S = &'static str> {
    format: u32,
    window: Option<Placement>,
    root: Option<ProjectRoot>,
    states: Vec<S>,
    kinds: Vec<S>,
    hidden_labels: Vec<S>,
    layout: S,
}

fn state_name(state: State) -> &'static str {
    match state {
        State::Undecided => "undecided",
        State::NotStarted => "not-started",
        State::InProgress => "in-progress",
        State::Completed => "completed",
        State::Cancelled => "cancelled",
        State::Conflicted => "conflicted",
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Issue => "issue",
        Kind::Group => "group",
    }
}

/// The session file of a data directory. Saves may run on several threads at once; each
/// carries a generation, and one older than the last written is dropped, so the file never
/// goes back to an earlier state.
#[derive(Debug)]
pub struct SessionFile {
    data: AppData,
    written: Mutex<u64>,
}

impl SessionFile {
    pub fn new(data: AppData) -> Self {
        Self {
            data,
            written: Mutex::new(0),
        }
    }

    /// The session last saved, or the defaults when there is none this build can read.
    pub fn load(&self) -> Session {
        fs::read(self.data.dir().join(SESSION_FILE))
            .ok()
            .and_then(|bytes| Session::decode(&bytes))
            .unwrap_or_default()
    }

    /// The generation last written or tried.
    pub fn written(&self) -> u64 {
        *self.written.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes `session` as generation `generation`, unless a later one has been tried.
    pub fn save(&self, generation: u64, session: &Session) -> io::Result<()> {
        let mut written = self.written.lock().unwrap_or_else(|e| e.into_inner());
        if generation <= *written {
            return Ok(());
        }
        // Even a failed write may have replaced the file, so an older one never follows it.
        *written = generation;
        self.data.replace(SESSION_FILE, &session.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(path: &str) -> ProjectRoot {
        ProjectRoot::new(std::env::temp_dir().join(path)).unwrap()
    }

    fn changed() -> Session {
        let mut filter = Filter::default();
        filter.toggle_state(State::Completed, true);
        filter.toggle_state(State::Undecided, false);
        filter.toggle_state(State::Conflicted, false);
        filter.toggle_kind(Kind::Group, false);
        filter.toggle_label(Label::Chore, false);
        filter.toggle_label(Label::Spike, false);
        filter.query = "検索".into();
        Session::of(
            Some(Placement {
                x: -1200.,
                y: 40.5,
                width: 900.,
                height: 640.,
                display: Some("1F2E3D4C-0000-4000-8000-000000000001".into()),
                maximized: true,
            }),
            Some(root("work/読書会")),
            &filter,
            Layout::Flat,
        )
    }

    #[test]
    fn a_session_survives_encoding() {
        let session = changed();
        assert_eq!(Session::decode(&session.encode()), Some(session.clone()));
        assert_eq!(
            Session::decode(&Session::default().encode()),
            Some(Session::default())
        );
        let filter = session.filter();
        assert_eq!(
            filter.states,
            BTreeSet::from([
                State::NotStarted,
                State::InProgress,
                State::Completed,
                State::Conflicted
            ])
        );
        assert_eq!(filter.kinds, BTreeSet::from([Kind::Issue]));
        assert!(!filter.labels.contains(&Label::Chore) && !filter.labels.contains(&Label::Spike));
        assert_eq!(filter.labels.len(), Label::ALL.len() - 2);
        assert_eq!(filter.query, "");
        assert_eq!(Session::default().filter(), Filter::default());
    }

    #[test]
    fn labels_not_left_out_are_chosen() {
        let bytes = br#"{"format":1,"window":null,"root":null,"states":[],"kinds":["issue"],"hidden_labels":["docs"],"layout":"tree"}"#;
        let filter = Session::decode(bytes).unwrap().filter();
        assert_eq!(filter.labels.len(), Label::ALL.len() - 1);
        assert_eq!(filter.states, BTreeSet::from([State::Conflicted]));
    }

    #[test]
    fn anything_else_is_not_a_session() {
        let valid = String::from_utf8(changed().encode()).unwrap();
        let mut invalid: Vec<String> = [
            "",
            "{",
            "[]",
            r#"{"format":1}"#,
            r#"{"format":2,"window":null,"root":null,"states":[],"kinds":[],"hidden_labels":[],"layout":"tree"}"#,
            r#"{"format":1,"window":null,"root":"relative","states":[],"kinds":[],"hidden_labels":[],"layout":"tree"}"#,
            r#"{"format":1,"window":null,"root":null,"states":["conflicted"],"kinds":[],"hidden_labels":[],"layout":"tree"}"#,
            r#"{"format":1,"window":null,"root":null,"states":[],"kinds":["epic"],"hidden_labels":[],"layout":"tree"}"#,
            r#"{"format":1,"window":null,"root":null,"states":[],"kinds":[],"hidden_labels":["Feat"],"layout":"tree"}"#,
            r#"{"format":1,"window":null,"root":null,"states":[],"kinds":[],"hidden_labels":[],"layout":"grid"}"#,
        ]
        .map(String::from)
        .into();
        invalid.push(valid.replacen("\"format\"", "\"extra\": 0, \"format\"", 1));
        for bytes in invalid {
            assert_eq!(Session::decode(bytes.as_bytes()), None, "{bytes}");
        }
    }

    #[test]
    fn a_window_place_that_cannot_place_a_window_is_dropped_alone() {
        for window in [
            r#"{"x":0,"y":0,"width":0,"height":10,"display":null,"maximized":false}"#,
            r#"{"x":0,"y":0,"width":10,"height":-1,"display":null,"maximized":false}"#,
        ] {
            let bytes = format!(
                r#"{{"format":1,"window":{window},"root":null,"states":[],"kinds":["issue"],"hidden_labels":[],"layout":"flat"}}"#
            );
            let session = Session::decode(bytes.as_bytes()).expect(&bytes);
            assert_eq!(session.placement, None, "{bytes}");
            assert_eq!(session.layout, Layout::Flat);
        }
    }

    #[test]
    fn a_missing_or_damaged_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let data = AppData::at(dir.path()).unwrap();
        let file = SessionFile::new(data.clone());
        assert_eq!(file.load(), Session::default());
        file.save(1, &changed()).unwrap();
        assert_eq!(file.load(), changed());
        assert_eq!(
            SessionFile::new(data).load(),
            changed(),
            "a new start reads it"
        );
        fs::write(dir.path().join(SESSION_FILE), b"{").unwrap();
        assert_eq!(file.load(), Session::default());
        fs::remove_file(dir.path().join(SESSION_FILE)).unwrap();
        fs::create_dir(dir.path().join(SESSION_FILE)).unwrap();
        assert_eq!(file.load(), Session::default());
        assert!(file.save(2, &changed()).is_err());
        // A failed write still counts, so an older save never follows it.
        fs::remove_dir(dir.path().join(SESSION_FILE)).unwrap();
        file.save(1, &changed()).unwrap();
        assert_eq!(file.load(), Session::default());
    }

    #[test]
    fn an_older_save_never_replaces_a_newer_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = SessionFile::new(AppData::at(dir.path()).unwrap());
        file.save(2, &changed()).unwrap();
        file.save(1, &Session::default()).unwrap();
        assert_eq!(file.load(), changed());
        file.save(3, &Session::default()).unwrap();
        assert_eq!(file.load(), Session::default());
    }
}
