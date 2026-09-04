---
name: start
description: 指定されたAccepted Issueに着手し、実装、テスト、self-review、commit、doneまで完遂する。明示的な$axon:startまたは完全workflowの依頼に使い、単なる着手・実装依頼、Issueの自律選択、Groupには使わない。
---

# Start and finish one Axon Issue

This is an opinionated personal workflow. It composes Axon capabilities with implementation, review, and commit policy; it is not part of Axon's state model.

Require `axon-kit:conventions`, `axon-kit:work-state`, and `axon-kit:add-note`. Require the personal `self-review` and `commit-conventions` skills. If any dependency is unavailable, stop before changing Axon or git state and name the missing skill; do not reproduce its rules locally.

Explicit `$axon:start <issue-id>` invocation, or an express request for this full workflow with an Issue ID, authorizes the normal path through commit and Axon completion for that Issue. Repository instructions and host permissions still apply, and unrelated external effects remain outside scope.

## Establish scope and claim

Use the official Axon conventions to read the complete Issue context and all Declaration Revisions. Do not accept a Group or choose another Issue.

- If it is `ready`, use `axon-kit:work-state` to start it before editing files.
- If it is already `InProgress` with a compatible claim for the current actor and worktree, resume from its Notes and current repository state without starting it again.
- If another actor or worktree owns the claim, or the Issue is undecided, rejected, blocked, orphaned, inactive, or ended, stop with the observed state. Do not change Disposition, relationships, schedule, or another claim to make it startable.
- If the declaration calls for an unresolved product, scope, public-contract, state-model, or hard-to-reverse decision that is not represented as a prerequisite, return the missing decision instead of silently choosing it. Ordinary implementation details are not a reason to stop.

Freeze the task scope from the Issue declaration, relevant Notes and Revisions, repository instructions, and current code. Preserve unrelated working-tree changes and do not absorb adjacent work merely because it is nearby.

## Implement and validate

Inspect the relevant code and documentation, implement the complete declared outcome, and run validation proportional to the change. Follow repository-specific skills and commands when they apply.

Do not alter the Issue declaration, Disposition, dependencies, or schedule as an implementation shortcut. If implementation discovers a material result or constraint that later sessions need, retain it for the result or handoff Note rather than editing description.

## Self-review

Use `self-review` on the fixed task scope. Resolve every finding through that skill's disposition process, apply accepted in-scope fixes, and complete its required verification loop.

Do not commit or mark the Issue done while self-review is incomplete, has failed to produce a required result, or leaves an accepted material finding, a required decision, or required investigation unresolved.

## Commit and complete

Use `commit-conventions` before `git commit`. Treat this explicit workflow invocation as the user's commit instruction for the Issue, including on a repository branch whose policy otherwise requires explicit commit authorization. Commit only changes belonging to the fixed task scope and never create an empty commit.

After a successful commit, use `axon-kit:add-note` to record the material implementation result, commit ID, validation, and self-review outcome without duplicating a state-change reason. Then use `axon-kit:work-state` to mark the Issue done. Verify the final Entity state and all affected frontiers.

If the Issue was already satisfied and no repository change is required, do not create an empty commit. Record the verified no-change result when it is useful, then mark the Issue done only if its declaration is actually fulfilled.

## Failure and handoff

If the workflow cannot complete after the claim was acquired:

1. Do not commit an unreviewed or failing result and do not mark the Issue done.
2. Preserve safe in-scope working-tree changes; do not discard work automatically.
3. Use `axon-kit:add-note` to record only the material current state, completed work, remaining work, validation, and blocker.
4. Use `axon-kit:work-state` to release the Issue with a reason after the Note is verified.

If Note recording fails or its DB outcome is unknown, do not release; report the current claim and reconciliation data. If release fails, do not append the Note again.

Return the Issue ID, final Axon state, commit ID when created, validation and self-review results, Note number, and any unresolved blocker.
