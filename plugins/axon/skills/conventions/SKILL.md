---
name: conventions
description: Axon公式kitの上に、重複確認、ユーザー判断、調査範囲、Note、Codexの権限処理を加える個人用協業規約。他のaxon plugin skillの共通基盤として使い、Axon自体の意味論としては扱わない。
---

# 個人用Axon協業規約

このskillは`axon-kit:conventions`を前提とし、Axonの意味論ではなく個人用ワークフローの方針を追加する。公式kitと矛盾する場合は操作を止め、差異を報告する。

## 判断と操作の権限

対象の選択、採否、時期、構造、着手は、ユーザーの依頼または呼び出し元ワークフローが供給する。候補、比較、推奨は提示してよいが、提示だけを状態変更の許可とみなさない。

明示された操作だけを実行し、登録を着手へ、採否変更をreleaseやdoneへ、子の完了を親Groupの完了へ自動的につなげない。

`axon decide`、`axon when`、`axon release`には、ユーザーの判断または作業状況から復元できるreasonを必ず付ける。reasonは状態変更の根拠に限定し、Noteと同じ内容を重複させない。

## 既存情報と波及の調査

既存Entityを変更するときは`axon show <id>`の全出力を読み、判断またはdeclarationに関係する全Declaration Revisionと`axon log <id>`を確認する。古いNoteやRevisionを年齢だけで無関係とみなさない。

terminal状態が変わるDisposition操作、`done`、または完全な波及確認が必要な操作では、直接のdependent、Groupの祖先と子孫、active scope、`ready`、`triage`を確認する。reverse `AfterEntity`の完全な影響があり得る場合は、`axon list`の全IDから参照元を調べる。

## 新規Entityの重複確認

新規登録前に`axon list`と必要なfrontierを確認する。同じ目的、scope、完了条件、kind、構造的役割を持つ未終端Entityは重複候補として扱い、再利用、再判断、別Entityとしての作成をユーザーが選ぶまで新規作成しない。

cross-kindでも実効scopeが同じ候補は提示する。完了済みまたはRejectedの候補がある場合は、その履歴を再利用するか新しい作業として区別するかを確認する。部分的に重なるだけのEntityは登録を妨げないが、依頼されていない関係を追加しない。

## Noteと進行理由

後続作業に必要な実装・調査結果、制約、申し送りはNoteにする。単なる進捗実況、定型的な開始・完了報告、状態変更reasonと同じ内容はNoteにしない。

releaseやdoneに先立つNoteが必要な場合は、Noteの保存と番号を確認してから進行状態を変更する。後続の状態変更が失敗または結果不明でも、確認済みNoteを再追加しない。

## Codexから共有DBを更新する場合

Git linked worktreeで共有`.axon`がsandbox外にあるためmutationだけが拒否された場合は、そのmutation commandだけをホストの許可機構で再実行する。読み取りcommandや他のprogramまで権限を広げない。再実行できなければ状態を推測せず、未反映として報告する。
