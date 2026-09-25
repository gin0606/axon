use super::{
    Output, Publication,
    args::{CandidateOptions, ConditionOptions, ListOptions},
    condition, display,
    render::{self, branch_boundary, format_note, list_row, recorder_display},
    store::{open, resolve},
};
use axon::{
    Result,
    lifecycle::{CandidateList, StateEvent},
    read,
};

fn result(text: String, empty_hint: &str) -> Result<Output> {
    let diagnostic = if text.is_empty() {
        format!("{empty_hint}\n")
    } else {
        String::new()
    };
    Ok(Output {
        text,
        publication: Publication::None,
        diagnostic,
    })
}
pub(super) fn export(ids: Vec<String>) -> Result<Output> {
    let (_, store) = open()?;
    let (_, snapshot) = store.read()?;
    let text = {
        let selectors = ids
            .iter()
            .map(|id| resolve(&snapshot, id))
            .collect::<Result<Vec<_>>>()?;
        axon::declaration::export(&snapshot, &selectors)
            .and_then(|d| d.serialize(&snapshot))
            .map_err(|e| axon::Error::Invalid(e.to_string()))?
    };
    result(text, "No records.")
}
pub(super) fn search_notes(query: String) -> Result<Output> {
    let (_, store) = open()?;
    let (_, snapshot) = store.read()?;
    result(
        render::search_notes(&read::search_notes(&snapshot, &query)?),
        "No matching Notes.",
    )
}
pub(super) fn list(options: ListOptions) -> Result<Output> {
    let (_, store) = open()?;
    let (_, snapshot) = store.read()?;
    let text = read::list(
        &snapshot,
        |view, e| options.matches(view, e),
        options.selection.search.as_deref(),
    )
    .into_iter()
    .map(|e| list_row(&e, options.selection.search.is_some()))
    .collect();
    result(text, "No matching Entities.")
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
    let (location, store) = open()?;
    let (_, snapshot) = store.read()?;
    let text = {
        let evaluation = evaluation(&options.conditions, location.worktree);
        read::candidates(
            &snapshot,
            kind,
            |e| options.selection.matches(e),
            |entity, script| {
                evaluation
                    .run_command(entity, script)
                    .map_err(|e| axon::Error::Invalid(e.to_string()))
            },
            options.selection.search.as_deref(),
        )?
        .into_iter()
        .map(|e| list_row(&e, options.selection.search.is_some()))
        .collect()
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
    )
}
pub(super) fn show(
    value: String,
    details: bool,
    skip_conditions: bool,
    options: ConditionOptions,
) -> Result<Output> {
    let (location, store) = open()?;
    let (_, snapshot) = store.read()?;
    let id = resolve(&snapshot, &value)?;
    let detail = if skip_conditions {
        read::detail(&snapshot, &id)?
    } else {
        let evaluation = evaluation(&options, location.worktree);
        let hint = format!(
            "Use axon show {id}{} --skip-conditions to read saved information without running conditions",
            if details { " --details" } else { "" }
        );
        read::detail_with(&snapshot, &id, |entity, script| {
            evaluation
                .run_command(entity, script)
                .map_err(|e| axon::Error::Invalid(format!("{e}\n{hint}")))
        })?
    };
    result(render::show(&detail, details), "No records.")
}
pub(super) fn show_note(id: String, note_id: String, recorder_details: bool) -> Result<Output> {
    let (_, store) = open()?;
    let (_, snapshot) = store.read()?;
    let text = {
        let entity = resolve(&snapshot, &id)?;
        let note = snapshot.note(&note_id.try_into()?)?;
        if note.entity != entity {
            return Err(axon::Error::Invalid(
                "Note does not belong to the specified Entity".into(),
            ));
        }
        format_note(note, recorder_details, false)
    };
    result(text, "No Notes.")
}
pub(super) fn log(value: String, recorder_details: bool) -> Result<Output> {
    let (_, store) = open()?;
    let (_, snapshot) = store.read()?;
    let text = {
        let mut text = String::new();
        for entry in read::history(&snapshot, &resolve(&snapshot, &value)?)? {
            branch_boundary(entry.concurrent_with_previous, &mut text);
            let record = entry.record;
            let description = match &record.event {
                StateEvent::Created { initial, .. } => format!("Created: {initial:?}"),
                StateEvent::Transition {
                    before,
                    after,
                    reason,
                    ..
                } => format!(
                    "{before:?} → {after:?}{}",
                    reason
                        .as_ref()
                        .map(|r| format!("  Reason: {}", display::line(r)))
                        .unwrap_or_default()
                ),
                StateEvent::Integration {
                    inputs,
                    selected,
                    reason,
                } => format!(
                    "Integrated: selected {:?}{}",
                    inputs[*selected].current.lifecycle,
                    reason
                        .as_ref()
                        .map(|r| format!("  Reason: {}", display::line(r)))
                        .unwrap_or_default()
                ),
            };
            text.push_str(&format!(
                "{}  {}  {description}\n",
                display::muted(display::timestamp(&record.context.at)),
                recorder_display(&record.context, recorder_details)
            ));
        }
        text
    };
    result(text, "No records.")
}
pub(super) fn list_notes(value: String, recorder_details: bool) -> Result<Output> {
    let (_, store) = open()?;
    let (_, snapshot) = store.read()?;
    let text = {
        let mut text = String::new();
        for entry in read::notes(&snapshot, &resolve(&snapshot, &value)?)? {
            branch_boundary(entry.concurrent_with_previous, &mut text);
            text.push_str(&format_note(entry.record, recorder_details, true));
        }
        text
    };
    result(text, "No Notes.")
}
