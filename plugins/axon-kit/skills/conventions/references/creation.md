# Entity creation contract

Read this reference before creating a new Issue or Group.

## Use the caller's identity decision

Axon does not define semantic uniqueness for Entities. The calling request or workflow decides whether an existing Entity should be reused and supplies the intended kind, declaration, parent, and outgoing dependencies. Do not perform a repository-wide duplicate policy unless the caller requires one.

Write the title and optional description so the Entity remains understandable without the creating conversation. An Issue describes one concern or work item; a Group describes an explicit plan boundary. Later findings and handoffs belong in Notes rather than the description.

## Supply the complete initial state

All four creators accept `--parent <group-id>`, repeated `--needs <entity-id>`, and one initial condition: `--manual`, `--at <YYYY-MM-DD>`, `--after <entity-id>`, or `--command <shell-string>`. No condition option means `Always`; no `--needs` means no dependencies. IDs accept full IDs or unique suffixes; repeated dependencies are stored once. Quote a shell string as one argument and use `--command='--help text'` for a leading hyphen.

Creation saves the Entity, relationships, condition, and any initial Revision in one transaction. `plan` starts Accepted, `capture` Undecided; both are NotStarted without a claim. An Accepted Revision contains the complete declaration, including dependencies, but not the condition. Undecided creation has no Revision. Initial values create no fictional decision or condition-change history. A clear failure leaves no partial Entity or records.

Command strings are saved without evaluation, including confirmation output. Use `show --skip-command-evaluation` for saved-state verification, and inspect the first Revision for Accepted creation. Normal derived queries still evaluate Command. Known initial inputs need no technical capture/edit/accept staging; this does not authorize adoption of unresolved work or automatic redecision of an existing fixed declaration.

## Treat additions as non-idempotent

`plan`, `capture`, `group plan`, and `group capture` allocate a new Entity every time. Record the exact creation payload and observe the returned ID before continuing with later phases.

If the command's outcome or allocated ID is unknown, first confirm that the process ended. Inspect `axon list` and candidate Entities for the exact intended kind, declaration, parent, actor context, and creation timing. Treat one identifiable new match as the created Entity. If no match exists, the same frozen creation may be retried once; if multiple matches remain possible, report the outcome as unknown and do not create another Entity.
