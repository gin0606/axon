---
name: start
description: 指定されたAccepted Issueに着手し、実装、テスト、self-review、commit、doneまで完遂する。明示的な$axon:startまたは完全workflowの依頼に使い、単なる着手・実装依頼、Issueの自律選択、Groupには使わない。
---

# Axon Issue 1件に着手して完了する

これは、方針を明確に定めた個人用ワークフローである。Axonの機能と、実装・review・commitの方針を組み合わせるものであり、Axonの状態モデルの一部ではない。

`axon-kit:conventions`、`axon-kit:work-state`、`axon-kit:add-note`を必須とする。個人用skillの`self-review`と`commit-conventions`も必須とする。いずれかの依存先が利用できない場合は、Axonまたはgitの状態を変更する前に停止し、利用できないskillを明示する。その規約をこのskill内で再現してはならない。

明示的な`$axon:start <issue-id>`の呼び出し、またはIssue IDを指定したこの完全ワークフローの明示的な依頼は、そのIssueについてcommitとAxon上の完了まで通常経路を進める権限を与える。リポジトリの指示とホスト側の権限制約は引き続き適用し、無関係な外部への作用は対象外とする。

## 対象範囲を確定してclaimを取得する

Axon公式の規約に従って、Issueの全情報とすべてのDeclaration Revisionを読む。Groupを対象にしたり、別のIssueを選んだりしてはならない。

- `ready`なら、ファイルを編集する前に`axon-kit:work-state`を使ってstartする。
- すでに`InProgress`で、現在のactorとworktreeに整合するclaimがあるなら、再度startせず、Notesと現在のリポジトリ状態を基に再開する。
- 別のactorまたはworktreeがclaimを保持している場合、あるいはIssueがundecided、rejected、blocked、orphaned、inactive、endedのいずれかである場合は、観測した状態を示して停止する。start可能にするためにDisposition、relationship、schedule、別のclaimを変更してはならない。
- declarationが、前提条件として表現されていない未解決のプロダクト、対象範囲、公開契約、状態モデル、または後戻りしにくい判断を要求する場合は、暗黙に判断せず、不足している判断を示して終了する。通常の実装詳細は停止理由にしない。

Issueのdeclaration、関連するNotesとRevisions、リポジトリの指示、現在のコードに基づいて作業範囲を固定する。無関係なworking treeの変更を保持し、近くにあるというだけで周辺作業を取り込んではならない。

## 実装して検証する

関連するコードとドキュメントを調査し、declarationで定められた成果を完全に実装して、変更に応じた検証を行う。リポジトリ固有のskillやコマンドが適用される場合は、それらに従う。

実装上の近道として、Issueのdeclaration、Disposition、dependencies、scheduleを変更してはならない。実装中に、後続セッションに必要な重要な結果や制約が判明した場合は、descriptionを編集せず、結果または引き継ぎのNoteに残す。

## Self-reviewを行う

固定した作業範囲に対して`self-review`を使う。すべてのfindingをそのskillの採否判定手順で解決し、採用した範囲内の修正を適用して、必須の検証サイクルを完了する。

self-reviewが未完了、必須の結果を得られていない、または採用した重要なfinding、必須の判断、必須の調査が未解決である間は、commitもIssueのdoneも行ってはならない。

## Commitして完了する

`git commit`の前に`commit-conventions`を使う。このworkflowの明示的な呼び出しを、そのIssueに対するユーザーからのcommit指示として扱う。これは、通常は明示的なcommit許可を必要とする方針のリポジトリbranchでも同様である。固定した作業範囲に属する変更だけをcommitし、空commitは決して作らない。

commitに成功した後、`axon-kit:add-note`を使い、状態変更のreasonと重複させずに、重要な実装結果、commit ID、検証、self-reviewの結果を記録する。続いて`axon-kit:work-state`を使い、Issueをdoneにする。最終的なEntityの状態と、影響を受けたすべてのfrontierを確認する。

Issueがすでに満たされていてリポジトリの変更が不要な場合は、空commitを作らない。役立つ場合は変更不要と検証した結果を記録し、declarationが実際に満たされている場合に限りIssueをdoneにする。

## 失敗時の引き継ぎ

claimの取得後にワークフローを完了できない場合は、次のようにする。

1. reviewしていない結果や検証に失敗している結果をcommitせず、Issueをdoneにしない。
2. 安全で作業範囲内にあるworking treeの変更を保持し、作業内容を自動的に破棄しない。
3. `axon-kit:add-note`を使い、重要な現在の状態、完了した作業、残作業、検証結果、blockerだけを記録する。
4. Noteを確認した後、`axon-kit:work-state`を使い、reasonを付けてIssueをreleaseする。

Noteの記録に失敗した場合、またはDB上の結果が不明な場合はreleaseせず、現在のclaimと照合に必要な情報を報告する。releaseに失敗した場合は、同じNoteを再度追加してはならない。

Issue ID、Axonの最終状態、作成した場合はcommit ID、検証とself-reviewの結果、Note番号、未解決のblockerを報告する。
