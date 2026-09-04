---
name: conventions
description: Axonの状態・情報モデルと安全なCLI操作の共通契約を提供する。他のaxon-kit skillと利用者workflowの基盤として使い、協業方針、作業選択、実装、commitは定めない。
---

# Axon operation contract

Use this skill as the shared foundation for every Axon capability. It defines what Axon data means and how an already-authorized operation is executed without losing history, duplicating effects, or mistaking an uncertain result for success.

## Keep the layers separate

- Axon defines Entity state, relationships, derived facts, and information ownership. It does not decide whether a human or an agent chooses work, makes a disposition decision, or reviews a result.
- The calling request or workflow supplies the target, intended effect, and authority for that effect. An `axon-kit` capability performs only that effect and returns control; it does not infer permission for later phases.
- Repository rules and host permissions remain in force. A workflow cannot use this kit to expand its external authority.
- Axon state synchronization does not prescribe implementation, testing, review, or commit behavior.

## Read the applicable contracts

Read [the model contract](references/model.md) before interpreting an Entity, changing its declaration or Control state, or reasoning about relationships and derived facts.

Read [the mutation contract](references/mutations.md) before any command that changes the Axon DB. Read-only inspection does not require that reference unless a previous mutation has an uncertain outcome.

For new Issue or Group creation, also read [the creation contract](references/creation.md).

## Prefer the installed CLI contract

Use `axon help` or the relevant `axon <command> --help` when command syntax or an input contract is uncertain. Do not compensate for a disagreement between the installed CLI, its documentation, and these instructions by inventing an alternative write path. Report the mismatch so the caller can decide which version or artifact is authoritative.

## Return a capability result

Report the target IDs, the requested effect, the observed final state, and any relevant frontier or relationship impact. For a mutation, classify the DB result as applied, not applied, partially completed, or unknown. Do not hide unfinished phases behind a general success message.
