---
name: storage
description: Initialize an Axon management root or validate a complete file snapshot. Use for backend selection, init recovery, Git ignore consequences, and axon storage check; not for migration, merge conflict resolution, or ordinary Entity operations.
---

# Initialize or validate Axon storage

Use `axon-kit:conventions`. Read its mutation contract before `init`; `storage check` is read-only.

## Keep backend selection explicit

The caller supplies `sqlite` or `file` when creating a new root. Do not choose between shared SQLite and worktree-local tracked state as an implementation detail. Existing valid configuration selects the authoritative store; do not switch it by editing configuration or by initializing over another backend.

In Git, the active management root is the current worktree root. Outside Git, `init` uses the current directory and normal commands use the nearest configured ancestor. Inspect the intended root before mutation. Do not infer an active store from an old `.axon/axon.db`, a nearby state file, or another worktree.

## Initialize without replacing state

Run `axon init --backend <sqlite|file> [prefix]` as one standalone mutation. Omit the prefix only when the management-root directory name is the intended ID prefix.

`init` does not reset or upgrade an existing authoritative store. In Git:

- SQLite uses `axon/state.db` under the common Git directory and keeps worktree configuration local by adding `/.axon/` to the shared `info/exclude`.
- File storage creates Git-trackable `.axon/config.json` and `.axon/state.jsonl` plus `.axon/.gitignore`, which excludes transient files while retaining those three files.

`init` does not stage or commit Git files and does not register the merge driver. Report every created or updated configuration, state, and ignore artifact. Preserve existing ignore content; if a parent or global ignore still hides file-backend artifacts, report the conflict instead of editing unrelated Git configuration.

If initialization stops after publishing only part of the root, preserve the state, configuration, pending marker, temporary files, and reported paths. Do not rerun into an ambiguous root or create another empty state. Re-run only after the observed files establish the CLI's documented safe recovery or no-op case.

## Validate a complete snapshot

Run `axon storage check <snapshot>` for a read-only validation of canonical file storage. It does not use backend discovery, the Git index, or Command evaluation. A successful check establishes structural validity of those exact bytes; it does not make the snapshot authoritative or prove that another file has not drifted.

## Return

Return the selected mode, management root or snapshot, backend and authoritative paths when applicable, store identity, validation result, Git-ignore consequences, changed artifacts, and storage-result classification. Do not migrate data, set up merge integration, stage, commit, or switch a live root unless a separate authorized workflow owns that effect.
