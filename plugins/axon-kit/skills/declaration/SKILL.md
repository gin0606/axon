---
name: declaration
description: Entity数を問わずstrict YAML declarationのreview、canonicalize、export、check、applyを安全に扱う。declaration artifactとaxon export/importに使い、通常の単一Entity変更やCLI自体の実装には使わない。
---

# Operate on Axon declaration files

Use `axon-kit:conventions`. Read its model contract and, before any DB or user-owned artifact mutation, its mutation contract.

This capability owns the declaration artifact and `axon export` / `axon import prepare|check|apply` mechanics. A declaration can edit only title, description, parent, and outgoing dependencies. It must not create or edit Progress, Disposition, Resurface condition, claim, Declaration Revision, Note, or typed history.

The calling request or workflow supplies whether the task is read-only review, canonicalization, export, or DB application, plus any adoption and reconsideration decisions needed for live changes. Artifact editing does not imply permission to mutate the DB, and DB apply does not imply permission to change Control state.

Read [the declaration workflow](references/workflow.md) completely before canonicalizing, exporting to a file, applying to the DB, combining declaration work with state changes, or handling a conflict or partial result. For a read-only `axon import check <file>` with no rewrite or application, the model and mutation boundaries in this entrypoint are sufficient.

When content decisions are unresolved, use `axon-kit:plan` for new accepted Entities, `axon-kit:capture` for new undecided Entities, and `axon-kit:triage` for existing declarations or disposition changes. Use `axon-kit:add-note` only for independently requested supplemental information.

Return `DB applied: yes`, `DB applied: no`, or `DB applied: unknown`; list changed IDs and kinds, declaration keys and allocated IDs, structural and derived impact, warnings, artifact replacement status, and retained recovery files. Do not conceal a staged or partial state behind a successful file operation.
