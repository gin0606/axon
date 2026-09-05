# Using axon

axon is a local-first tracker for issues and explicit plan groups. It stores one SQLite database per management root and shares a Git repository's database across all worktrees.

This guide explains everyday use. For exact contracts, see the Japanese
[state model](../reference/state-model.md), [information model](../reference/information-model.md),
[CLI reference](../reference/cli.md), and [declaration format](../reference/declaration-file.md).

## State model

An Entity is either an `Issue` or a `Group`. Both kinds have the same generated public ID, title, description, claim, and three independent axes.

| Axis or relationship | Meaning |
| --- | --- |
| Progress | `NotStarted`, `InProgress`, or `Ended`. `Ended` means no more work will be performed. |
| Disposition | `Undecided`, `Accepted`, or `Rejected`. This records whether the work or result should be pursued. |
| Resurface condition | `Always`, `AtDate`, or `AfterEntity`. It controls when the Entity returns to attention without changing another axis. |
| Dependency | Any Entity may require any other Entity's result. |
| Containment | An Issue or Group may have one parent Group. The resulting structure is a tree. |

`ready`, `blocked`, `orphaned`, `surfaced`, `terminal`, active scope, blocking causes, and Group summaries are derived when data is read. They are never stored.

A root Entity is in active scope. A Group opens its descendants only while it is `InProgress`, `Accepted`, surfaced, and neither blocked nor orphaned. Starting a Group does not start its descendants. A Group can end only when every descendant is terminal; it can be released only when no descendant is `InProgress`.

A dependency is satisfied only by an `Ended` non-Rejected target. A Rejected target makes the dependent orphaned. In contrast, an `AfterEntity` condition is satisfied by either `Ended` or `Rejected`, because waiting has ended even when no result will be produced.

## Plan declarations, revisions, and notes

An Entity owns a plan declaration consisting of its title, description, parent Group, and outgoing dependencies. `Undecided` declarations are drafts. `Accepted` and `Rejected` declarations are fixed because the Disposition is a decision about that exact definition. To change a fixed declaration, use `axon decide undecide <id> -r <reason>`, edit it, inspect the complete result, and decide it again.

Each decision to `Accepted` or `Rejected` records the complete declaration as an immutable Declaration Revision. An unchanged declaration reuses its previous Revision. Use `axon revision list <id>`, `axon revision show <id> <number>`, and `axon revision diff <id> <from> <to>` to inspect the decision targets and their structural differences.

A Note is append-only information learned after an Entity was defined: investigation evidence, results, corrections, or handoff context. Add one with `axon note add <id> -m <body>` or `axon note add <id> -F <file>`. Notes can be added to either kind in every state without changing the declaration or control state. `note list` and `note show` use stable Entity-local numbers. `axon show` displays the description and every Note in save order without truncation; axon does not infer importance from age.

## Basic workflow

1. Run `axon init` once at the management root.
2. Create accepted work with `axon plan <title>` or an accepted plan scope with `axon group plan <title>`. Use `capture` instead of `plan` when Disposition should begin as `Undecided`.
3. Add an Entity to a plan with `axon group set <entity-id> <parent-group-id>`. Start the parent Group when its plan scope should become active.
4. Use `axon ready` to find startable Entities and `axon start <id>` to claim one explicit target.
5. Use `axon done <id>` when work ends. Finish all descendants before ending a Group.
6. Use `axon triage` for the active frontier of Undecided and orphaned Entities. Use `axon show <group-id>` to inspect the Group's complete subtree, including inactive and terminal descendants, or `axon list` for the complete management-root inventory.

Use `--parent <group-id>` on any creation command to create an Entity inside a Group atomically. Use `--kind issue|group` on list queries when only one kind is relevant.

## Editing a plan declaration

Use a declaration file when several Issues, Groups, containment edges, and dependencies need to be reviewed and changed as one plan. A declaration edits only the Entities listed under `issues` and `groups`; relationships do not expand that edit set.

1. Export an existing edit set with `axon export <id>...`, `axon export --group <group-id>`, or `axon export --group <group-id> --recursive`. Combine selectors to take their union.
2. Add or edit Entity records and their owned relationships. New records use `id: null`, a unique `key`, `base: null`, and the initial Accepted/NotStarted observed state.
3. Run `axon import prepare <file>` to assign final IDs and rewrite canonical YAML. This changes the file but not the database.
4. Run `axon import check <file>` to inspect structural changes and changes to ready, blocked, orphaned, active-scope, and Group-completion facts.
5. Run `axon import apply <file>` explicitly. It repeats validation under a write lock, applies every change in one SQLite transaction, and refreshes fingerprints and observed snapshots in the file.

Progress, Disposition, resurface conditions, claims, external references, and incoming relationships are read-only in declarations. Use the ordinary transition commands for state changes. A stale fingerprint stops check/apply instead of merging concurrent changes.

