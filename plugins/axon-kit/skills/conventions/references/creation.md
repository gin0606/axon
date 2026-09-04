# Entity creation contract

Read this reference before creating a new Issue or Group.

## Use the caller's identity decision

Axon does not define semantic uniqueness for Entities. The calling request or workflow decides whether an existing Entity should be reused and supplies the intended kind, declaration, parent, and outgoing dependencies. Do not perform a repository-wide duplicate policy unless the caller requires one.

Write the title and optional description so the Entity remains understandable without the creating conversation. An Issue describes one concern or work item; a Group describes an explicit plan boundary. Later findings and handoffs belong in Notes rather than the description.

## Treat additions as non-idempotent

`plan`, `capture`, `group plan`, and `group capture` allocate a new Entity every time. Record the exact creation payload and observe the returned ID before continuing with later phases.

If the command's outcome or allocated ID is unknown, first confirm that the process ended. Inspect `axon list` and candidate Entities for the exact intended kind, declaration, parent, actor context, and creation timing. Treat one identifiable new match as the created Entity. If no match exists, the same frozen creation may be retried once; if multiple matches remain possible, report the outcome as unknown and do not create another Entity.
