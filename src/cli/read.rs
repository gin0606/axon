use super::{
    Output, Publication,
    args::{CandidateOptions, ConditionOptions, ListOptions},
    condition, display,
    render::{self, branch_boundary, format_note, log_line},
    store::{Read, resolve},
};
use axon::{
    Result,
    lifecycle::{CandidateList, RecordId},
    read,
};

fn result(text: String, empty_hint: &str, notice: String) -> Result<Output> {
    let mut diagnostic = notice;
    if text.is_empty() {
        diagnostic.push_str(empty_hint);
        diagnostic.push('\n');
    }
    Ok(Output {
        text,
        publication: Publication::None,
        diagnostic,
    })
}
pub(super) fn export(ids: Vec<String>) -> Result<Output> {
    let opened = Read::open()?;
    let text = {
        let selectors = ids
            .iter()
            .map(|id| resolve(&opened.view, id))
            .collect::<Result<Vec<_>>>()?;
        axon::declaration::export(&opened.records, &opened.view, &selectors)
            .and_then(|d| d.serialize(&opened.view))
            .map_err(|e| axon::Error::Invalid(e.to_string()))?
    };
    result(text, "No records.", String::new())
}
pub(super) fn search_notes(query: String) -> Result<Output> {
    let opened = Read::open()?;
    let text = render::search_notes(&read::search_notes(&opened.read(), &query)?);
    result(text, "No matching Notes.", String::new())
}
pub(super) fn list(options: ListOptions) -> Result<Output> {
    let opened = Read::open()?;
    let text = render::list(
        &read::list(
            &opened.read(),
            |view, id| options.matches(view, id),
            options.selection.search.as_deref(),
        ),
        options.selection.search.is_some(),
    );
    result(text, "No matching Entities.", opened.notice())
}
pub(super) fn proposals(options: CandidateOptions) -> Result<Output> {
    candidates(options, CandidateList::Proposals)
}
pub(super) fn tasks(options: CandidateOptions) -> Result<Output> {
    candidates(options, CandidateList::Tasks)
}
fn evaluation(options: &ConditionOptions, root: std::path::PathBuf) -> condition::Evaluation {
    if options.trace_conditions {
        condition::Evaluation::tracing(root, options.condition_timeout)
    } else {
        condition::Evaluation::with_timeout(root, options.condition_timeout)
    }
}
fn candidates(options: CandidateOptions, kind: CandidateList) -> Result<Output> {
    let opened = Read::open()?;
    let text = {
        let view = opened.read();
        let evaluation = evaluation(&options.conditions, opened.location.worktree.clone());
        let rows = read::candidates(
            &view,
            kind,
            |id| options.selection.matches(&view, id),
            |entity, script| {
                evaluation
                    .run_command(entity, script)
                    .map_err(|e| axon::Error::Invalid(e.to_string()))
            },
            options.selection.search.as_deref(),
        )?;
        render::list(&rows, options.selection.search.is_some())
    };
    result(
        text,
        match kind {
            CandidateList::Proposals => {
                "No proposals candidates. Conditions and ancestor scope may hide saved Entities; use axon list for the inventory."
            }
            CandidateList::Tasks => {
                "No task candidates. Conditions and ancestor scope may hide saved Entities; use axon list for the inventory."
            }
        },
        opened.notice(),
    )
}
pub(super) fn show(
    value: String,
    details: bool,
    skip_conditions: bool,
    options: ConditionOptions,
) -> Result<Output> {
    let opened = Read::open()?;
    let id = resolve(&opened.view, &value)?;
    let view = opened.read();
    let detail = if skip_conditions {
        read::detail(&view, &id)?
    } else {
        let evaluation = evaluation(&options, opened.location.worktree.clone());
        let hint = format!(
            "Use axon show {id}{} --skip-conditions to read saved information without running conditions",
            if details { " --details" } else { "" }
        );
        read::detail_with(&view, &id, |entity, script| {
            evaluation
                .run_command(entity, script)
                .map_err(|e| axon::Error::Invalid(format!("{e}\n{hint}")))
        })?
    };
    result(
        render::show(&detail, details),
        "No records.",
        opened.notice(),
    )
}
fn record_id(value: &str) -> Result<RecordId> {
    RecordId::try_from(value).map_err(Into::into)
}
pub(super) fn show_note(id: String, note_id: String, recorder_details: bool) -> Result<Output> {
    let opened = Read::open()?;
    let text = {
        let entity = resolve(&opened.view, &id)?;
        let note_id = record_id(&note_id)?;
        let note = opened
            .records
            .note(&note_id)
            .ok_or_else(|| axon::Error::Invalid(format!("missing Note {note_id}")))?;
        if note.entity != entity {
            return Err(axon::Error::Invalid(
                "Note does not belong to the specified Entity".into(),
            ));
        }
        format_note(&note_id, note, recorder_details, false)
    };
    result(text, "No Notes.", String::new())
}
pub(super) fn log(value: String, recorder_details: bool) -> Result<Output> {
    let opened = Read::open()?;
    let text = {
        let id = resolve(&opened.view, &value)?;
        let mut text = String::new();
        for entry in read::history(&opened.records, &id)? {
            branch_boundary(entry.concurrent_with_previous, &mut text);
            text.push_str(&log_line(&entry, recorder_details));
        }
        if let Some(heads) = opened.view.heads(&id)
            && heads.len() > 1
        {
            text.push_str(&format!(
                "{}\n",
                display::situation(&format!("Conflicted: {} heads", heads.len()))
            ));
        }
        text
    };
    result(text, "No records.", String::new())
}
/// `axon resolve [ID]`: the conflicted Entities (or the one named) with their heads.
pub(super) fn conflicts(value: Option<String>) -> Result<Output> {
    let opened = Read::open()?;
    let text = {
        let view = opened.read();
        let ids: Vec<_> = match &value {
            Some(value) => {
                let id = resolve(&opened.view, value)?;
                if !opened.view.is_conflicted(&id) {
                    return Err(axon::Error::Invalid(format!(
                        "Entity {id} is not conflicted"
                    )));
                }
                vec![opened.view.key(&id).expect("conflicted Entity is known")]
            }
            // Creation order, as every listing.
            None => opened
                .view
                .in_creation_order()
                .into_iter()
                .filter(|id| opened.view.is_conflicted(id))
                .collect(),
        };
        render::conflicts(&view, &ids)
    };
    // The notice covers what the listing does not show: the conflicts it left out, the
    // violations and the gaps.
    let shown = if value.is_some() {
        1
    } else {
        opened.view.conflicted().len()
    };
    result(
        text,
        "No conflicted Entities.",
        opened.notice_excluding(shown),
    )
}
pub(super) fn list_notes(value: String, recorder_details: bool) -> Result<Output> {
    let opened = Read::open()?;
    let text = {
        let id = resolve(&opened.view, &value)?;
        let mut text = String::new();
        for (note_id, note) in read::notes(&opened.records, &id) {
            text.push_str(&format_note(note_id, note, recorder_details, true));
        }
        text
    };
    result(text, "No Notes.", String::new())
}
