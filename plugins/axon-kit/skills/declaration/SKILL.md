---
name: declaration
description: Review, export, canonicalize, check, or apply strict YAML plan declarations for any number of Entities. Use for declaration artifacts and axon export/import; not for ordinary single-Entity changes or implementation of the CLI itself.
---

# Operate on Axon declaration files

Use `axon-kit:conventions`. Read its model contract and, before any DB or artifact mutation, its mutation contract.

This capability owns declaration artifacts and the `axon export` / `axon import prepare|check|apply` mechanics. A declaration can edit only title, description, parent, and outgoing dependencies. It cannot edit Progress, Disposition, Resurface condition, claim, Declaration Revision, Note, typed history, incoming relationships, or external Entity values. Required external snapshots can be added/removed as owned relations change; this does not expand the edit set.

The calling request or workflow supplies the operation mode and the intended declaration content. Artifact editing does not authorize DB application, and DB application does not authorize a Control-state change.

Read [the declaration workflow](references/workflow.md) completely before exporting to a file, canonicalizing, applying, or recovering from a conflict or uncertain result. A read-only `axon import check <file>` needs only the boundaries in this entrypoint.

Use `axon-kit:triage` when a fixed declaration must first return to `Undecided`, and use `axon-kit:plan` or `axon-kit:capture` for single-Entity creation with supplied content and initial conditions that the declaration format cannot represent. Those capabilities remain separate from artifact mechanics.

Return the operation mode, `DB applied` classification, changed IDs and kinds, key-to-ID mappings, relevant structural and derived impact, warnings, artifact status, and any recovery file that must be retained.
