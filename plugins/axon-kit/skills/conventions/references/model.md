# Axon model contract

Read this reference when an operation needs to interpret or change Entity data.

## Entity and state axes

Axon has two Entity kinds, Issue and Group. Both have a stable ID and the same three independent state axes:

- Progress: `NotStarted`, `InProgress`, or `Ended`
- Disposition: `Undecided`, `Accepted`, or `Rejected`
- Resurface condition: `Always`, `AtDate`, `AfterEntity`, `Manual`, or `Command`

Do not use one axis as a proxy for another. A command that changes Progress must not silently change Disposition or Resurface condition, and the reverse also applies.

An Entity is terminal when its Progress is `Ended` or its Disposition is `Rejected`. `ready`, `blocked`, `orphaned`, `surfaced`, `terminal`, active scope, blocking causes, and Group completion facts are derived from stored state and relationships; do not treat them as independently editable data.

`ready` means that an Entity is in active scope, `NotStarted`, `Accepted`, surfaced, not blocked, and not orphaned. `triage` includes an Entity only when all four conditions hold: it is non-terminal, its own Resurface condition is satisfied (surfaced), it is in active scope (all ancestor Group activation gates are open), and it is `Undecided` or orphaned. These definitions do not assign the decision or work to a human or an agent.

`Manual` remains unsurfaced until explicitly replaced or cleared with `axon when clear`. It has no payload and changes neither Progress, Disposition, nor claim. A Manual Group closes descendant active scope without changing descendant state. A root Entity is always in active scope but is absent from `triage` while its own condition is Manual; a surfaced child is absent while an ancestor gate is closed. Undecided or orphaned Entities enter `triage` when all four conditions hold.

`Command` stores a shell string, not its observed result. Queries that need derived status can run it: exit 0 satisfies the condition, exit 1 does not, and other exits, signals, or spawn failures fail the Axon command. Results are shared within one invocation and reevaluated next time; satisfaction can revert without changing Progress, Disposition, or claim. Clearing or correcting the condition does not require successful evaluation. See `axon when command --help` for the execution contract.

## Information ownership

An Entity's plan declaration consists only of:

- title
- description
- parent Group
- outgoing dependencies

Progress, Disposition, Resurface condition, and claim are Control state. Decision and progress reasons belong to their typed history. `ready` and the other derived facts are observations, not declaration or state fields.

An `Undecided` Entity has a draft declaration that can be edited. An `Accepted` or `Rejected` Entity has a fixed declaration. To change a fixed declaration, return it to `Undecided`, edit and verify the complete draft, then apply the intended final Disposition separately. Preserve any reasons supplied for those transitions in typed history.

Initial creation may supply dependencies and a condition atomically. It preserves NotStarted without a claim and records initial values without fabricated transitions; Accepted creation captures the complete declaration in its first Revision, while Undecided creation has none. Initial Command strings are not executed for saving or confirmation.

Each accepted or rejected declaration is preserved as an immutable Declaration Revision. A Note is append-only supplemental information that does not change the declaration or Control state. Investigation results, implementation results, constraints learned later, and handoff details belong in Notes. Do not use description as an activity log, use a Note to simulate a state change, edit an old Note, or duplicate a state-change reason in a Note.

## Entity context

Before changing an existing Entity, read `axon show <id> --skip-command-evaluation` and inspect the saved fields relevant to the requested operation. Read Declaration Revisions, Notes, and typed history when the request changes or depends on the information they preserve. Evaluate derived conditions separately when the operation requires them. Do not infer missing context from a frontier listing.

Inspect related Entities when their state or declaration can change the operation's validity or a consequence the caller needs to understand. Use `axon list --skip-command-evaluation` when a complete saved-state inventory, including inactive Entities, is required; evaluate conditions separately only when the derived observation matters. `ready` and `triage` are frontiers, not complete inventories. Absence from `triage` does not mean an Entity is missing or its creation/update failed; do not repeat creation on that evidence. Use `axon show <id> --skip-command-evaluation` for saved state and then a normal derived query when readiness, surfacing, or active-scope evaluation is required. `status` summarizes plans, saved claims, candidates, and waits; it is not a complete inventory.

## Relationships and Groups

Dependencies are prerequisites. A dependency is satisfied when its target is `Ended` and not `Rejected`; a rejected target makes the dependent orphaned. `AfterEntity` is a schedule condition instead: an ended or rejected target makes the waiter surfaced.

A Group is an explicit plan Entity, not a tag. Starting it opens its activation gate but does not start descendants. A Group can be done only while `InProgress` and after every descendant is terminal. Releasing a Group requires zero `InProgress` descendants. Rejecting a Group makes the Group terminal and closes its active scope but does not mutate descendant state. Non-terminal descendants below it can remain as a stable inactive saved state; their visibility alone does not require rejection, release, or other cleanup. When a saved claim or its external work actually needs a disposition, report the observed claim and the available per-Entity choices without changing descendants automatically.

Do not automatically start descendants, finish an ancestor Group, move children, or rewrite dependencies as a side effect of another capability. Return those possible next operations to the calling workflow.

## Inspect without executing external conditions

Use `axon list --skip-command-evaluation` or `axon show <id> --skip-command-evaluation`
when only saved information is needed, or when a Command condition fails or does not finish.
These forms execute no Command, including ancestor, descendant, and related conditions.
`unevaluated` is a read-time observation, not false or a stored state. Other conditions
remain evaluable. This option can be combined with `--trace-conditions` but emits no
Command trace; it does not establish readiness for a lifecycle mutation. Normal reads
and lifecycle checks continue to evaluate conditions.

Use `--trace-conditions` only when the caller needs the evaluation evidence. It emits each executed shell string plus captured stdout and stderr without redaction or truncation; do not expose or persist sensitive output unnecessarily. An abnormal exit can fail the Axon invocation without producing a trace block.

### Literal saved-text search

`axon list --search <text>` searches the current title and description plus every Note body. It is case-sensitive literal matching: regex, `%`, and `_` have no special meaning, and actors, history, and Revisions are excluded. It combines with kind and saved-state filters before Command evaluation. Add `--skip-command-evaluation` for a pure saved-text search. Matched locations and stable Note IDs identify where to inspect the full content. Search can narrow candidates, but absence from one literal query does not prove semantic uniqueness.

### Inventory state filters

`axon list` without filters includes every Entity. Combine `--progress not-started|in-progress|ended`, `--disposition undecided|accepted|rejected`, `--terminal=true|false`, and `--kind issue|group` with AND; each option is accepted once. Terminal means Ended or Rejected (including their overlap). Omit `--terminal` to include both. Matches retain inactive and unsurfaced Entities: `--terminal=false` is not the ready/triage frontier or active scope. Empty matches succeed. Filters preserve saved state, history, claims, row format, and ordering.

For example, use `axon list --progress not-started --disposition accepted` for unstarted accepted plans, `axon list --disposition rejected` for rejected Entities, or `axon list --progress ended` for ended work. Saved-state filters run before row/Command evaluation; required ancestors of retained rows can still be evaluated. Add `--skip-command-evaluation` to prevent all Command execution.
