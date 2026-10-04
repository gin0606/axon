---
name: triage
description: Axonの既存Entityを調査し、ユーザーの判断と合意済み計画から変更内容を整理・反映する協業ワークフロー。
---

# 調査と反映

`axon:conventions` と `axon-kit:triage` を使う。`axon show ID --details --skip-conditions`と関係する祖先・関係先を読み、補足の選択は`axon:conventions`の「本文を入口に読む」に従う。供給された採用・撤回・取りやめ・再検討・`Completed`から`NotStarted`への`Reopen`と、合意範囲の本文・label・構造・種類・条件変更だけを反映する。合意範囲の変更で目的や完了条件が変わり、`axon:conventions`の基準で選ぶlabelも変わる場合は、あわせて`axon label set`で反映する。目的や採用判断が不足なら呼び出し側へ返す。`Reopen`が`Completed`の依存元や祖先の採否で拒否された場合、それらを戻すか採用するかは新しい判断として返す。Entityの目的は依頼と確定情報からのみ構成するため、状態操作のための新しい目的や別Issueを創作しない。各作用と波及を照合し、調査だけの依頼からmutationへ進まない。
