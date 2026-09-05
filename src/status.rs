use crate::*;
use std::collections::HashSet;
use std::fmt::Write;

pub fn run(group: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let (id, view) = open_store()?.status_snapshot(group)?;
    let decoration = current_output_decoration();
    let output = render(&view, id.as_ref(), decoration)?;
    write_rows(
        &output,
        "No plans or ungrouped Issues have a non-terminal root or saved claims. Use `axon list` for all stored Entities.",
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
                || !root.is_terminal()
                || members(view, root)
                    .iter()
                    .any(|e| e.progress.claim().is_some())
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
    let count_style = |count, style| if count == 0 { OUTPUT_MUTED } else { style };
    let mut output = format!(
        "{} {}  {} {}  {} {}\n",
        decoration.paint(OUTPUT_MUTED, "Saved claims:"),
        decoration.paint(count_style(claims, OUTPUT_ACTIVE), claims),
        decoration.paint(OUTPUT_MUTED, "Triage candidates:"),
        decoration.paint(count_style(triage.len(), OUTPUT_DECISION), triage.len()),
        decoration.paint(OUTPUT_MUTED, "Ready candidates:"),
        decoration.paint(count_style(ready.len(), OUTPUT_POSITIVE), ready.len()),
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
            let reasons = wait_reasons(view, ancestor, &scope, decoration)?;
            if !reasons.is_empty() {
                writeln!(
                    output,
                    "  {} {}",
                    decoration.paint(OUTPUT_MUTED, "External ancestor scope:"),
                    decoration.paint(OUTPUT_ID, &ancestor.id),
                )
                .unwrap();
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
        writeln!(
            output,
            "\n{}",
            decoration.paint(OUTPUT_HEADING, "Ungrouped Issues")
        )
        .unwrap();
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
    writeln!(
        output,
        "\n{}",
        decoration.paint(
            OUTPUT_MUTED,
            "Use `axon show <ID>` for the complete subtree, description, notes, and history."
        )
    )
    .unwrap();
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
        writeln!(
            output,
            "{indent}  {}",
            decoration.paint(OUTPUT_POSITIVE, "Ready candidate")
        )
        .unwrap();
    }
    if let Some((_, reason)) = triage.iter().find(|(e, _)| e.id == entity.id) {
        let (reason, style) = match reason {
            TriageReason::Undecided => ("Undecided", OUTPUT_DECISION),
            TriageReason::Orphaned => ("Orphaned", OUTPUT_FAILURE),
        };
        writeln!(
            output,
            "{indent}  {} {}",
            decoration.paint(OUTPUT_MUTED, "Triage candidate:"),
            decoration.paint(style, reason),
        )
        .unwrap();
    }
    if entity.kind == EntityKind::Group && !entity.is_terminal() {
        let can_complete = view.can_complete_group(&entity.id);
        writeln!(
            output,
            "{indent}  {} {}",
            decoration.paint(OUTPUT_MUTED, "Can complete:"),
            decoration.paint(
                if can_complete {
                    OUTPUT_POSITIVE
                } else {
                    OUTPUT_WAITING
                },
                yes_no(can_complete)
            ),
        )
        .unwrap();
    }
    if let Some(claim) = entity.progress.claim() {
        writeln!(
            output,
            "{indent}  {} {}",
            decoration.paint(OUTPUT_MUTED, "Claim:"),
            claim_details(claim, decoration)
        )
        .unwrap();
        let active = view.within_active_scope(&entity.id)?;
        writeln!(
            output,
            "{indent}  {} {}",
            decoration.paint(OUTPUT_MUTED, "Active scope:"),
            decoration.paint(
                if active { OUTPUT_ACTIVE } else { OUTPUT_MUTED },
                yes_no(active)
            ),
        )
        .unwrap();
    }
    for reason in wait_reasons(view, entity, scope, decoration)? {
        writeln!(output, "{indent}  {reason}").unwrap();
    }
    Ok(())
}

fn wait_reasons(
    view: &View,
    entity: &Entity,
    scope: &HashSet<EntityId>,
    decoration: OutputDecoration,
) -> derived::Result<Vec<String>> {
    let mut reasons = Vec::new();
    if matches!(entity.progress, Progress::Ended) {
        if entity.kind == EntityKind::Group {
            view.is_surfaced(entity)?;
            if entity.disposition == Disposition::Rejected {
                reasons.push(decoration.paint(
                    OUTPUT_MUTED,
                    "Rejected Group: descendant scope is inactive; saved states and claims are unchanged.",
                ));
            }
        }
        return Ok(reasons);
    }
    if entity.kind == EntityKind::Group && entity.disposition == Disposition::Rejected {
        reasons.push(decoration.paint(
            OUTPUT_MUTED,
            "Rejected Group: descendant scope is inactive; saved states and claims are unchanged.",
        ));
    } else if has_local_descendant_gate(view, entity)? {
        reasons.push(format!(
            "{} Progress={}, Disposition={}",
            decoration.paint(OUTPUT_WAITING, "Descendant gate closed:"),
            decoration.paint(progress_style(&entity.progress), entity.progress.label()),
            decoration.paint(
                disposition_style(entity.disposition),
                entity.disposition.label()
            )
        ));
    }
    if !entity.is_terminal()
        || entity.progress.claim().is_some()
        || entity.kind == EntityKind::Group
    {
        if !view.is_surfaced(entity)? {
            let mut condition = decoration.paint(
                OUTPUT_MUTED,
                format!(
                    "Resurface condition not satisfied: {}",
                    entity.resurface_condition.label()
                ),
            );
            if let ResurfaceCondition::AfterEntity(id) = &entity.resurface_condition
                && let Some(target) = view.get(id)
            {
                condition.push_str(&format!("; {}", reference(target, scope, decoration)));
            }
            reasons.push(condition);
        }
        let mut targets = view.direct_dependencies(&entity.id);
        targets.sort_by(|a, b| a.id.cmp(&b.id));
        for target in targets {
            if target.disposition == Disposition::Rejected {
                reasons.push(format!(
                    "{} {}",
                    decoration.paint(OUTPUT_FAILURE, "Rejected prerequisite (Orphaned):"),
                    reference(target, scope, decoration)
                ));
            } else if !target.is_terminal() {
                reasons.push(format!(
                    "{} {}",
                    decoration.paint(OUTPUT_WAITING, "Unresolved dependency:"),
                    reference(target, scope, decoration)
                ));
            }
        }
    }
    Ok(reasons)
}

fn parent_field(entity: &Entity, decoration: OutputDecoration) -> String {
    entity
        .parent
        .as_ref()
        .map(|id| {
            format!(
                "  {} {}",
                decoration.paint(OUTPUT_MUTED, "Parent:"),
                decoration.paint(OUTPUT_ID, id)
            )
        })
        .unwrap_or_default()
}

fn state_row(entity: &Entity, decoration: OutputDecoration) -> String {
    format!(
        "{}  [{}/{}]{}\n",
        render_entity_identity(entity, decoration),
        decoration.paint(progress_style(&entity.progress), entity.progress.label()),
        decoration.paint(
            disposition_style(entity.disposition),
            entity.disposition.label()
        ),
        parent_field(entity, decoration)
    )
}

fn reference(entity: &Entity, scope: &HashSet<EntityId>, decoration: OutputDecoration) -> String {
    format!(
        "{}  {}{}",
        decoration.paint(OUTPUT_ID, &entity.id),
        entity.title,
        if scope.contains(&entity.id) {
            String::new()
        } else {
            format!("  {}", decoration.paint(OUTPUT_MUTED, "External"))
        }
    )
}
