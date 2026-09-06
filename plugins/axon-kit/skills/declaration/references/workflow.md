# Declaration workflow

Read this reference before exporting to a file, canonicalizing, applying, or recovering a declaration operation.

## Keep declaration and Control state separate

A declaration edits only title, description, parent, and outgoing dependencies. Existing `Accepted` or `Rejected` declarations are fixed; `axon import` does not bypass that rule. The calling workflow must use `axon-kit:triage` to return an Entity to `Undecided` before applying a real declaration change and must make any later Disposition or Resurface-condition change separately.

New records in a declaration represent `Accepted`, `NotStarted`, `Always` Entities with no claim. Use staged `axon-kit:capture` and ordinary Control-state commands when the intended initial state differs. Do not normalize a caller's intended state merely to fit declaration import.

If a combined workflow partially completes, stop at the first unresolved phase. Report the applied DB state and remaining phases; do not compensate or roll back automatically.

## Export and review

Use explicit IDs, `--group`, and `--recursive` selectors to define the editable set. Their union is the complete edit set; relationship endpoints do not become editable automatically.

For export to a file, write to a temporary path, confirm that `axon export` succeeded, and run `axon import check` on the result before replacing an existing destination. Do not overwrite an existing caller-owned artifact unless the request includes that replacement.

For a read-only check, run `axon import check <file>` without preparing or rewriting the file. For a read-only review, keep the original unchanged, prepare a temporary copy if necessary, and inspect both the declaration content and the reported structural and derived impact.

## Add an external dependency or parent snapshot

Use `axon docs declaration` for the offline procedure and examples. Keep the destination edit set explicit. Export the external Entity separately; copy its id, base, title and complete observed mapping into destination references.entities, add kind from its source issues/groups list, and omit key/description. Never invent snapshot values or add the target to the destination edit set merely to resolve an ID.

Include exactly the snapshots needed by destination relations and observed AfterEntity targets, recursively through reference observed states. Reuse required source references.entities records, not all source records or relations. Destination readonly relations belong to external owners pointing into the destination edit set; preserve them from its export. Remove snapshots that are no longer required. Adding a correct required snapshot is not editing the referenced Entity.

New owners and existing Undecided owners can own editable edges to fixed external parents/prerequisites. After prepare/check/apply/check, require no remaining changes and verify external declaration/Control values are unchanged (new incoming edges may appear in their exports).

A missing file ID is not proof of DB absence. Check the ID and export its snapshot. For DB absence verify the active root; for a stale/incorrect reference base preserve and compare a fresh export, and return unresolved conflicts. A matching-base snapshot mismatch requires restoring exported kind/title/observed, not changing external state.

## Canonicalize or apply

Work on a private temporary copy when the caller-owned source must remain recoverable. `axon import prepare` discards comments and rewrites canonical YAML.

1. Run `axon import prepare <working-file>` as a standalone artifact mutation.
2. Inspect the rewritten YAML, including the editable set, allocated IDs, owned relationships, readonly relationships, and external snapshots.
3. Run `axon import check <working-file>`. Review all errors, warnings, structural changes, and derived impact before continuing.
4. For canonicalization only, replace the requested destination after verifying that the source has not changed. Report `DB applied: no` and stop.
5. For DB application, confirm that the checked working file has not changed, then run `axon import apply <working-file>` as a standalone DB mutation.
6. Run `axon import check <working-file>` again and require success with no remaining changes. Verify the changed Entities and relevant derived effects.

## Conflicts and uncertain results

Do not run `prepare` over a stale file to conceal a fingerprint conflict. Preserve the stale file, obtain a fresh export, and return the conflicting declaration fields and relationships to the calling workflow. Do not invent an automatic merge policy.

If the DB commit succeeded but the declaration-file rewrite failed, keep the exact file and run the same `axon import apply` again. Axon accepts that retry only when the DB already matches the complete declared result.

If command completion is unknown, first confirm that the process ended and keep the working file unchanged. Reconcile its readonly snapshots and owned values with the current DB. Retry only the same preserved apply file under Axon's recovery contract; otherwise report `DB applied: unknown`.

Delete only temporary files created by this operation, and only after their DB and artifact outcomes are known. Report the exact path and recovery purpose of any retained file.
