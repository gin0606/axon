use super::*;

#[derive(Clone, Copy, ValueEnum)]
pub enum KindFilter {
    Issue,
    Group,
}
impl KindFilter {
    fn matches(self, entity: &Entity) -> bool {
        matches!(
            (self, entity.kind),
            (Self::Issue, Kind::Issue) | (Self::Group, Kind::Group)
        )
    }
}
#[derive(Clone, Copy, ValueEnum)]
pub enum LifecycleFilter {
    Undecided,
    NotStarted,
    InProgress,
    Completed,
    Cancelled,
}
impl LifecycleFilter {
    fn state(self) -> Lifecycle {
        match self {
            Self::Undecided => Lifecycle::Undecided,
            Self::NotStarted => Lifecycle::NotStarted,
            Self::InProgress => Lifecycle::InProgress,
            Self::Completed => Lifecycle::Completed,
            Self::Cancelled => Lifecycle::Cancelled,
        }
    }
}
#[derive(Args)]
pub struct Selection {
    /// Restrict the Entity kind before evaluating any conditions
    #[arg(long)]
    kind: Option<KindFilter>,
    /// Literal, case-sensitive text in current title or description; AND with other filters. Search Note bodies with axon note search
    #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
    search: Option<String>,
}
impl Selection {
    pub fn matches(&self, entity: &Entity) -> bool {
        self.kind.is_none_or(|k| k.matches(entity))
            && self
                .search
                .as_ref()
                .is_none_or(|query| !matches_in(entity, query).is_empty())
    }
}
#[derive(Args)]
pub struct ListOptions {
    #[command(flatten)]
    pub selection: Selection,
    /// Restrict the saved lifecycle (independent of surfacing and blocking)
    #[arg(long)]
    lifecycle: Option<LifecycleFilter>,
    /// Select terminal (true) or non-terminal (false) Entities; omit for both
    #[arg(long, action = clap::ArgAction::Set)]
    terminal: Option<bool>,
}
impl ListOptions {
    pub fn matches(&self, entity: &Entity) -> bool {
        self.selection.matches(entity)
            && self
                .lifecycle
                .is_none_or(|l| l.state() == entity.current.lifecycle)
            && self
                .terminal
                .is_none_or(|terminal| terminal != entity.current.lifecycle.editable())
    }
}
fn matches_in(entity: &Entity, query: &str) -> Vec<String> {
    let mut locations = Vec::new();
    if entity.current.title.contains(query) {
        locations.push("Title".into());
    }
    if entity.current.description.contains(query) {
        locations.push("Description".into());
    }
    locations
}
pub fn list_row(snapshot: &Snapshot, entity: &Entity, selection: &Selection) -> String {
    let mut text = row(snapshot, entity);
    if let Some(query) = &selection.search {
        text.push_str(&format!(
            "  {} {}\n",
            display::muted("Matched:"),
            matches_in(entity, query).join(", ")
        ));
    }
    text
}
pub fn resolve(snapshot: &Snapshot, value: &str) -> Result<EntityId> {
    let _: EntityId = value.to_owned().try_into()?;
    let mut matches: Vec<_> = snapshot
        .entities()
        .filter(|e| e.id.to_string().ends_with(value))
        .map(|e| e.id.clone())
        .collect();
    matches.sort();
    match matches.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(sqlite::Error::Invalid(format!("no such Entity: {value}"))),
        ids => Err(sqlite::Error::Invalid(format!(
            "ambiguous Entity ID {value}: {}",
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
pub fn fresh_entity_id(prefix: &str, snapshot: &Snapshot) -> Result<EntityId> {
    fresh_entity_id_with(snapshot, || EntityId::generate(prefix))
}
fn fresh_entity_id_with(
    snapshot: &Snapshot,
    mut generate: impl FnMut() -> EntityId,
) -> Result<EntityId> {
    for _ in 0..100 {
        let id = generate();
        if snapshot.entities().all(|e| e.id != id) {
            return Ok(id);
        }
    }
    Err(sqlite::Error::Invalid(
        "could not allocate a unique Entity ID after 100 attempts".into(),
    ))
}
pub fn parse_timeout(value: &str) -> std::result::Result<Duration, String> {
    let (number, factor) = if let Some(n) = value.strip_suffix("ms") {
        (n, 1_u64)
    } else if let Some(n) = value.strip_suffix('s') {
        (n, 1_000)
    } else if let Some(n) = value.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = value.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        return Err(
            "use a positive integer followed by ms, s, m or h (for example 500ms or 2m)".into(),
        );
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err("timeout requires a positive integer".into());
    }
    let milliseconds = number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(factor))
        .filter(|n| *n > 0)
        .ok_or("timeout must be positive and fit in milliseconds")?;
    Ok(Duration::from_millis(milliseconds))
}
pub fn format_note(note: &Note, details: bool) -> String {
    format!(
        "{}  {}  {}\n{}\n\n",
        display::identity(&note.id),
        display::muted(display::timestamp(&note.context.at)),
        recorder_display(&note.context, details),
        display::human_text(&note.body)
    )
}
pub fn confirmation(id: &EntityId, effect: &str) -> String {
    let escaped = display::human_text(effect).replace('\n', "\\n");
    let effect = escaped.as_str();
    let effect = if effect.starts_with("No changes") || effect == "Cancelled" {
        display::muted(effect)
    } else if effect.starts_with("Started") {
        display::situation(effect)
    } else {
        display::positive(effect)
    };
    format!("{}  {effect}\n", display::identity(id))
}
pub fn operation_label(command: &Command) -> String {
    let (operation, target): (&str, Option<&str>) = match command {
        Command::Show { id, .. } => ("show", Some(id)),
        Command::Write { id, .. } => ("write", Some(id)),
        Command::Log { id, .. } => ("log", Some(id)),
        Command::Accept(c) => ("accept", Some(&c.id)),
        Command::Withdraw(c) => ("withdraw", Some(&c.id)),
        Command::Start(c) => ("start", Some(&c.id)),
        Command::Release(c) => ("release", Some(&c.id)),
        Command::Done(c) => ("done", Some(&c.id)),
        Command::Cancel(c) => ("cancel", Some(&c.id)),
        Command::Reconsider(c) => ("reconsider", Some(&c.id)),
        Command::When {
            command: When::Set { id, .. },
        } => ("when set", Some(id)),
        Command::When {
            command: When::Clear { id },
        } => ("when clear", Some(id)),
        Command::Dep {
            command: Dependency::Add { id, .. },
        } => ("dep add", Some(id)),
        Command::Dep {
            command: Dependency::Rm { id, .. },
        } => ("dep rm", Some(id)),
        Command::Group {
            command: Group::Set { id, .. },
        } => ("group set", Some(id)),
        Command::Group {
            command: Group::Unset { id },
        } => ("group unset", Some(id)),
        Command::Note {
            command: Notes::Add { id, .. },
        } => ("note add", Some(id)),
        Command::Note {
            command: Notes::List { id, .. },
        } => ("note list", Some(id)),
        Command::Note {
            command: Notes::Show { id, .. },
        } => ("note show", Some(id)),
        Command::Plan(_) => ("plan", None),
        Command::Capture(_) => ("capture", None),
        Command::Group {
            command: Group::Plan(_),
        } => ("group plan", None),
        Command::Group {
            command: Group::Capture(_),
        } => ("group capture", None),
        Command::List(_) => ("list", None),
        Command::Tasks(_) => ("tasks", None),
        Command::Triage(_) => ("triage", None),
        Command::Init { .. } => ("init", None),
        Command::Merge { .. } => ("merge", None),
        Command::Storage { .. } => ("storage check", None),
        Command::Docs { .. } => ("docs", None),
        Command::Import { .. } => ("import", None),
        Command::Export { .. } => ("export", None),
        Command::Actor => ("actor", None),
        Command::Note {
            command: Notes::Search { .. },
        } => ("note search", None),
        Command::Completion { .. } => ("completion", None),
    };
    target
        .map(|t| format!("{t} {operation}"))
        .unwrap_or_else(|| operation.into())
}
pub fn render_root_help() -> String {
    let mut command = Cli::command();
    command.build();
    let sections: &[(&str, &[&str])] = &[
        (
            "Workflow",
            &[
                "triage", "tasks", "capture", "plan", "accept", "start", "release", "done",
            ],
        ),
        ("Inspect", &["show", "list", "log", "note", "actor"]),
        (
            "Plan management",
            &[
                "export",
                "import",
                "write",
                "group",
                "dep",
                "when",
                "withdraw",
                "cancel",
                "reconsider",
            ],
        ),
        (
            "Setup & utilities",
            &["init", "storage", "merge", "completion", "docs", "help"],
        ),
    ];
    let mut text = format!(
        "A local issue tracker for Issues and Groups\n\n{} axon <COMMAND>\n",
        display::heading("Usage:")
    );
    for (heading, names) in sections {
        text.push_str(&format!("\n{}\n", display::heading(format!("{heading}:"))));
        for name in *names {
            let child = command.find_subcommand(name).expect("help command exists");
            let about = child
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default();
            text.push_str(&format!(
                "  {}{:padding$}  {about}\n",
                display::heading(name),
                "",
                padding = 10 - name.len()
            ));
        }
    }
    text.push_str(&format!("\n{}\n  axon help <COMMAND PATH>  Show detailed command help\n  axon docs                 Explain the lifecycle and daily workflow\n\n{}\n  -h, --help     Print help\n  -V, --version  Print version\n", display::heading("More help:"), display::heading("Options:")));
    text
}

pub fn search_notes(snapshot: &Snapshot, query: &str) -> Result<String> {
    let mut grouped = BTreeMap::<_, Vec<_>>::new();
    for note in snapshot.all_notes()? {
        if let Some(position) = note.body.find(query) {
            grouped
                .entry(&note.entity)
                .or_default()
                .push((note, position));
        }
    }
    let mut text = String::new();
    for entity in sorted(snapshot.entities().collect()) {
        for (note, position) in grouped.remove(&entity.id).unwrap_or_default() {
            text.push_str(&format!(
                "{}  {}  {}  {} {}\n",
                display::identity(&entity.id),
                display::identity(&note.id),
                display::muted(display::timestamp(&note.context.at)),
                display::muted("Excerpt:"),
                note_excerpt(&note.body, position, query.len()),
            ));
        }
    }
    Ok(text)
}

fn note_excerpt(body: &str, position: usize, query_len: usize) -> String {
    const CONTEXT: usize = 24;
    let start = body[..position]
        .char_indices()
        .rev()
        .nth(CONTEXT - 1)
        .map_or(0, |(index, _)| index);
    let end_match = position + query_len;
    let end = body[end_match..]
        .char_indices()
        .nth(CONTEXT)
        .map_or(body.len(), |(index, _)| end_match + index);
    let mut text = String::new();
    if start > 0 {
        text.push('…');
    }
    for character in body[start..end].chars() {
        match character {
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\u{061c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => {
                text.extend(character.escape_unicode());
            }
            _ => text.push_str(&display::human_text(character)),
        }
    }
    if end < body.len() {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collisions_retry_without_replacing_existing_entities() {
        let mut snapshot = Snapshot::new(StoreId::generate());
        let first: EntityId = "p-abcdef".to_string().try_into().unwrap();
        snapshot
            .create(
                first.clone(),
                Kind::Issue,
                Current {
                    title: "original".into(),
                    description: String::new(),
                    lifecycle: Lifecycle::Undecided,
                    condition: None,
                    parent: None,
                    dependencies: BTreeSet::new(),
                },
                context(),
            )
            .unwrap();
        let before = snapshot.clone();
        let mut suffixes = ["abcdef", "123456"].into_iter();
        assert_eq!(
            fresh_entity_id_with(&snapshot, || format!("p-{}", suffixes.next().unwrap())
                .try_into()
                .unwrap())
            .unwrap()
            .to_string(),
            "p-123456"
        );
        assert_eq!(snapshot, before);
    }
}