Treat an exported declaration as a working snapshot. After a successful apply, keep the rewritten file only when it is intentionally maintained elsewhere; otherwise remove the temporary working file after verifying the result. If the database commit succeeds but rewriting the file fails, retain the original file and run the same `apply` again: axon accepts the retry only when the database already matches the complete declared result.

## Choosing a query

| Command | Question answered |
| --- | --- |
| `axon ready` | Which active Entities can start now? |
| `axon triage` | Which Entities are on the active decision frontier? |
| `axon claims` | Which Entities are claimed, by whom, where, and since when? |
| `axon list` | Which Entities exist, including inactive, blocked, deferred, ended, and rejected ones? |
| `axon show <id>` | What is this Entity's state, plan scope, relationships, claim, history, and derived status? For a Group, what are the complete subtree and its direct dependencies? |
| `axon log <id>` | Why did its Disposition or resurface condition change? |
| `axon note list|show` | What supplemental information has been appended to this Entity? |
| `axon revision list|show|diff` | Which declaration was decided, and how did decided declarations differ? |

## Commands that change data

- `start`, `done`, `release`, `decide`, and `when` are transitions. Repeating the current value fails without changing state, timestamps, or history.
- `write`, `group set`, `group unset`, `dep add`, and `dep rm` are declaration settings. A real change requires an `Undecided` owner; repeating an already satisfied request succeeds without changing timestamps or history.
- `plan`, `capture`, `group plan`, and `group capture` are additions and create a new Entity each time. `note add` is also an addition and always appends a new Note.
- `show`, `write`, `start`, `done`, `release`, `decide`, `when`, `dep`, and `log` resolve the target kind from the common ID namespace.
- `dep add` and `dep rm` support Issue-to-Issue, Issue-to-Group, Group-to-Issue, and Group-to-Group dependencies.
- `group set` moves either kind below a Group; `group unset` removes its parent.
- `import prepare` changes only its YAML file; `import apply` is the only declaration command that changes Entity data.

`show` obtains its current state, declaration metadata, relationships, description, Notes, and history from one database read transaction. For a Group it also prints every descendant, including terminal Entities, as an ID-ordered containment tree with compact state markers. A following dependency section lists direct outgoing dependencies owned by the Group or its descendants, distinguishes satisfied, unresolved, and rejected targets, and labels targets outside the subtree without expanding them. Child descriptions, Notes, Revisions, histories, and claim details remain available through an individual `show` instead of being expanded into the Group view.

An Ended Group cannot be moved, gain or lose descendants, or change its outgoing dependencies. A terminal descendant below an Ended Group cannot be made non-terminal. New follow-up work belongs outside that completed scope.

## Safety and concurrency

State transitions check their preconditions and write history in one transaction. `start` checks readiness while acquiring its claim. Group completion and release inspect descendants in that same transaction.

Declaration apply validates the same containment and wait-graph invariants against a tentative full snapshot while holding an immediate write transaction. A parse, conflict, invariant, or SQLite failure leaves every Entity unchanged.

Dependency, `AfterEntity`, and containment edges are projected into activation and completion wait graphs. Relation changes reject any cycle spanning those relationship types. Group-originated waits apply to the Group and all descendants. Edge-removing `dep rm`, `when clear`, `when at`, and `group unset` remain available to repair invalid legacy data.

Claims record actor, worktree, and start time. axon does not decide that a claim is stale from its age or a process ID; inspect and release it explicitly.

## Input, output, and exit status

Successful results and mutation confirmations go to standard output. Errors go to standard error and return a non-zero status. Empty `ready`, `triage`, `claims`, and `list` queries keep standard output empty and write only a short note to standard error.

Human-readable output uses consistent structures for Entity rows, history and index rows, single-record details, and mutation confirmations. Entity rows begin with ID and kind; Note and Revision indexes begin with their Entity-local number. Long-form content and diffs remain separate from one-line records, and mutation confirmations begin with the affected Entity ID.

On an attended terminal, human-readable output uses restrained ANSI styling to reinforce generated identifiers, labels, states, and diff markers. Piped and redirected output is plain, `NO_COLOR` disables styling, and text structure never depends on color. Stored titles, descriptions, Note bodies, and reasons are preserved without styling. A downstream closed pipe is treated as successful output completion.

Revision reads use one database snapshot, and optional descriptions label `present` or `absent` separately from their content. Note bodies are stored and displayed without trimming; a body containing only whitespace is rejected. `export` and completion output are generated content and never receive human-oriented styling.

## Storage, worktrees, and identifiers

axon stores `.axon/axon.db` at the management root. In Git, the management root is the parent of the common Git directory, so linked worktrees share the database. Outside Git, commands search ancestors for the nearest database.

The executable opens only the schema version it implements. It does not rewrite an older database during an ordinary command; an unsupported version fails before Entity data is read or changed.

Every Issue and Group ID uses `<prefix>-<random six characters>`. The prefix comes from `axon init`; kind is not encoded in the ID. A full ID or a unique suffix may be used wherever an Entity ID is accepted. Group slugs do not exist.
