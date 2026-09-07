# Axon snapshot merge workflow

Read this reference before preparing, resolving, checking, applying, or recovering an Axon file-backend merge.

## Freeze complete inputs

Use three explicit complete snapshots: base, ours, and theirs. Choose an unused workspace directory and an output path outside that workspace. Preserve the exact input bytes; do not edit the workspace copies, manifest, fixed context, or output preimage.

Run `axon merge prepare --base <base> --ours <ours> --theirs <theirs> --output <output> --workspace <unused-workspace>` as a standalone artifact mutation. A nonzero result can still create the workspace and preserve useful originals and diagnostics. Inspect the workspace before deciding whether the preparation was unapplied, unresolved, or unknown; do not rerun with the same workspace name.

The manifest fixes absolute input paths and digests, output and preimage, active file-store binding when applicable, and evaluation context. `choices.json` contains stable conflict and input-choice IDs, including automatic selections. `report.json` distinguishes valid, unresolved, input drift, and invalid results. A candidate is publishable only when `candidate.jsonl` and its checked metadata describe a completely valid result.

## Resolve without rewriting originals

Edit only `resolution.json`. Use the manifest's input digests as choices; `ours` and `theirs` are conversational labels, not accepted identities. Base is comparison evidence and cannot be selected as the current result. Review the complete Entity bundles and relevant record IDs rather than choosing by title or latest timestamp.

Use a repair only when the calling request or supplied decision fixes its exact effect. Supported repairs use Axon's normal guarded operations, including dependency changes, Note addition, state changes, and start with the workspace's fixed context. Do not edit historical records, fabricate a claim, or bypass the fixed-declaration transition rules. A merge conflict does not authorize a Disposition, declaration, dependency, or work-state decision.

Run `axon merge check <workspace>` after each resolution edit. It recomputes the whole candidate and invalidates earlier approval when inputs, resolution, or context drift. Inspect every remaining conflict and the resulting Entity state, relationships, history, claims, and store identity. Repeat only while new supplied decisions or verified corrections make progress.

## Publish only the checked candidate

Immediately before publication, verify that inputs, resolution, checked candidate, destination preimage, backend, store identity, and output path still match the workspace. Run `axon merge apply <workspace>` as one standalone storage mutation.

Apply publishes only the last checked valid candidate. It does not stage the result or continue Git. Verify the output bytes and store identity, run `axon storage check <output>`, and inspect the affected Entities. If the same candidate is already at the output, a verified repeat can be a no-op; do not infer that case without matching the preserved candidate and destination.

If publication may have reached the destination but completion is unknown, preserve the entire workspace and output. Confirm the process ended and compare the candidate, destination, backend, and recorded digests before any retry. Never replace the destination manually or regenerate the workspace to conceal drift.

## Recover a Git-driver conflict

The low-level driver can preserve raw inputs under `.axon/merge/<id>` while its `%A` output is a Git temporary path. Do not apply that driver workspace directly to `.axon/state.jsonl`. Build a new explicit workspace using the preserved complete inputs or verified Git stage 1/2/3 snapshots, and set the actual state file as output.

After a validated explicit apply, run `axon storage check` on the state file. The caller, not this capability, decides and performs `git add`, commit, merge or rebase continuation, or abort. Normal Axon operations remain blocked while the index entry is unmerged.

## Stop conditions

Stop and retain the workspace when a semantic choice is missing, a repair would expand authority, required input is incomplete, drift cannot be reconciled, publication is unknown, or repeated checking makes no progress. Report which inputs are authoritative, which phases completed, and what decision or observation is required next.
