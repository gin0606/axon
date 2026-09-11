---
name: conventions
description: 単一lifecycle版Axonの状態・情報と安全なCLI操作の共通契約。
---

# Axon操作契約

このcheckoutの新lifecycle CLI向け。呼び出し元が与えたbinaryの絶対パスと保存先を固定する。`--help` に `tasks / accept / cancel` があることを照合する。PATH上の旧binaryや既存sessionにロード済みの旧skillへ混ぜない。導入は [利用ガイド](../../../../docs/guide/getting-started.md)。

Entityを解釈するときは [モデル](references/model.md)、mutation前には [保存操作](references/mutations.md)、新規登録では [作成](references/creation.md) を読む。

対象、採用判断、実装やcommitの権限は呼び出し側が決める。CLIの操作可能性はそれらの権限を与えない。状態・関係・付随情報の意味はAxonが定義する。

構文が不確かなら選択したbinaryのhelpを確認する。不一致を独自の保存経路で補わず、観測内容を呼び出し側へ返す。要求した作用、最終状態、保存結果（適用済み・未適用・部分適用・不明）と関係への影響を報告する。
