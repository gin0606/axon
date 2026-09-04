---
name: declaration
description: Axon declaration artifactを、ユーザー所有ファイルの保護、Control stateとの段階的な組み合わせ、競合時の再調整まで含めて扱う個人用ワークフロー。単純なread-only checkには公式kitだけを使う。
---

# Axon declarationを個人用方針で扱う

`axon:conventions`、`axon-kit:declaration`、必要な内容判断に応じて`axon:register`または`axon:triage`を使う。

[詳細手順](references/workflow.md)を最後まで読み、宣言のreview、canonicalize、export、apply、Control state変更との組み合わせ、競合または部分完了を扱う。

このskillはユーザー所有artifactを保護し、複数phaseの順序と引き渡しを定める個人用ワークフローである。Axonの保存モデルや`axon import`自体の契約は`axon-kit:declaration`を正とする。
