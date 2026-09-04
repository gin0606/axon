---
name: add-note
description: 既存IssueまたはGroupへ、declarationやControl stateを変えない追加情報を追記専用Noteとして追加する。明示されたNote、重要な結果、申し送りに使い、declaration編集、状態変更、新規Entityには使わない。
---

# Append one Axon Note

Use `axon-kit:conventions`. Read its model and mutation contracts before adding a Note.

## Accept only supplemental information

Read the complete Entity context. A Note may record investigation or implementation results, constraints learned later, or a useful handoff. It must not change what the Entity is or its current Control state.

- Route title, description, parent, or outgoing dependency changes to `axon-kit:triage`.
- Route Progress, Disposition, Resurface condition, or claim changes to the capability that owns that state.
- Keep a state-change reason in typed history; do not repeat it as a Note.
- Do not add routine progress narration, a generic start or completion message, or content already present in another Note.
- Correct an old Note by appending a new Note that identifies the original by its Entity-local number. Never edit or delete the original.

The calling request or workflow supplies the target and Note content. It may ask this capability to phrase established facts, but this capability does not invent a disposition decision or choose a target. Return `input required` if classification, target, or a represented decision is unresolved.

## Freeze the append input

Fix the exact Note body and actor label before the first append attempt. When using `-F` or stdin, freeze the bytes in an agent-owned, read-only snapshot and record its digest as required by the mutation contract.

Read `axon note list <id>` immediately before the append and record the count and latest Entity-local number, using zero when no Notes exist.

## Append once and verify

Run `axon note add <id> -m <body>` or the equivalent `-F <snapshot>` as one standalone mutation.

On observed success, record the returned Note number and never repeat the append. Read `axon note show <id> <number>` to verify body, actor, and timestamp, then read the complete `axon show <id>` output and confirm that the declaration, Control state, and relationships did not change.

If the append clearly failed, do not retry without resolving its cause. If its outcome is unknown:

1. Confirm the original process ended.
2. List every Note number added after the recorded pre-append boundary.
3. Read each candidate and compare both frozen body and actor.
4. Treat one match as success and multiple matches as `DB applied: unknown`; preserve unrelated concurrent Notes.
5. Only when every later Note was observed and none matches may the same frozen append be retried once. Reconcile that attempt by the same rule and do not retry again.

Return the Note number and summary, confirm that no other information class changed, and report concurrent Notes separately. Retain any snapshot when the DB outcome remains unknown.
