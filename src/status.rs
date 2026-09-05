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
    for root in roots {
        write!(output, "\n{}", state_row(root, decoration)).unwrap();
        if root.kind == EntityKind::Group {
            let summary = view.group_summary(&root.id);
            writeln!(
                output,
                "  Can complete: {}",
                yes_no(view.can_complete_group(&root.id))
            )
            .unwrap();
            for line in render_entity_counts("Descendants", &summary.descendants, decoration) {
                writeln!(output, "{line}").unwrap();
            }
        }
        let block = members(view, root);
        for entity in &block {
            if entity.id != root.id && entity.kind == EntityKind::Group {
                write!(output, "  Scope: {}", state_row(entity, decoration)).unwrap();
                writeln!(
                    output,
                    "    Can complete: {}",
                    yes_no(view.can_complete_group(&entity.id))
                )
                .unwrap();
            }
        }
        output.push_str("  Saved claims (InProgress):\n");
        let mut any = false;
        for entity in &block {
            if let Some(claim) = entity.progress.claim() {
                write!(
                    output,
                    "    {}",
                    render_claim_row(entity, claim, decoration)
                )
                .unwrap();
                writeln!(
                    output,
                    "      Active scope: {}{}",
                    yes_no(view.within_active_scope(&entity.id)?),
                    parent_field(entity)
                )
                .unwrap();
                any = true;
            }
        }
        if !any {
            output.push_str("    (none)\n");
        }
        for (label, candidates) in [
            (
                "Triage candidates",
                triage.iter().map(|(e, _)| *e).collect::<Vec<_>>(),
            ),
            ("Ready candidates", ready.clone()),
        ] {
            writeln!(output, "  {label}:").unwrap();
            let mut any = false;
            for entity in &block {
                if candidates.iter().any(|e| e.id == entity.id) {
                    write!(output, "    {}", state_row(entity, decoration)).unwrap();
                    if label == "Triage candidates" {
                        let reason = triage.iter().find(|(e, _)| e.id == entity.id).unwrap().1;
                        writeln!(
                            output,
                            "      Reason: {}",
                            match reason {
                                TriageReason::Undecided => "Undecided",
                                TriageReason::Orphaned => "Orphaned",
                            }
                        )
                        .unwrap();
                    }
                    any = true;
                }
            }
            if !any {
                output.push_str("    (none)\n");
            }
        }
        output.push_str("  Waits and gates (by owning scope):\n");
        let mut owners = block.clone();
        // Ancestors explain a selected nested scope without entering its counts.
        owners.extend(view.ancestors(&root.id));
        owners.sort_by(|a, b| a.id.cmp(&b.id));
        let mut any = false;
        for entity in owners {
            let mut reasons = Vec::new();
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
                        condition.push_str(&format!("; {}", reference(target, &scope)));
                    }
                    reasons.push(condition);
                }
                let mut targets = view.direct_dependencies(&entity.id);
                targets.sort_by(|a, b| a.id.cmp(&b.id));
                for target in targets {
                    if target.disposition == Disposition::Rejected {
                        reasons.push(format!(
                            "Rejected prerequisite (Orphaned): {}",
                            reference(target, &scope)
                        ));
                    } else if !target.is_terminal() {
                        reasons.push(format!(
                            "Unresolved dependency: {}",
                            reference(target, &scope)
                        ));
                    }
                }
            }
            if !reasons.is_empty() {
                write!(output, "    {}", state_row(entity, decoration)).unwrap();
                if !scope.contains(&entity.id) {
                    output.push_str("      External ancestor scope\n");
                }
                for reason in reasons {
                    writeln!(output, "      {reason}").unwrap();
                }
                any = true;
            }
        }
        if !any {
            output.push_str("    (none)\n");
        }
    }
    output.push_str(
        "\nUse `axon show <ID>` for the complete subtree, description, notes, and history.\n",
    );
    Ok(output)
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
