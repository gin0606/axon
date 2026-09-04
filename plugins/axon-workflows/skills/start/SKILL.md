---
name: start
description: 指定されたAccepted Issueに着手し、実装、テスト、self-review、commit、doneまで完遂する。明示的な$axon-workflows:startまたは完全workflowの依頼に使い、単なる着手・実装依頼、無権限のIssue選択、Groupには使わない。
---

# Axon Issue 1件に着手して完了する

`axon-workflows:conventions`、`axon:conventions`、`axon:work-state`、必要に応じて`axon:register`、`axon:triage`、`axon-kit:add-note`を使う。個人用skillの`self-review`と`commit-conventions`も必須とする。必要な依存先が利用できない場合は、Axonまたはgitの状態を変更する前に停止し、不足しているskillを明示する。

明示的な`$axon-workflows:start <issue-id>`、またはIssue IDと実装、review、commit、doneまでの終端を指定した完全workflowの依頼は、このskillに列挙した操作の権限を与える。

## 対象を固定してclaimを取得する

対象はユーザーが指定したAccepted Issue、または明示的な上位workflowが与えた範囲内で選んだAccepted Issueに限る。Groupや権限のない別Issueを選ばない。対象の全情報、関係するDeclaration RevisionとNote、リポジトリの指示、現在のコードとworking treeを確認する。

- `ready`なら、成果物を編集する前に`axon:work-state`でstartする。
- すでにInProgressで、現在のactorとworktreeに整合するclaimがあれば、再度startせず再開する。
- 別のactorまたはworktreeがclaimを保持している場合は奪わず停止する。
- undecided、rejected、blocked、orphaned、inactive、endedで着手できない場合は、観測した状態を示す。着手するためだけに状態や関係を変更しない。

declarationが未解決のプロダクト判断、scope、公開契約、状態モデル、後戻りしにくい判断を要求する場合は、その判断をユーザーへ返す。通常の実装詳細は自律して決める。無関係なworking treeの変更を保持し、近くにあるというだけでscopeへ取り込まない。

## 実装して検証する

合意済みの目的、scope、完了条件を満たす設計を自律して選び、関連するコードと文書を調査して完全に実装する。リポジトリ固有の指示とskillに従い、変更に応じた検証を行う。

合意済みscopeを正確かつ実行可能に表すための親Group、dependency、分解は、`axon:register`または`axon:triage`で自律して整える。新しい目的、scope、完了条件、公開仕様、採用判断を加える場合はユーザーへ返す。

別Issueとして扱う必須の先行作業が判明した場合は、その構造と理由を記録するが、そのIssueへ着手しない。明示的な上位workflowが複数Issueの選択権限を与えている場合だけ継続できる。scope外で見つけた一般化可能な改善はUndecidedで記録できるが、勝手に着手しない。

完了の近道として、対象Issueの目的、scope、完了条件、Disposition、Resurface conditionを変更しない。後続セッションに必要な確定済みの結果や制約は、descriptionではなくNoteへ残す。

## セルフレビューする

固定したscopeに対して`self-review`を使う。findingを検証し、妥当なscope内の修正を自律して適用し、重大なfindingがなくなるまで必要な検証と再確認を行う。findingの解決が目的、scope、公開仕様、完了条件を変える場合はユーザーへ返す。

self-review、必須の検証、採用したfindingの修正が完了するまでcommitまたはdoneへ進まない。

## コミットして完了する

`git commit`の前に`commit-conventions`を使う。このworkflowの明示的な呼び出しを、対象Issueに属する変更のcommit指示として扱う。main上でも同様だが、固定したscopeに属する変更だけをcommitする。無関係な変更を安全に分離できない場合は判断を返す。空commitは作らない。

commit後、`axon-kit:add-note`で重要な実装結果、commit ID、検証、self-review結果を、状態変更reasonと重複しない一件のNoteへ記録する。続いて`axon:work-state`でdoneにし、Entityの最終状態と影響を受けたfrontierを確認する。

既存実装がすでにdeclarationを満たしている場合は、必要な検証とself-reviewを行い、空commitを作らず結果Noteとdoneへ進む。完了条件が主観的、未定義、または一部未達ならdoneにせずユーザーへ返す。

## 中断と失敗を扱う

検証に失敗した成果やreview未完了の成果をcommitせず、Issueをdoneにしない。安全でscope内にあるworking treeの変更は保持する。

同じ会話でユーザー判断を待つ間はclaimを維持する。明示的な保留、引き継ぎ、または長期の外部待ちでは、重要な現在地、完了済み作業、残作業、検証結果、blockerを一件のNoteへ記録し、確認後にreason付きでreleaseする。

Noteの保存に失敗した場合やDB上の結果が不明な場合はreleaseせず、現在のclaimと照合情報を報告する。releaseに失敗した場合は同じNoteを再追加しない。安全な同一操作の再試行は自律して行い、rollback、補償操作、別の最終状態が必要な場合は判断を返す。

Issue ID、Axonの最終状態、作成した場合はcommit ID、検証とself-reviewの結果、Note番号、未解決事項を報告する。このskillはpush、Pull Request、release、別Issueへの着手を行わない。
