---
name: triage
description: 既存Entityを調査し、確定した判断・計画・関係・条件をAxon CLIで反映する。
---

# 調査と反映

`axon:conventions` と `axon-kit:triage` を使う。show --details・logと必要なNote・祖先・関係先から判断材料を確認する。供給された採用・撤回・取りやめ・再検討と、合意範囲の本文・構造・条件変更だけを反映する。目的や採用判断が不足なら呼び出し側へ返す。状態操作のための新しい目的や別Issueを創作しない。各作用と波及を照合し、調査だけの依頼からmutationへ進まない。
