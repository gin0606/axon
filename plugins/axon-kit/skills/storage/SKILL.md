---
name: storage
description: Initialize an Axon management root or validate a complete file snapshot. Use for backend selection, init recovery, Git ignore consequences, and axon storage check; not for migration, merge conflict resolution, or ordinary Entity operations.
---

# Initialize or validate Axon storage

Use `axon-kit:conventions`. Read its mutation contract before `init`; `storage check` is read-only.

## Keep backend selection explicit

The caller supplies `sqlite` or `file` when creating a new root. Do not choose between shared SQLite and worktree-local tracked state as an implementation detail. One backend per Git repository is supported; mixed backends across worktrees are an unsupported operational state, not an invitation to create a selector.

There is no backend configuration file. In Git, file storage is at the current worktree root's `.axon/state.jsonl`; SQLite is at `.axon/axon.db` under the parent of the Git common directory and is shared without worktree registration. Outside Git, normal discovery stops at the nearest ancestor with either canonical state or `init.pending`. Both canonical paths present is a mixed-backend error; neither is uninitialized. Corruption, unreadable state, or a pending marker stops discovery without fallback. Do not infer the authority of files outside these prescribed paths.

## Initialize without replacing state

Run `axon init --backend <sqlite|file> [prefix]` as one standalone mutation. Omit the prefix only when the storage-root directory name is the intended ID prefix. Git-free init uses the current directory and rejects nesting under an existing management root.

Init is new-only: existing state, including valid state, is rejected. It does not reset, upgrade, switch backend, or repair. SQLite creates the database without changing ignore files. File init, inside or outside Git, creates or complements `.axon/.gitignore` (ignore all except itself and `state.jsonl`) and root `.gitattributes` (`/.axon/state.jsonl merge=axon`).

Report created or updated state and integration artifacts. Init does not register the driver, stage, or commit. Preserve unrelated rules. An outer or global ignore hiding `.axon/` is user policy, not an initialization error or authorization to alter that policy.

After partial initialization, preserve state, pending marker, temporary and integration files. Stop writers and inspect the reported paths. Complete auxiliary files manually only with appropriate authority and valid state; remove the marker only after verification. Otherwise preserve the incomplete box elsewhere before a fresh init. Re-running init is not recovery, and existing state must never be overwritten.

## Validate a complete snapshot

Run `axon storage check <snapshot>` for a read-only validation of canonical file storage. It does not use backend discovery, the Git index, or Command evaluation. A successful check establishes structural validity of those exact bytes; it does not make the snapshot authoritative or prove that another file has not drifted.

## Return

Return the selected mode, management root or snapshot, backend and authoritative paths when applicable, store identity, validation result, Git-ignore consequences, changed artifacts, and storage-result classification. Do not migrate data, register the Git driver, stage, commit, or switch a live root unless a separate authorized workflow owns that effect.
