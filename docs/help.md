# axon CLI

axon is a local-first issue tracker that keeps independent decisions on separate axes. Its primary use is coordinating coding agents and people inside one Git repository. It stores one untracked SQLite database per repository and shares it across that repository's worktrees.

## State model

Every issue has three independent axes and two kinds of relationship.

| Kind | Values and meaning |
| --- | --- |
| Progress | `NotStarted`, `InProgress`, or `Ended`. `Ended` means no more work will be performed; use Disposition to tell completion from abandonment. |
| Disposition | `Undecided`, `Accepted`, or `Rejected`. This records whether the issue's work or result should be pursued. |
| Resurface condition | `Always`, `AtDate`, or `AfterIssue`. This controls when the issue returns to attention without changing Progress or Disposition. |
| Dependency | An issue requires another issue's result before it can start. |
| Group | A hierarchy for related issues. Dependencies may also exist between groups. |

`ready`, `blocked`, `orphaned`, `surfaced`, and `terminal` are derived when data is read; they are not stored states.

- An issue is `ready` when it is `NotStarted`, `Accepted`, surfaced, has no unresolved dependency, is not orphaned, and is not blocked by a group dependency.
- An issue is `blocked` while a dependency is not terminal.
- An issue is `orphaned` when a dependency is `Rejected`, so the required result will not be produced. axon does not change the dependent issue's Disposition automatically; a person must decide whether to remove the dependency or reject the dependent issue.
- An issue is `surfaced` when its resurface condition is satisfied.
- An issue is `terminal` when its Progress is `Ended` or its Disposition is `Rejected`.

Read Progress and Disposition together. `Ended + Accepted` means the accepted work was completed, `Ended + Rejected` means work stopped without an accepted result, and `Ended + Undecided` means an investigation ended but its result still needs a decision. No command silently changes both axes.

## Basic workflow

1. Run `axon init` once in a Git repository.
2. Use `axon plan <title>` for work already accepted, or `axon capture <title>` for an observation that still needs a decision.
3. Use `axon ready` to find mechanically startable work. Choose an explicit ID, then run `axon start <id>` to claim only that issue.
4. Use `axon write <id>` to update its title, description, work result, or handoff. When work will not continue, run `axon done <id>` to set Progress to `Ended`.
5. Use `axon triage` to find `Undecided` or orphaned issues. Inspect them with `axon show` and `axon log`, then use `axon decide accept` or `axon decide reject` only after a person makes the decision.

If a session may have died while holding a claim, use `axon stale` to find stale claims. Check the recorded actor and session before `axon release <id>`. `stale` only reports; it never releases a claim.

## Choosing a query

| Command | Use it to answer |
| --- | --- |
| `axon ready` | Which issues can be started now? |
| `axon triage` | Which issues require a human disposition decision? |
| `axon list` | What issues exist, including blocked, deferred, active, ended, and rejected ones? |
| `axon show <id>` | What is this issue's current state, claim, progress history, dependencies, dependents, group, and blocking cause? |
| `axon log <id>` | Why did its Disposition or resurface condition change? |
| `axon stale` | Which claims are old and owned by processes that are no longer running? |

`plan` creates `Accepted` work, while `capture` creates `Undecided` work. A captured issue therefore appears in `triage`, not `ready`. Use `list` when an issue appears in neither query.

## Commands that change data

- `start` atomically checks that an issue is ready, sets Progress to `InProgress`, creates its claim, and records the actor and time. It never selects an issue for you.
- `done` accepts only `InProgress -> Ended`. It removes the claim and records the actor and time in progress history. On success, it prints only the target issue's end confirmation; run `axon ready` separately to query current candidates across all relationships and resurface conditions. It does not accept a reason; record work results in the issue description.
- `release` accepts only `InProgress -> NotStarted`. It removes the claim and records the optional handoff or release reason in progress history.
- `decide accept`, `decide reject`, and `decide undecide` change only Disposition and record the optional reason in the decision log.
- `when at`, `when after`, and `when clear` change only the resurface condition and record the optional reason in the decision log.
- `write` changes the title or description. `--message` and `--file` are mutually exclusive; `--file -` reads the description from standard input.
- `dep add` and `dep rm` add or remove an issue dependency. A dependency means the other issue's result is required, not merely that it should happen first.
- `group new`, `group set`, and `group unset` manage group membership. An issue belongs to at most one group.
- `group dep add` and `group dep rm` manage dependencies between groups.
- `group reject` converges every descendant issue to `Rejected`. Its optional reason is recorded for issues that actually change; already rejected issues remain unchanged and receive no decision event.

`write`, membership changes, dependency changes, and `group reject` are target-setting operations: repeating an already satisfied request succeeds without changing timestamps or adding history. `start`, `done`, `release`, `decide`, and `when` are transitions: an invalid transition or a request for the current value fails without changing state, history, or timestamps.

Every `--reason` option is optional. Reasons belong to typed state changes rather than free-standing comments: `release` reasons appear in progress history from `show`, while `decide`, `when`, and `group reject` reasons appear in decision history from `log`. `start` and `done` do not accept reasons.

## Safety and concurrency

- Query commands do not claim work or make decisions. `ready`, `triage`, and `stale` deliberately separate observation from mutation.
- `start` checks readiness and acquires the claim in one transaction. Concurrent attempts to start the same issue cannot both succeed.
- A claim identifies its actor, session, process, and start time. Do not release a live claim merely because it is old.
- `done` and `release` operate on the current claim and reject issues in any other Progress state.
- Rejecting a dependency makes dependents orphaned. axon reports this for human triage instead of guessing whether the dependency should be removed.
- A resurface reference and a dependency have different meaning. If referenced issue X is rejected, `when after X` becomes surfaced because waiting is over; `dep add --needs X` becomes orphaned because X's result will not exist.
- Issue dependencies and `AfterIssue` references form one issue-wait graph for cycle detection even though their meanings remain distinct. `dep add` and `when after` reject direct or indirect cycles. Group dependencies and the group-parent hierarchy are separate acyclic graphs; axon does not detect deadlocks spanning issue and group relationships.
- Cycle checks and edge updates run in the same write transaction. Removing an edge remains allowed even when an existing database already contains a cycle.
- Decision and release reasons are user-provided text. axon stores and displays them verbatim; do not put secrets in issue data or command arguments.

## Input, output, and exit status

Help, query results, details, and successful mutation confirmations go to standard output. Argument and operation errors go to standard error and return a non-zero status.

When `ready`, `triage`, or `list` has no rows, standard output stays empty so pipelines receive no false candidate. A short explanation is written to standard error and the command still succeeds. In issue-list output, the first whitespace-separated field is always the issue ID, so commands such as `axon ready | fzf --preview 'axon show {1}'` work predictably.

Issue titles, descriptions, reasons, group names, and other user-provided text are displayed unchanged and may use any language. Fixed help, status labels, confirmations, warnings, and errors are in English. There is no localization mode.

## Storage, worktrees, and identifiers

axon stores data in `.axon/axon.db` beside the repository's common Git directory and does not track it with Git. `git rev-parse --git-common-dir` is used so all worktrees of the same repository share one database.

Issue IDs have the form `<prefix>-<random 6 characters>`. The prefix comes from `axon init` or defaults to the repository directory name. Commands accept a full ID or its six-character suffix; an ambiguous suffix fails and lists its matches. Groups are addressed by their user-chosen slug.

## Help forms

- `axon -h` prints the short command list.
- `axon --help` and `axon help all` print this manual followed by generated reference help for every leaf command.
- `axon help <command path>` and `axon <command path> --help` print help for one command path.
