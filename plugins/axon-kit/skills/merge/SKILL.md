---
name: merge
description: Prepare, resolve, validate, and publish a three-way Axon file-backend snapshot merge, or explicitly configure its Git merge driver. Use for axon merge workspaces and file-backend conflicts; not for declaration YAML merging or ordinary Git merge/rebase completion.
---

# Merge Axon file snapshots safely

Use `axon-kit:conventions` and read its mutation contract. This capability owns Axon's merge workspace and candidate publication, not the surrounding Git merge, staging, commit, rebase, or conflict policy.

For `merge setup`, follow the setup boundary below. For prepare, resolution, check, apply, Git-driver recovery, or an uncertain result, read [the merge workflow](references/workflow.md) completely.

## Configure Git integration explicitly

Run `axon merge setup` only when the caller authorizes repository-local Git configuration and `.gitattributes` changes. It records the current executable's absolute path as the merge driver, adds the state-path attribute, and ensures the merge workspace is ignored. It does not initialize file storage, stage, or commit files. Verify the Git configuration, attribute, ignore entry, and changed artifacts; clones and other repositories require their own setup.

Do not invoke `axon merge driver` as a manual merge command. Git owns its `%O`, `%A`, and `%B` arguments and the driver's temporary output path.

## Return

Return the operation mode, workspace and output paths, input and candidate digests, unresolved conflicts or applied repairs, validation and storage-result classifications, changed artifacts, and the exact Git work that remains outside this capability.
