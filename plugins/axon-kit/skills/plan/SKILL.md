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

When the complete declaration has no outgoing dependencies and the Resurface condition is `Always`, run `axon plan <title>` for an Issue or `axon group plan <title>` for a Group. Include `--parent <group-id>` and the initial description through `-m <description>` or `-F <snapshot>` when present, so the complete declaration is accepted atomically. Apply the mutation contract's frozen-input rule before using a file or stdin.

When outgoing relationships are needed or the intended Resurface condition is not `Always`:

1. Create the Entity as `Undecided` with `axon capture` or `axon group capture`; include the decided parent and initial description in that mutation.
2. Set outgoing dependencies with separate mutations while the declaration remains draft. If the intended Resurface condition is not `Always`, apply it with `axon when ...`, include `-r <reason>` when supplied, and verify it before adoption.
3. Read the complete Entity context and verify the intended draft, zero Declaration Revisions, and no unintended state or Note.
4. Run `axon decide accept <id>` as a standalone mutation, including `-r <reason>` when supplied.

If a staged phase fails, leave the one Entity `Undecided`, return its ID and the remaining phases, and do not create another Entity or accept an incomplete declaration.

After adoption, read the Entity context and the newly recorded Declaration Revision. Verify the fixed declaration, `Progress=NotStarted`, `Disposition=Accepted`, intended Resurface condition, parent, and dependencies. Return the ID, kind, declaration summary, readiness impact, and `DB applied` classification. Do not start the Entity.
