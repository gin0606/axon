---
name: plan
description: Axonに採用判断と目的・完了条件が確定したIssueまたはGroupを未着手として登録する。
---

# 採用済み計画を登録する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 [作成契約](../conventions/references/creation.md)に従う。Issue・Groupの採用判断と目的・完了条件が供給済みなら `axon capture --accept --label ... --title ... -m ...` で`NotStarted`として登録する。採用判断が供給されていなければ登録せず呼び出し側へ返し、未判断として残す依頼は `axon-kit:capture` が扱う。`--kind` には供給されたkind (`issue` / `group`)、`--label` には供給されたlabelを明示し、どちらかが供給されていなければ既定や推測に頼らず呼び出し側へ返す。必要な関係を初期入力に含め、保存後にkind・label・lifecycle・本文・関係・条件を照合する。

登録は着手を含まず、親Groupの保存値も変えない。採用済みで登録しても、`Undecided`の祖先や未完了の依存先（自身・祖先）があれば、そのIssueは着手できない。最終確認待ちのGroupへ未終了の子を加えると、そのGroupは`Complete`できなくなる。これらの影響は保存後の照合結果として呼び出し側へ返す。
