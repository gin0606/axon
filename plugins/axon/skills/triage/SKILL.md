---
name: triage
description: Axonの既存Entityを調査し、ユーザーの判断と合意済み計画から変更内容を整理・反映する協業ワークフロー。
---

# 調査と反映

`axon:conventions` と `axon-kit:triage` を使う。`axon show ID --details --skip-conditions`・logと必要なNote・祖先・関係先から判断材料を確認する。供給された採用・撤回・取りやめ・再検討・`Completed`から`NotStarted`への`Reopen`と、合意範囲の本文・構造・条件変更だけを反映する。目的や採用判断が不足なら呼び出し側へ返す。`Reopen`が`Completed`の依存元や祖先の採否で拒否された場合、それらを戻すか採用するかは新しい判断として返す。Entityの目的は依頼と確定情報からのみ構成するため、状態操作のための新しい目的や別Issueを創作しない。各作用と波及を照合し、調査だけの依頼からmutationへ進まない。
