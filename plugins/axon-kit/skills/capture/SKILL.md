---
name: capture
description: Axonに未判断の懸念を新しいIssueまたはGroupとして登録する。採用や実装には使わない。
---

# 未判断を残す

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 [作成契約](../conventions/references/creation.md)に従う。与えられた懸念の目的と境界を保ち、`axon capture --title ... -m ...` で`Undecided`として登録する。`--kind` には供給されたkind (`issue` / `group`) を明示し、供給されていなければ既定に頼らず呼び出し側へ返す。`--accept` は付けない。採用済みとして登録する依頼は `axon-kit:plan` が扱う。返されたIDのkind・lifecycle・本文・関係・条件を照合する。
