---
name: plan
description: 新しいIssueまたはGroupを、完成したplan declarationとともにAcceptedとして登録する。採用が決まった新規作業に使い、未判断の記録、既存Entityの判断、実装には使わない。
---

# Register an accepted Axon Entity

Use `axon-kit:conventions`. Read its model, creation, and mutation contracts before creating an Entity.

## Input contract

The caller supplies an adoption decision, the intended Resurface condition, and enough context to establish the purpose, scope, and completion condition. This capability may research repository facts, improve durable wording, and detect duplicates. It must not substitute its own adoption decision, expand the plan, or begin implementation.

If the supplied content leaves a material choice about purpose, public behavior, state model, hard-to-reverse cost, decomposition, parent, or dependency, return the concrete unresolved choice to the calling workflow. Ordinary implementation details do not make an accepted plan incomplete.

Inspect current Entities using the creation contract. If an exact unfinished duplicate is already `Accepted`, return it as reused with its actual Progress and declaration instead of creating another. If the duplicate is `Undecided` or `Rejected`, return it to `axon-kit:triage` with the supplied adoption decision; this capability must not silently treat a non-Accepted candidate as a completed plan or allocate a replacement. Return an ended or structurally ambiguous candidate unless the caller already supplied the decision that distinguishes new work from reconsideration.

## Build the complete declaration

Use an Issue for one accepted work item and a Group for an explicit plan boundary containing multiple Entities. The declaration should let a later implementer recover why the work exists and what makes it complete without the creating conversation.

Include scope exclusions, constraints, parent, and outgoing dependencies only when they affect implementation or readiness. Put future investigation and implementation results in Notes, not in the initial description.

Before mutation, reread the proposed declaration without conversational context. If this capability supplied a material requirement or structural decision not already authorized by the caller, return the complete proposal as `input required` instead of writing.

## Create and verify

When the complete declaration has no outgoing dependencies and the Resurface condition is `Always`, run `axon plan <title>` for an Issue or `axon group plan <title>` for a Group. Include `--parent <group-id>` and the initial description through `-m <description>` or `-F <snapshot>` when present, so the complete declaration is accepted atomically. Apply the mutation contract's frozen-input rule before using a file or stdin.

When outgoing relationships are needed or the intended Resurface condition is not `Always`:

1. Create the Entity as `Undecided` with `axon capture` or `axon group capture`; include the decided parent and initial description in that mutation.
2. Set outgoing dependencies with separate mutations while the declaration remains draft. If the intended Resurface condition is not `Always`, apply it with `axon when ... -r <reason>` and verify it before adoption.
3. Read the complete Entity context and verify the intended draft, zero Declaration Revisions, and no unintended state or Note.
4. Run `axon decide accept <id> -r <reason>` as a standalone mutation, using the supplied reason for adoption.

If a staged phase fails, leave the one Entity `Undecided`, return its ID and the remaining phases, and do not create another Entity or accept an incomplete declaration.

After adoption, read the complete Entity context plus every Declaration Revision. For a newly created Entity, verify the fixed declaration, `Progress=NotStarted`, `Disposition=Accepted`, the intended Resurface condition, parent, dependencies, and newly recorded Revision. For a reused Accepted Entity, preserve and report its actual Progress, declaration, and history without mutating it. Return the ID, kind, declaration summary, readiness impact, and `DB applied` classification. Do not start the Entity.
