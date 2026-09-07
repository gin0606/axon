---
name: merge
description: Prepare, resolve, validate, and publish a three-way Axon file-backend snapshot merge. Use for axon merge workspaces and file-backend conflicts; not for declaration YAML merging or ordinary Git merge/rebase completion.
---

# Merge Axon file snapshots safely

Use `axon-kit:conventions` and read its mutation contract. This capability owns Axon's merge workspace and candidate publication, not the surrounding Git merge, staging, commit, rebase, or conflict policy.

For prepare, resolution, check, apply, Git-driver recovery, or an uncertain result, read [the merge workflow](references/workflow.md) completely.

## Keep Git integration separate

Driver registration uses ordinary Git config after installation; `axon merge setup` is not provided. File init supplies the attribute and ignore rules. A merge request alone does not authorize changing Git configuration. When separately authorized, register `merge.axon.driver` as `axon merge driver %O %A %B` and `merge.axon.recursive` as `binary` in the repository. Axon must be on PATH; clones need their own registration. Global configuration is optional, not required.

Do not invoke `axon merge driver` as a manual merge command. Git owns its `%O`, `%A`, and `%B` arguments and the driver's temporary output path.

## Return

Return the operation mode, workspace and output paths, input and candidate digests, unresolved conflicts or applied repairs, validation and storage-result classifications, changed artifacts, and the exact Git work that remains outside this capability.
