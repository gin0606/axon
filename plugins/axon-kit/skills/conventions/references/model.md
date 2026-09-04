# Axon model contract

Read this reference when an operation needs to interpret or change Entity data.

## Entity and state axes

Axon has two Entity kinds, Issue and Group. Both have a stable ID and the same three independent state axes:

- Progress: `NotStarted`, `InProgress`, or `Ended`
- Disposition: `Undecided`, `Accepted`, or `Rejected`
- Resurface condition: `Always`, `AtDate`, or `AfterEntity`

Do not use one axis as a proxy for another. A command that changes Progress must not silently change Disposition or Resurface condition, and the reverse also applies.

An Entity is terminal when its Progress is `Ended` or its Disposition is `Rejected`. `ready`, `blocked`, `orphaned`, `surfaced`, `terminal`, active scope, blocking causes, and Group completion facts are derived from stored state and relationships; do not treat them as independently editable data.

`ready` means that an Entity is in active scope, `NotStarted`, `Accepted`, surfaced, not blocked, and not orphaned. `triage` is the active, non-terminal decision frontier: an Entity is there when it is `Undecided` or orphaned. These definitions do not assign the decision or work to a human or an agent.

## Information ownership

An Entity's plan declaration consists only of:

- title
- description
- parent Group
- outgoing dependencies

Progress, Disposition, Resurface condition, and claim are Control state. Decision and progress reasons belong to their typed history. `ready` and the other derived facts are observations, not declaration or state fields.

An `Undecided` Entity has a draft declaration that can be edited. An `Accepted` or `Rejected` Entity has a fixed declaration. To change a fixed declaration, return it to `Undecided` with a reason, edit and verify the complete draft, then apply the intended final disposition with a separate reason.

Each accepted or rejected declaration is preserved as an immutable Declaration Revision. A Note is append-only supplemental information that does not change the declaration or Control state. Investigation results, implementation results, constraints learned later, and handoff details belong in Notes. Do not use description as an activity log, use a Note to simulate a state change, edit an old Note, or duplicate a state-change reason in a Note.

## Entity context

Before changing an existing Entity, read the complete `axon show <id>` output, including kind, declaration, Control state, record counts, relationships, Group facts, and every displayed Note. If Declaration Revisions exist, read `axon revision list <id>` and every `axon revision show <id> <number>`. Do not discard older Revisions or Notes merely because they are old.

Inspect related Entities only when their state or declaration can change the requested operation or its consequences. Use `axon list` when all Entities, including inactive ones, are required; `ready` and `triage` are frontiers, not complete inventories.

## Relationships and Groups

Dependencies are prerequisites. A dependency is satisfied when its target is `Ended` and not `Rejected`; a rejected target makes the dependent orphaned. `AfterEntity` is a schedule condition instead: an ended or rejected target makes the waiter surfaced.

A Group is an explicit plan Entity, not a tag. Starting it opens its activation gate but does not start descendants. A Group can be done only while `InProgress` and after every descendant is terminal. Releasing a Group requires zero `InProgress` descendants. Rejecting a Group closes its active scope but does not mutate descendant state.

Do not automatically start descendants, finish an ancestor Group, move children, or rewrite dependencies as a side effect of another capability. Return those possible next operations to the calling workflow.
