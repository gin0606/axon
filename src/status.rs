use crate::*;
use std::collections::HashSet;
use std::fmt::Write;

pub fn run(group: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let (id, view) = open_store()?.status_snapshot(group)?;
    let decoration = current_output_decoration();
    let output = render(&view, id.as_ref(), decoration)?;
    write_rows(
        &output,
        "No plans or ungrouped Issues have unfinished Entities or saved claims. Use `axon list` for all stored Entities.",
        decoration,
    )?;
    Ok(())
}

fn members<'a>(view: &'a View, root: &'a Entity) -> Vec<&'a Entity> {
    std::iter::once(root)
        .chain(ordered_subtree(view, &root.id).into_iter().map(|(_, e)| e))
        .collect()
}

pub(super) fn render(
    view: &View,
    selected: Option<&EntityId>,
    decoration: OutputDecoration,
) -> derived::Result<String> {
    let mut roots = view
        .iter()
        .filter(|e| selected.map_or(e.parent.is_none(), |id| &e.id == id))
        .filter(|root| {
            selected.is_some()
                || members(view, root)
                    .iter()
                    .any(|e| !e.is_terminal() || e.progress.claim().is_some())
        })
        .collect::<Vec<_>>();
    roots.sort_by(|a, b| a.id.cmp(&b.id));
    if roots.is_empty() {
        return Ok(String::new());
    }
    let scope = roots
        .iter()
        .flat_map(|root| members(view, root))
        .map(|e| e.id.clone())
        .collect::<HashSet<_>>();
    let ready = view.ready(|e| scope.contains(&e.id))?;
    let triage = view.triage(|e| scope.contains(&e.id))?;
    let claims = view
        .claims()
        .into_iter()
        .filter(|(e, _)| scope.contains(&e.id))
        .count();
    let mut output = format!(
        "Saved claims: {claims}  Triage candidates: {}  Ready candidates: {}\n",
        triage.len(),
        ready.len()
    );
    for root in roots.iter().filter(|e| e.kind == EntityKind::Group) {
        output.push('\n');
        render_item(
            &mut output,
            view,
            root,
            &scope,
            &ready,
            &triage,
            decoration,
            "",
        )?;
        let summary = view.group_summary(&root.id);
        for line in render_entity_counts("Descendants", &summary.descendants, decoration) {
            writeln!(output, "{line}").unwrap();
        }
        for (_, entity) in ordered_subtree(view, &root.id) {
            if entity.kind == EntityKind::Group
                || !entity.is_terminal()
                || entity.progress.claim().is_some()
            {
                render_item(
                    &mut output,
                    view,
                    entity,
                    &scope,
                    &ready,
                    &triage,
                    decoration,
                    "  ",
                )?;
            }
        }
        let mut ancestors = view.ancestors(&root.id);
        ancestors.sort_by(|a, b| a.id.cmp(&b.id));
        for ancestor in ancestors {
            let reasons = wait_reasons(view, ancestor, &scope)?;
            if !reasons.is_empty() {
                writeln!(output, "  External ancestor scope: {}", ancestor.id).unwrap();
                for reason in reasons {
                    writeln!(output, "    {reason}").unwrap();
                }
            }
        }
    }
    let ungrouped = roots
        .iter()
        .filter(|e| e.kind == EntityKind::Issue)
        .collect::<Vec<_>>();
    if !ungrouped.is_empty() {
        output.push_str("\nUngrouped Issues\n");
        for entity in ungrouped {
            render_item(
                &mut output,
                view,
                entity,
                &scope,
                &ready,
                &triage,
                decoration,
                "  ",
            )?;
        }
    }
    output.push_str(
        "\nUse `axon show <ID>` for the complete subtree, description, notes, and history.\n",
    );
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn render_item(
    output: &mut String,
    view: &View,
    entity: &Entity,
    scope: &HashSet<EntityId>,
    ready: &[&Entity],
    triage: &[(&Entity, TriageReason)],
    decoration: OutputDecoration,
    indent: &str,
) -> derived::Result<()> {
    write!(output, "{indent}{}", state_row(entity, decoration)).unwrap();
    if ready.iter().any(|e| e.id == entity.id) {
        writeln!(output, "{indent}  Ready candidate").unwrap();
    }
    if let Some((_, reason)) = triage.iter().find(|(e, _)| e.id == entity.id) {
        writeln!(
            output,
            "{indent}  Triage candidate: {}",
            match reason {
                TriageReason::Undecided => "Undecided",
                TriageReason::Orphaned => "Orphaned",
            }
        )
        .unwrap();
    }
    if entity.kind == EntityKind::Group && !matches!(entity.progress, Progress::Ended) {
        writeln!(
            output,
            "{indent}  Can complete: {}",
            yes_no(view.can_complete_group(&entity.id))
        )
        .unwrap();
    }
    if let Some(claim) = entity.progress.claim() {
        writeln!(
            output,
            "{indent}  Claim: {}",
            claim_details(claim, decoration)
        )
        .unwrap();
        writeln!(
            output,
            "{indent}  Active scope: {}",
            yes_no(view.within_active_scope(&entity.id)?)
        )
        .unwrap();
    }
    for reason in wait_reasons(view, entity, scope)? {
        writeln!(output, "{indent}  {reason}").unwrap();
    }
    Ok(())
}

fn wait_reasons(
    view: &View,
    entity: &Entity,
    scope: &HashSet<EntityId>,
) -> derived::Result<Vec<String>> {
    let mut reasons = Vec::new();
    if matches!(entity.progress, Progress::Ended) {
        if entity.kind == EntityKind::Group {
            view.is_surfaced(entity)?;
            if entity.disposition == Disposition::Rejected {
                reasons.push("Rejected Group: descendant scope is inactive; saved states and claims are unchanged.".to_string());
            }
        }
        return Ok(reasons);
    }
    if has_local_descendant_gate(view, entity)? {
        reasons.push(format!(
            "Descendant gate closed: Progress={}, Disposition={}",
            entity.progress.label(),
            entity.disposition.label()
        ));
    }
    if !entity.is_terminal()
        || entity.progress.claim().is_some()
        || entity.kind == EntityKind::Group
    {
        if !view.is_surfaced(entity)? {
            let mut condition = format!(
                "Resurface condition not satisfied: {}",
                entity.resurface_condition.label()
            );
            if let ResurfaceCondition::AfterEntity(id) = &entity.resurface_condition
                && let Some(target) = view.get(id)
            {
                condition.push_str(&format!("; {}", reference(target, scope)));
            }
            reasons.push(condition);
        }
        let mut targets = view.direct_dependencies(&entity.id);
        targets.sort_by(|a, b| a.id.cmp(&b.id));
        for target in targets {
            if target.disposition == Disposition::Rejected {
                reasons.push(format!(
                    "Rejected prerequisite (Orphaned): {}",
                    reference(target, scope)
                ));
            } else if !target.is_terminal() {
                reasons.push(format!(
                    "Unresolved dependency: {}",
                    reference(target, scope)
                ));
            }
        }
    }
    Ok(reasons)
}

fn parent_field(entity: &Entity) -> String {
    entity
        .parent
        .as_ref()
        .map(|id| format!("  Parent: {id}"))
        .unwrap_or_default()
}

fn state_row(entity: &Entity, decoration: OutputDecoration) -> String {
    format!(
        "{}  [{}/{}]{}\n",
        render_entity_identity(entity, decoration),
        entity.progress.label(),
        entity.disposition.label(),
        parent_field(entity)
    )
}

fn reference(entity: &Entity, scope: &HashSet<EntityId>) -> String {
    format!(
        "{}  {}{}",
        entity.id,
        entity.title,
        if scope.contains(&entity.id) {
            ""
        } else {
            "  External"
        }
    )
}
