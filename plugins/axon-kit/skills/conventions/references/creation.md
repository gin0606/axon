# Entity creation contract

Read this reference before creating a new Issue or Group.

## Check identity before allocation

Inspect `axon list` and the relevant frontier. For every plausible duplicate, read the full Entity context and all Declaration Revisions.

- If an unfinished Entity has the same purpose, scope, completion conditions, kind, and structural role, do not create another. Compare its actual Disposition and Progress with the outcome requested by the calling capability. If they are compatible, return that ID as reused with its actual Control state. If they differ, return the candidate for the capability that owns the required transition; do not call the request complete or allocate a replacement.
- Kind alone neither proves nor disproves duplication. When cross-kind candidates have the same effective scope, the calling workflow must choose whether to reuse, restructure, or create separately.
- If a completed or rejected Entity addressed the same work, return its history. Create a new Entity only when the caller's supplied decision distinguishes new work from reconsidering the old decision.
- Partial overlap with a different role does not block creation, but return the related IDs. Do not add a parent or dependency unless it is part of the requested declaration.

## Write a durable declaration

Write the title and optional description so a later session can understand the Entity from `axon show` without the creating conversation. An Issue title identifies the problem or work; a Group title identifies the plan boundary. The declaration should make the reason for the work and its completion condition recoverable without forcing a template when the title already carries them.

Keep observations and proposed solutions distinct. Do not reserve description for future progress updates; later findings and handoffs are Notes.

## Reconcile uncertain creation

If creation output or the allocated ID is lost, inspect current Entities for the exact intended declaration and actor context before retrying. Never create a second Entity merely because the first command result was not observed. If one matching Entity can be established, use it. If zero or multiple candidates remain possible, report the ambiguity instead of guessing.
