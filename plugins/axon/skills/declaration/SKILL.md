---
name: declaration
description: Axon declaration artifactを、依頼されたfileまたはactive storageへの反映範囲、ユーザー所有fileの保護、Control stateとの段階操作、競合時の再調整まで含めて扱う個人用ワークフロー。単純なread-only checkには公式kitだけを使う。
---

# Axon declarationを個人用方針で扱う

`axon:conventions`、`axon-kit:declaration`、必要な内容判断に応じて`axon:register`または`axon:triage`を使う。

[詳細手順](references/workflow.md)を最後まで読み、review、export、canonicalize、check、apply、Control state変更、競合または部分完了を扱う。

このskillはユーザー所有artifactの保護と、複数phaseにまたがる自律実行の境界を定める。Axonの保存モデル、strict YAML、`axon export`と`axon import`の契約は`axon-kit:declaration`を正とする。
