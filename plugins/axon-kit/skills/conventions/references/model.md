# Axon model contract

Read this reference when an operation needs to interpret or change Entity data.

## Entity and state axes

Axon has two Entity kinds, Issue and Group. Both have a stable ID and the same three independent state axes:

- Progress: `NotStarted`, `InProgress`, or `Ended`
- Disposition: `Undecided`, `Accepted`, or `Rejected`
- Resurface condition: `Always`, `AtDate`, `AfterEntity`, `Manual`, or `Command`

Do not use one axis as a proxy for another. A command that changes Progress must not silently change Disposition or Resurface condition, and the reverse also applies.

An Entity is terminal when its Progress is `Ended` or its Disposition is `Rejected`. `ready`, `blocked`, `orphaned`, `surfaced`, `terminal`, active scope, blocking causes, and Group completion facts are derived from stored state and relationships; do not treat them as independently editable data.

`ready` means that an Entity is in active scope, `NotStarted`, `Accepted`, surfaced, not blocked, and not orphaned. `triage` is the active, non-terminal decision frontier: an Entity is there when it is `Undecided` or orphaned. These definitions do not assign the decision or work to a human or an agent.

`Manual` remains unsurfaced until explicitly replaced or cleared with `axon when clear`. It has no payload and changes neither Progress, Disposition, nor claim. A Manual Group closes descendant active scope without changing descendant state.

`Command` stores a shell string, not its observed result. Queries that need derived status can run it: exit 0 satisfies the condition, exit 1 does not, and other exits, signals, or spawn failures fail the Axon command. Results are shared within one invocation and reevaluated next time; satisfaction can revert without changing Progress, Disposition, or claim. Clearing or correcting the condition does not require successful evaluation. See `axon when command --help` for the execution contract.

## Information ownership

An Entity's plan declaration consists only of:

- title
- description
- parent Group
- outgoing dependencies

Progress, Disposition, Resurface condition, and claim are Control state. Decision and progress reasons belong to their typed history. `ready` and the other derived facts are observations, not declaration or state fields.

An `Undecided` Entity has a draft declaration that can be edited. An `Accepted` or `Rejected` Entity has a fixed declaration. To change a fixed declaration, return it to `Undecided`, edit and verify the complete draft, then apply the intended final Disposition separately. Preserve any reasons supplied for those transitions in typed history.

Each accepted or rejected declaration is preserved as an immutable Declaration Revision. A Note is append-only supplemental information that does not change the declaration or Control state. Investigation results, implementation results, constraints learned later, and handoff details belong in Notes. Do not use description as an activity log, use a Note to simulate a state change, edit an old Note, or duplicate a state-change reason in a Note.

## Entity context

Before changing an existing Entity, read `axon show <id>` and inspect the fields relevant to the requested operation. Read Declaration Revisions, Notes, and typed history when the request changes or depends on the information they preserve. Do not infer missing context from a frontier listing.

Inspect related Entities when their state or declaration can change the operation's validity or a consequence the caller needs to understand. Use `axon list` only when a complete inventory, including inactive Entities, is required; `ready` and `triage` are frontiers, not complete inventories.

## Relationships and Groups

Dependencies are prerequisites. A dependency is satisfied when its target is `Ended` and not `Rejected`; a rejected target makes the dependent orphaned. `AfterEntity` is a schedule condition instead: an ended or rejected target makes the waiter surfaced.

A Group is an explicit plan Entity, not a tag. Starting it opens its activation gate but does not start descendants. A Group can be done only while `InProgress` and after every descendant is terminal. Releasing a Group requires zero `InProgress` descendants. Rejecting a Group closes its active scope but does not mutate descendant state.

Do not automatically start descendants, finish an ancestor Group, move children, or rewrite dependencies as a side effect of another capability. Return those possible next operations to the calling workflow.
