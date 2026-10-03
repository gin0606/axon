---
name: start
description: 指定された採用済みのAxon Issueを、実装・検証・完了記録・commitまで進める。Group配下のIssueも直接指定できる。commitの扱いはユーザーの指示と利用先の規約に従う。
---

# Issueを完遂する

最初に[実行の共通手順](../../references/execution.md)を読む。明示的な`axon-workflow:start`の呼び出し、または対象Issueと実装からcommitまでを指定した依頼を、このworkflowの実行権限とする。`axon-workflow:start-group`が選んだIssueも同じ手順で扱う。

1. **対象を確認する。** 指定Issue（Group配下も含む）と全祖先Groupの本文を読み、実装範囲と完了条件を確認する。祖先は採用済みで、自身と祖先の依存先が完了していることを確認する。未判断・終了済みの対象は変更せず状態を報告する。`NotStarted`なら`axon:work-state`で`axon start`する。
2. **実装して検証する。** 対象の目的と完了条件を満たす変更と検証を行う。
3. **記録して完了する。** 重要な結果を`axon-kit:add-note`で記録してから、`axon:work-state`で対象Issueに`axon complete`を実行する。
4. **commitして報告する。** 共通手順に従って対象の変更をcommitする。Issueの最終状態、検証結果、commit、残った問題を報告する。
