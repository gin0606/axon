---
name: axon-implement-issue
description: ユーザーから指定された採用済みの axon Entity の着手・実装・計画進行、または Disposition を問わず既に InProgress の Entity の進行同期・引き渡し・打ち切り・完了を axon に反映する。Entity ID を渡されて実装、進行、再開、release、done を依頼されたときに使う。採否相談や自律的な仕事選びには使わない。
---

# Axon Entity の作業状態を同期する

指定された 1 件の Entity の作業状況と axon 上の進行を一致させる。この skill は axon 外の作業内容や権限を追加しない。

## 共有 DB への書き込み権限

Git linked worktree では共有 `.axon` が作業ディレクトリの外に置かれることがある。状態変更 command が共有 DB への書き込み権限不足で拒否された場合は、その command だけを実行環境の許可機構で再実行する。読み取り・探索 command や他の program まで権限を広げず、再実行できなければ状態を推測せずに拒否と未反映を報告する。

## 着手

1. `axon show <id>` で kind、本文、採否、進行、依存、親 Group を確認する。
2. `start` は同じ transaction で ready を検査して claim を取得する。候補選定には `axon ready`、失敗理由の追加調査には `axon show`、`axon ready`、`axon triage`、`axon list` を使う。
3. `ready` でも、本文に実装前のユーザー判断や未完成の設計成果物が必要だと明記されているのに、依存として表現されていない場合は `axon start` しない。axon 上の記述と導出状態の不一致としてユーザーへ報告し、採否や依存を独断で変えない。通常の実装詳細が未記載であることだけでは止めない。
4. 着手可能なら、作業を始める前に `axon start <id>` を実行する。
5. 再開を依頼され、すでに着手中なら、claim を奪ったり `axon release` したりせず、`axon show <id>` に含まれる Note と着手者を確認する。Group の claim は子孫を lock しない。安全に引き継げると判断できない場合はユーザーに確認する。
6. 新しく start する対象が未判断、不採用、依存待ち、前提喪失、終了済み、または別の作業者が着手中なら、採否や依存を独断で変えず、ユーザーに状態を報告する。既に InProgress なら Disposition にかかわらず、安全な release / done と申し送りの確認を続ける。
7. `axon start` が失敗したら、別の Entity を選ばずユーザーに報告する。

## Group の進行

Group は子孫を包含する計画範囲であり、Group の start は子孫を自動 start しない。start 後に `axon ready` と `axon triage` で新しく active になった frontier を確認し、依頼された範囲で子孫の判断や実装を進める。ユーザーが Group だけの start を依頼した場合は、子孫を自律的に選んで着手しない。

Group を Rejected にしても自身の Progress / claim と子孫の保存状態は変わらない。配下は active scope 外になり、その Group を dependency target とする Entity は orphaned、`AfterEntity` で待つ Entity は surfaced になり得る。NotStarted の Rejected Group はそのまま terminal であり、claim 解消のために start、release、done を行わない。ただし祖先 Group の完了に非 terminal 子孫の整理が必要なら、その扱いを合意する。InProgress の場合、一時中断または引き渡しなら InProgress の子孫を担当者との合意なく解放せず、その数が 0 になってから Group を release する。恒久的な打ち切りなら、子孫を独断で変更せず、合意に従って全子孫を terminal にするか包含外へ移してから Group を done にし、`Ended × Rejected` にする。Group は子孫から自動終了しないため、計画全体と妨げがないことを `axon show <group-id>` で確認する。非 terminal な子孫が残る場合は done にせず、その ID と状態を報告する。

この確認は axon に記録された本文と関係の整合だけを対象にする。現行コードや設計文書を再調査する外部の実装手順へ、この skill の責務を広げない。

## 申し送り

この skill で Note を追記するときは、事前に `axon note list <id>` で最新番号を控え、意図した本文を保持して `axon note add` を一度だけ実行する。成功時は返された番号を `axon note show` で本文と actor まで確認する。command の成否が不明なら先の process が終了したことを確認し、事前番号より後の Note を list / show する。新規 Note が 0 件と確認できたときだけ再試行し、1 件で本文と actor が意図と一致し、他 actor の並行追記など帰属を疑う事情がなければその番号を使う。複数の新規 Note、属性不一致、または一致しても帰属が確定できない場合は追記せず停止し、候補番号を報告する。

作業を途中で止める場合は、再開に必要な現在地、残作業、検証状況を `axon note add <id> -m <body>` または `-F <file>` で追記し、恒久的な完了や打ち切りでなければ `axon done <id>` を実行しない。Note は plan declaration や状態を変えず、Issue / Group と全状態へ追記できる。既存 Note の誤りや前提変化も上書きせず、新しい Note として残す。

一時中断または引き渡しで claim を手放す合意がある場合は、申し送り Note の保存を `axon show <id>` で確認し、`note add` が返した Note 番号を控えた後に `axon release <id> -r <reason>` を単独で実行する。Group は InProgress の子孫が 0 件であることを先に確認し、子孫を担当者との合意なく release しない。release の失敗または成否不明では再度 Note を追記せず止まり、Note 番号と現在状態を報告する。再試行時は内容が変わった場合だけ新しい Note を追記し、そうでなければ確認済みの Note 番号を再利用する。`axon show <id>` と `axon claims` で NotStarted と claim 解消を確認し、`axon list`、`axon ready`、`axon triage` で frontier の変化を確認する。

## 完了

1. `done` は「今後その Entity の作業を進めない」ことを表し、一時中断には使わない。Accepted Entity は本文の目的を達成して残作業がない場合、Rejected Entity は恒久的な打ち切りをユーザーが合意した場合に実行する。Undecided Entity は今後作業しないことをユーザーが明示した場合だけ実行する。Group はどの Disposition でも、加えて全子孫が terminal でなければならない。
2. `done` 前に `axon list` の全 ID を列挙してそれぞれ `axon show <id>` を実行し、対象を参照する `Resurface condition: AfterEntity(...)` を探す。見つけた waiter とその祖先 Group を記録する。
3. 実装、調査、計画進行で後から参照すべき結果や検証状況が生じた場合は、`done` 前に簡潔な結果 Note を追記し、`axon show <id>` で全文の保存を確認し、`note add` が返した Note 番号を控える。必要な Note を保存・確認できなければ `done` せず停止する。状態同期だけで新しい結果がない場合は追加しない。
4. 条件を満たす InProgress Entity に `axon done <id>` を実行する。
5. `done` が失敗または成否不明なら再度 Note を追記せず止まり、確認済みの Note 番号と現在状態を報告する。再試行時は内容が変わった場合だけ新しい Note を追記し、そうでなければ確認済みの Note 番号を再利用する。成功したら `axon show <id>` で終了状態と、追記した場合の結果 Note を確認する。
6. terminal にした Entity に親 Group がある場合は InProgress の祖先 Group を順に `axon show` し、現在 done できるかと残る非 terminal 子孫を確認する。祖先 Group の進行が依頼範囲に含まれない限り、自動で done にしない。
7. `done` / `decide`、dependency の追加・削除、Resurface condition の変更、Group の activation gate を変える `start` / `release` を行った場合は、kind を問わず関連 Entity の `show` と `axon list`、`axon ready`、`axon triage` で dependent、手順 2 で見つけた `AfterEntity` waiter とその Group ancestry、Group 子孫、frontier への波及を確認する。
8. ユーザーに Entity の最終状態、追記した場合の結果 Note とその番号、完了可能になった祖先 Group、関連 Entity と frontier への波及を報告する。

未解決の完了条件が残る場合は `axon done <id>` を実行しない。
