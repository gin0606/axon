# 日常の操作

以下の `axon` は [初回手順](getting-started.md) で選んだ新binaryを指します。構文は `axon <command> --help` で確認できます。

## 状況や記録を確認する

| 目的 | 操作 |
| --- | --- |
| 未判断の候補 | `axon triage` |
| 浮上した未着手と全着手中 | `axon tasks` |
| 保存済み全件（条件を実行しない） | `axon list` |
| 本文と直接の待ち理由 | `axon show ID` |
| Noteの本文 | `axon note list ID` |
| 状態変更の経緯 | `axon log ID` |
| 記録者の詳細 | `axon log ID --recorder-details` / `axon note list ID --recorder-details` |

`tasks` には依存や親の着手待ちも含まれます。表示の状況を読み、着手できると決めつけないでください。候補に出ないことは操作禁止やEntityの不存在を意味しません。

## 登録と状態変更

`capture --title '懸念' -m '内容'` は未判断、`plan --title '仕事' -m '目的と完了条件'` は未着手の採用済みIssueを登録します。`group capture` / `group plan` はGroupを作ります。作成時の `--parent G` と繰り返せる `--needs B` で関係を付けられます。

未判断を採用するには `accept ID`、未着手の採用を撤回するには `withdraw ID`。作業は `start ID`、中断は `release ID -r '理由'`、完了は `done ID`、取りやめは `cancel ID -r '理由'`、取りやめの再検討は `reconsider ID`。完了したEntityは再開しません。結果は `note add ID -m '結果'` へ残し、本文変更は `write ID --title '題名' -m '本文'` で行います。

## Groupと関係

`group set A --parent G` / `group unset A` で所属を変更し、`dep add A --needs B` / `dep rm A --needs B` で依存を変更します。親や依存先の状態、循環、終了した構成の制約はCLIが検査します。Groupをstartしても子はstartされません。

Groupのdone前には目的・完了条件、全子孫の終了、成果の統合と必要な検証を確認します。showの直属の子を辿り、必要な本文・Note・logを読んで不足を確認します。全子孫Noteの一括取得は必須ではありません。子の終了だけで親を自動完了せず、Groupに対するdone自体を計画全体の最終確認済みという入力にします。

## 再浮上

`when set ID --command 'test -f ready.txt'` は条件を保存し、`when clear ID` は解除します。保存時には実行しません。triage/tasksだけが必要な条件を `/bin/sh -c` で評価し、終了0は成立、1は未成立、その他は一覧の失敗です。`--condition-timeout` と `--trace-conditions` の契約は [外部条件](../development/lifecycle-candidates.md) を参照してください。
