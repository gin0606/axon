---
name: register
description: 新しいIssueまたはGroupの重複と計画の完成度を確認し、依頼から確定できる採否、本文、構造、再浮上条件で登録する個人用ワークフロー。既存Entityの変更や実装には使わない。
---

# 新しいAxon Entityを登録する

`axon:conventions`と、最終状態に応じた`axon-kit:capture`または`axon-kit:plan`を使う。

## 登録内容を確定する

関連するコード、文書、Entityを必要な範囲で調べ、目的、scope、完了条件、kind、親Group、outgoing dependency、未設定またはshell文字列の再浮上条件を確定する。

- ユーザーが実行または採用を明示していれば採用済みのNotStarted、懸念や未解決事項の記録ならUndecidedとする。採否を読み取れない単独の「登録して」では確認する。
- 一つの懸念または作業はIssue、複数Entityを含む明示的な計画範囲はGroupとする。合意済みscopeからkindを一意に決められない場合は確認する。
- 再浮上条件の既定は未設定とする。shell文字列と実行影響が依頼から確定している場合だけCommandを保存する。「後で」だけでは条件を発明しない。旧日時・Manual・AfterEntityの意味をshellへ移す必要があるなら、時刻基準、実行場所、参照先を含む具体案を示して判断を得る。

`axon:conventions`に従い、合意済みの目的、scope、完了条件から親、dependency、分解を自律して導く。計画の意味、採用、公開仕様、独立した完了単位を新たに決める必要がある場合だけユーザーへ返す。

## 会話から独立したdeclarationを作る

後続セッションが`axon show`だけを読んでも、なぜ存在し、何を満たせば終了かを理解できるtitleとdescriptionにする。会話内だけの指示語、比較対象、略称を残さない。観測と提案を区別し、登録後に得られる結果や申し送りをdescriptionの予約欄にしない。

ユーザーが示した内容と調査で確定した事実からdeclarationを構成する。通常の実装詳細は実装者に委ねる。目的、scope、完了条件そのものを補って発明する必要がある場合は、登録前にその判断を確認する。重要な意思決定を加えていなければ、完成した文面だけを理由に再確認を求めない。

## 重複を解決して登録する

`axon:conventions`の重複確認を行う。完全に一致する未終端Entityを再利用できる場合は、そのIDと現在状態を結果とする。採否の不一致、CompletedまたはCancelled、cross-kind、scopeの差、複数候補がある場合はユーザーの選択を得る。新しいIDの作成が明示されていれば新規登録する。

採否未判断なら`axon-kit:capture`、採用済みなら`axon-kit:plan`を使い、作成結果と関係を確認する。このskillは登録と検証で終了し、同じ依頼が明示的に実装workflowまで含み、そのworkflowへ引き渡す場合を除いてstartや実装へ進まない。

作成または再利用したID、kind、declaration、lifecycleと条件、重複判断、候補と着手前提への影響、未解決事項を報告する。
