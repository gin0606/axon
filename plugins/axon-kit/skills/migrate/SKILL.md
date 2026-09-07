---
name: migrate
description: Convert a preserved Axon current-schema SQLite snapshot into a verified SQLite or file-backend output without switching the live root. Use for axon migrate and recovery of its artifacts; not for ordinary schema opening, root initialization, or merge conflicts.
---

# Migrate a preserved Axon snapshot

Use `axon-kit:conventions` and read its mutation contract. Migration creates a new artifact set but never modifies the source or switches the active root.

## Freeze the migration boundary

The caller supplies the exact source SQLite database, an unused output directory, and target backend `sqlite` or `file`. The source must match the current schema. This command does not run schema updates; ordinary commands own supported automatic updates. Retired formats require a compatible build or a separately planned one-time conversion.

Before the final conversion, stop every writer to the source and preserve the source database together with any WAL/SHM files and the exact old and new binaries needed for investigation. Record their paths and digests. Do not add Notes or other bookkeeping to the source while it is frozen.

Choosing a target backend is a user or calling-workflow decision. File storage is worktree-local and Git-trackable; Git SQLite is shared at `.axon/axon.db` under the parent of the common Git directory. This capability does not choose that tradeoff or infer it from the current root.

## Convert once

Run `axon migrate --source <db> --output <unused-directory> --backend <sqlite|file>` as a standalone mutation. The output directory must not be deleted or reused after a failed or uncertain attempt.

On success, verify the final manifest and every recorded digest. Require the expected preserved source backup, target `axon.db` or `state.jsonl`, canonical `snapshot.jsonl`,  and completed validation results. Run `axon storage check <output>/snapshot.jsonl` as an independent read-only check.

When practical, copy the verified target store into a separate disposable root and inspect Entities, Notes, Revisions, and histories with the candidate binary. Do not use this validation copy as the live result.

No backend configuration is produced. Migration does not create Git ignore/attribute rules or register the driver; those are separate cutover tasks. Init is new-only and cannot repair integration after cutover.

## Keep cutover separate

Migration output is not an active-root switch. Do not replace storage, update tracked files, restart writers, or remove the old version unless the caller separately authorizes and owns cutover. A cutover must keep writers stopped, preserve the old root and configuration, place only the selected backend's canonical state without overwriting retained data, and verify every intended worktree against one compatible binary and store identity.

If the top-level manifest is absent, unreadable, incomplete, or has a digest mismatch, do not adopt the result. Preserve the entire output and classify the attempt from its diagnostic boundary. A new attempt uses another unused directory and the same verified source snapshot; never erase an ambiguous attempt merely to reuse its path.

## Return

Return the source and binary identities, target backend, output path, manifest and validation results, backups that must be retained, storage-result classification, and whether live cutover remains outstanding.
