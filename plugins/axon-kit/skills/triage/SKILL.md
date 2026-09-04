---
name: triage
description: Inspect an existing Entity and safely apply a supplied Disposition, Resurface condition, declaration, containment, or dependency change. Use for decision support and existing-Entity changes; not for choosing the decision maker, appending an independent Note, or implementation.
---

# Inspect or change an existing Axon Entity

Use `axon-kit:conventions`. Read its model contract and, before any write, its mutation contract.

## Establish the decision surface

Read the target's current context. Read Declaration Revisions, `axon log <id>`, and related Entities when their preserved history or state affects the available choices or requested consequences.

Distinguish:

- an undecided declaration
- a lost prerequisite that made the Entity orphaned
- reconsideration of an Accepted, Rejected, or Ended Entity
- a declaration-only correction
- a Resurface condition change

Before proposing or applying a Disposition change, compare current and proposed terminal status. Inspect direct dependents, Group context, active scope, and relevant frontiers. When the caller needs complete reverse `AfterEntity` impact, use `axon list` because Axon has no direct reverse-waiter query.

For an orphaned Entity, do not treat prerequisite repair and rejection as the same operation.

## Separate advice from application

For decision support, return the current decision surface, realistic choices, relevant state and relationship effects, and a recommendation when requested.

Analysis does not authorize mutation. Apply only a conclusion supplied by the calling request or workflow. If that conclusion leaves a material declaration or relationship choice unresolved, return `input required` without writing.

Axon does not require the conclusion to come from a human or an agent. This skill validates and applies the supplied conclusion without choosing the collaboration policy.

## Apply only the supplied changes

Run each mutation separately and verify its postconditions before the next phase.

- Use `axon decide ...` for Disposition and `axon when ...` for Resurface condition. Include `-r <reason>` when supplied and store that reason in typed history, not a Note.
- An `Undecided` declaration can be edited without changing Disposition.
- For a real change to a fixed declaration, first run `axon decide undecide <id>` with any supplied reason, verify the draft, edit only the supplied declaration fields and relationships, verify the complete declaration, then apply the supplied final Disposition as a separate transition.
- A no-op declaration value does not require reconsideration.
- If a staged change fails, stop with the current state, completed phases, and remaining phases. Do not automatically restore the previous declaration or Disposition.
- Route supplemental findings or handoffs to `axon-kit:add-note`; do not append them to the description.

When decomposing an accepted plan, use `axon-kit:plan` for decided children and `axon-kit:capture` for unresolved children. Apply only supplied parent and dependency relationships. Do not start a child implicitly.

## Preserve work-state independence

Changing Disposition does not change Progress or claim. Preserve both unless the caller separately requests a lifecycle change, in which case route that effect to `axon-kit:work-state`. Do not silently clear a claim during triage.

A Group decision does not rewrite descendant state. Report effects on descendant active scope. Lifecycle preconditions for releasing or ending a Group belong to `axon-kit:work-state`.

## Verify and return

Read each changed Entity and verify the requested declaration, Control state, relationships, and typed history. Verify related and derived effects that are relevant to the supplied change.

Return all changed IDs, supplied reasons, created children, preserved claim state, relevant structural and derived impact, unresolved choices, and the `DB applied` classification. Do not begin implementation.
