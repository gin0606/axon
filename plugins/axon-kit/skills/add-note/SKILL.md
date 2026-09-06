---
name: add-note
description: Append supplemental information to an existing Issue or Group as an immutable Note without changing its declaration or Control state. Use for a requested Note, established result, correction, or handoff; not for declaration edits, state changes, or new Entities.
---

# Append one Axon Note

Use `axon-kit:conventions`. Read its model and mutation contracts before adding a Note.

## Accept only supplemental information

Read the target context needed to classify the information. A Note may record investigation or implementation results, constraints learned later, a correction, or a useful handoff. It must not change what the Entity is or its current Control state.

- Route title, description, parent, or outgoing dependency changes to `axon-kit:triage`.
- Route Progress, Disposition, Resurface condition, or claim changes to the capability that owns that state.
- Keep a state-change reason in typed history; do not repeat it as a Note.
- Correct an old Note by appending a new Note that identifies the original by its stable ID. Never edit or delete the original.

The calling request or workflow supplies the target and Note content. It may ask this capability to phrase established facts, but this capability does not invent a decision or choose a target. Return `input required` if the target, information class, or represented decision is unresolved.

## Freeze the append input

Fix the exact Note body and observe the actor label before the first append attempt. When using `-F` or stdin, preserve the bytes and record their digest as required by the mutation contract.

Read `axon note list <id>` immediately before the append and record the count and complete set of stable IDs, using an empty set when no Notes exist.

## Append once and verify

Run `axon note add <id> -m <body>` or the equivalent `-F <snapshot>` as one standalone mutation.

On observed success, record the returned Note ID and never repeat the append. Read `axon note show <id> <note-id>` to verify the body, actor, and timestamp.

If the append clearly failed, do not retry without resolving its cause. If its outcome is unknown:

1. Confirm the original process ended.
2. List every Note ID absent from the recorded pre-append set; ID order is not creation order.
3. Read each candidate and compare both frozen body and actor.
4. Treat one match as success and multiple matches as `DB applied: unknown`; preserve unrelated concurrent Notes.
5. Only when every later Note was observed and none matches may the same frozen append be retried once. Reconcile that attempt by the same rule and do not retry again.

Return the Note ID and summary and report concurrent Notes separately. Retain the input snapshot when the DB outcome remains unknown.
