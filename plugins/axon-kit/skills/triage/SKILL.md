---
name: triage
description: 既存Entityの判断材料を調査し、与えられた採否、時期、declaration、包含、dependencyの変更を安全に反映する。判断支援と既存Entityの変更に使い、判断主体の固定、独立したNote、実装には使わない。
---

# Inspect or change an existing Axon Entity

Use `axon-kit:conventions`. Read its model and, before any write, mutation contracts.

## Establish the decision surface

Read the complete target context, every Declaration Revision, and `axon log <id>`. Read related Entities only when their state or declaration affects the available choices or consequences.

Distinguish:

- an undecided declaration
- a lost prerequisite that made the Entity orphaned
- reconsideration of an Accepted, Rejected, or Ended Entity
- a declaration-only correction
- a Resurface condition change

Before proposing or applying a Disposition change, compare current and proposed terminal status. Inspect direct dependents, every reverse `AfterEntity` waiter, Group descendants and ancestry, active scope, and the `ready` and `triage` frontiers. There is no direct reverse-`AfterEntity` query, so enumerate all IDs with `axon list` and read their complete contexts when that impact is possible.

For an orphaned Entity, include both removing or replacing the failed dependency and changing the Entity's own Disposition among realistic choices. Do not collapse prerequisite repair into rejection.

## Separate advice from application

For decision support, return:

- what the Entity asks to decide
- realistic choices, including leaving current state unchanged
- effects on Progress, Disposition, schedule, relationships, active scope, and frontiers
- a recommendation with evidence and uncertainty when the caller requested analysis

Analysis does not authorize mutation. Apply only a conclusion supplied by the calling request or workflow. If the supplied conclusion leaves a material declaration, relationship, claim, or descendant choice unresolved, return `input required` with no write.

Axon does not require the conclusion to come from a human or an agent. This skill validates and applies the supplied conclusion without choosing the collaboration policy.

## Apply only the supplied changes

Run each mutation separately and verify its postconditions before the next phase.

- Use `axon decide ... -r <reason>` for Disposition and `axon when ... -r <reason>` for Resurface condition. Store the decision reason in typed history, not a Note.
- An `Undecided` declaration can be edited without changing Disposition.
- For a real change to a fixed declaration, first run `axon decide undecide <id> -r <reason>`, verify the complete draft, edit title, description, parent, and outgoing dependencies, then verify the entire declaration before applying the supplied final Disposition with its own reason.
- A no-op declaration value does not require reconsideration.
- If any phase of a fixed-declaration change fails, stop with the current draft, applied phases, and remaining phases. Do not automatically restore the previous declaration or disposition.
- Route supplemental findings or handoffs to `axon-kit:add-note`; do not append them to description.

When decomposing an accepted plan, use `axon-kit:plan` for decided children and `axon-kit:capture` for unresolved children. Apply only supplied parent and dependency relationships. Do not treat decomposition as permission to start any child.

## Preserve work-state independence

Changing Disposition does not change Progress or claim. If the target is `InProgress`, the caller must separately supply whether the existing claim continues, is temporarily released, or is permanently ended. Route that lifecycle effect to `axon-kit:work-state` after any required handoff Note. Do not silently clear a claim during triage.

A Group decision does not rewrite descendant state. Before rejecting, releasing, ending, or restructuring a Group, return the effects on descendant active scope and any non-terminal descendants that prevent the requested lifecycle outcome.

## Verify and return

Read the complete context and every Revision of every changed or created Entity. Verify declaration, Control state, relationships, typed history, Notes, claims, dependent and waiter effects, Group completion facts, and current frontiers as applicable.

Return all changed IDs, supplied decision reasons, any created children, claim handling, structural and derived impact, unresolved choices, and the `DB applied` classification. Do not begin implementation.
