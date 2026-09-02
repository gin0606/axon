# axon CLI

axon is a local-first tracker for issues and explicit plan groups. It stores one SQLite database per management root and shares a Git repository's database across all worktrees.

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

## Basic workflow

1. Run `axon init` once at the management root.
2. Create accepted work with `axon plan <title>` or an accepted plan scope with `axon group plan <title>`. Use `capture` instead of `plan` when Disposition should begin as `Undecided`.
3. Add an Entity to a plan with `axon group set <entity-id> <parent-group-id>`. Start the parent Group when its plan scope should become active.
4. Use `axon ready` to find startable Entities and `axon start <id>` to claim one explicit target.
5. Use `axon done <id>` when work ends. Finish all descendants before ending a Group.
6. Use `axon triage` for the active frontier of Undecided and orphaned Entities. Use `axon list` to inspect inactive or otherwise hidden descendants.

Use `--parent <group-id>` on any creation command to create an Entity inside a Group atomically. Use `--kind issue|group` on list queries when only one kind is relevant.

## Choosing a query

| Command | Question answered |
| --- | --- |
| `axon ready` | Which active Entities can start now? |
| `axon triage` | Which active-frontier Entities need a human decision? |
| `axon claims` | Which Entities are claimed, by whom, where, and since when? |
| `axon stale` | The same complete claim facts, without inferring staleness from age or process state. |
| `axon list` | Which Entities exist, including inactive, blocked, deferred, ended, and rejected ones? |
| `axon show <id>` | What is this Entity's state, plan scope, relationships, claim, history, and derived status? |
| `axon log <id>` | Why did its Disposition or resurface condition change? |

## Commands that change data

- `start`, `done`, `release`, `decide`, and `when` are transitions. Repeating the current value fails without changing state, timestamps, or history.
- `write`, `group set`, `group unset`, `dep add`, and `dep rm` are settings. Repeating an already satisfied request succeeds without changing timestamps or history.
- `plan`, `capture`, `group plan`, and `group capture` are additions and create a new Entity each time.
- `show`, `write`, `start`, `done`, `release`, `decide`, `when`, `dep`, and `log` resolve the target kind from the common ID namespace.
- `dep add` and `dep rm` support Issue-to-Issue, Issue-to-Group, Group-to-Issue, and Group-to-Group dependencies.
- `group set` moves either kind below a Group; `group unset` removes its parent.

An Ended Group cannot be moved, gain or lose descendants, or change its outgoing dependencies. A terminal descendant below an Ended Group cannot be made non-terminal. New follow-up work belongs outside that completed scope.

## Safety and concurrency

State transitions check their preconditions and write history in one transaction. `start` checks readiness while acquiring its claim. Group completion and release inspect descendants in that same transaction.

Dependency, `AfterEntity`, and containment edges are projected into activation and completion wait graphs. Relation changes reject any cycle spanning those relationship types. Group-originated waits apply to the Group and all descendants. Edge-removing `dep rm`, `when clear`, `when at`, and `group unset` remain available to repair invalid legacy data.

Claims record actor, worktree, and start time. axon does not decide that a claim is stale from its age or a process ID; inspect and release it explicitly.

## Input, output, and exit status

Successful results and mutation confirmations go to standard output. Errors go to standard error and return a non-zero status. Empty `ready`, `triage`, `claims`, `stale`, and `list` queries keep standard output empty and write only a short note to standard error.

The first whitespace-separated field in every list row is the Entity ID. The second identifies its kind. User-provided titles, descriptions, and reasons are displayed unchanged.

## Storage, worktrees, and identifiers

axon stores `.axon/axon.db` at the management root. In Git, the management root is the parent of the common Git directory, so linked worktrees share the database. Outside Git, commands search ancestors for the nearest database.

Every Issue and Group ID uses `<prefix>-<random six characters>`. The prefix comes from `axon init`; kind is not encoded in the ID. A full ID or a unique suffix may be used wherever an Entity ID is accepted. Group slugs do not exist.
