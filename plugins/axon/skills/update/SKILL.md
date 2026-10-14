---
name: update
description: Axonの既存Entityを調査し、ユーザーの判断と合意済み計画から変更内容を整理・反映する協業ワークフロー。単一または互いに関係のないEntityの個別の変更、`Start`・`Release`・`Complete`を除くlifecycle遷移、種類の変換、再浮上条件を扱う。変更前後のいずれかで同じGroupに含まれる（Group自身と全子孫）、またはdependencyでつながる複数Entityのtitle・description・label・親・dependencyをまとめて変える依頼には、依頼が個別の変更を明示しない限り`axon:declaration`を使う。
---

# 調査と反映

`axon:conventions` と `axon-kit:update` を使う。`axon show ID --details --skip-conditions`と関係する祖先・関係先を読み、補足の選択は`axon:conventions`の「本文を入口に読む」に従う。供給された採用・撤回・取りやめ・再検討・`Completed`から`NotStarted`への`Reopen`と、合意範囲の本文・label・構造・種類・条件変更だけを反映する。合意範囲の変更で目的や完了条件が変わり、`axon:conventions`の基準で選ぶlabelも変わる場合は、あわせて`axon label set`で反映する。目的や採用判断が不足なら呼び出し側へ返す。`Reopen`が`Completed`の依存元や祖先の採否で拒否された場合、それらを戻すか採用するかは新しい判断として返す。Entityの目的は依頼と確定情報からのみ構成するため、状態操作のための新しい目的や別Issueを創作しない。各作用と波及を照合し、調査だけの依頼からmutationへ進まない。

declarationでの変更とここで扱う操作の両方を含む依頼では、操作の順序を`axon:declaration`に従う。
