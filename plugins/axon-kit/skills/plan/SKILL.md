---
name: plan
description: 採用済みの目的・完了条件を持つIssueまたはGroupを未着手として登録する。
---

# 採用済み計画を登録する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 [作成契約](../conventions/references/creation.md)に従う。採用判断と目的・完了条件が供給済みなら `capture --accept --title ... -m ...` でNotStartedとして登録する。採用判断が供給されていなければ登録せず呼び出し側へ返し、未判断として残す依頼は `axon-kit:capture` が扱う。`--kind` には供給されたkind (`issue` / `group`) を明示し、供給されていなければ既定に頼らず呼び出し側へ返す。必要な関係を初期入力に含め、保存後にkind・lifecycle・本文・関係・条件を照合する。採用判断や実装を創作しない。
