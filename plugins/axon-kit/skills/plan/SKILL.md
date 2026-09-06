---
name: plan
description: Register a new Issue or Group as Accepted with a complete plan declaration. Use when adoption and declaration content are already supplied; not for unresolved capture, existing-Entity decisions, or implementation.
---

# Register an accepted Axon Entity

Use `axon-kit:conventions`. Read its model, creation, and mutation contracts before creating an Entity.

## Input contract

The caller supplies the adoption decision, intended kind, complete declaration, Resurface condition, and any transition reasons it wants recorded. This capability may improve wording without changing meaning. It must not substitute its own adoption decision, choose whether another Entity should be reused, expand the plan, or begin implementation.

If the supplied declaration is incomplete or ambiguous, return the missing input without writing.

## Build the complete declaration

Use an Issue for one accepted work item and a Group for an explicit plan boundary containing multiple Entities. The declaration should let a later implementer recover why the work exists and what makes it complete without the creating conversation.

Use only the declaration content and relationships supplied by the caller. Put future investigation and implementation results in Notes, not in the initial description.

Before mutation, reread the proposed declaration without conversational context. If this capability would have to supply a material requirement or structural decision, return the missing input instead of writing.

## Create and verify

Run `axon plan <title>` for an Issue or `axon group plan <title>` for a Group. Include `--parent <group-id>`, repeat `--needs <entity-id>` for each outgoing dependency, and supply the initial description through `-m <description>` or `-F <snapshot>`. Apply the mutation contract's frozen-input rule before using a file or stdin.

Use the supplied initial condition: omit condition options for `Always`, or choose exactly one of `--manual`, `--at <YYYY-MM-DD>`, `--after <entity-id>`, or `--command <shell-string>`. IDs accept full IDs or unique suffixes. These inputs, the Entity, and its first complete Revision are saved atomically. Do not stage through capture merely to configure already-supplied dependencies or a condition. Initial values do not create transition history and creation has no reason option; do not fabricate transitions to attach a reason. Explicitly requested real transitions remain separate operations.

Initial Command strings are saved without execution. Verify using `axon show <id> --skip-command-evaluation` so confirmation does not execute an external command. Report readiness as unevaluated when applicable; evaluate it separately only when the caller needs that observation.

On a clear creation failure, no partial Entity is left. Reconcile an uncertain outcome through the creation contract before any retry.

After adoption, read the Entity context and the newly recorded Declaration Revision. Verify the fixed declaration, `Progress=NotStarted`, `Disposition=Accepted`, intended Resurface condition, parent, dependencies, no claim, and zero decision/progress transitions. Return the ID, kind, declaration summary, readiness impact, and `DB applied` classification. Do not start the Entity.
