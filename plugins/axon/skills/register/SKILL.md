---
name: register
description: 新しいIssueまたはGroupの重複と計画の完成度を確認し、ユーザーの判断に従ってUndecidedまたはAcceptedで登録する個人用ワークフロー。既存Entityの変更や実装には使わない。
---

# 新しいAxon Entityを登録する

`axon:conventions`と、最終状態に応じた`axon-kit:capture`または`axon-kit:plan`を使う。

## 登録前の調査

関連するコード、文書、Entityを必要な範囲で調べ、目的、scope、完了条件、kind、親Group、outgoing dependencyを確認する。`axon:conventions`の重複確認を行い、再利用や再判断が必要な候補があれば、新しいIDを割り当てる前にユーザーへ提示する。

Issueは一つの懸念または作業、Groupは複数Entityを含み得る明示的な計画範囲に使う。kindや構造的役割が決まらない場合は登録しない。

## 会話から独立したdeclarationを作る

後続セッションが`axon show`だけを読んでも、なぜ存在し、何を満たせば終了かを理解できるtitleとdescriptionにする。会話内だけの指示語、比較対象、略称を残さない。観測と提案を区別し、登録後に得られる結果や申し送りをdescriptionの予約欄にしない。

Acceptedとして登録する場合は、目的、scope、利用者から見える振る舞い、公開契約、状態モデル、後戻りしにくいコスト、分解、親、dependencyに、実装前に解消すべき判断が残っていないか確認する。残る場合は、この相談で決めるか、別の前提Entityとして表現するかをユーザーに選んでもらう。通常の実装詳細だけを理由に登録を止めない。

エージェントが目的、完了条件、構造へ実質的な判断を加えた場合は、最終案を提示し、ユーザーがその内容とDispositionを確認してから登録する。意味を変えない誤字修正や整形だけなら再確認を要求しない。

## 公式kitで登録する

採否未判断なら`axon-kit:capture`、採用済みなら`axon-kit:plan`を使う。登録後にstartや実装へ進まない。

作成したID、kind、declaration、Control state、重複判断、readinessへの影響、部分完了または未解決事項を報告する。
