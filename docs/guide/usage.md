# 日常の操作

以下の `axon` は [初回手順](getting-started.md) で選んだ新binaryを指します。構文は `axon <command> --help` で確認できます。

## 状況や記録を確認する

| 目的 | 操作 |
| --- | --- |
| 保存状態・条件・全直接関係 | `axon show ID --details` |
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

Groupのdone前には目的・完了条件、全子孫の終了、成果の統合と必要な検証を確認します。showの全子孫ツリーを確認し、必要な本文・Note・logを読んで不足を確認します。全子孫Noteの一括取得は必須ではありません。子の終了だけで親を自動完了せず、Groupに対するdone自体を計画全体の最終確認済みという入力にします。

## 並行作業と引継ぎ

SQLiteを共有するworktreeでは、同じ未着手Entityへの並行startは一つだけ成功します。失敗側はshow・logで現在値を読み、実行中のworkerと調整してください。記録者情報を所有権として扱わず、作業終了を確認してから継続・releaseを判断します。

fileの別worktreeでは同じEntityをそれぞれstartできます。Gitで取り込むまで互いの作業は見えません。異なる現在値の衝突はEntity全体で選び、両側のNoteと実操作の履歴を保持します。同じworktreeでGit更新とAxon書込みを並行しないでください。統合後は [明示的な統合](../development/lifecycle-file.md#明示的な統合) に従って検査・stageし、show・Note・logで成果を確認して通常操作へ戻ります。

## 再浮上

`when set ID --command 'test -f ready.txt'` は条件を保存し、`when clear ID` は解除します。保存時には実行しません。triage/tasksだけが必要な条件を `/bin/sh -c` で評価し、終了0は成立、1は未成立、その他は一覧の失敗です。`--condition-timeout` と `--trace-conditions` の契約は [外部条件](../development/lifecycle-candidates.md) を参照してください。

IDはprefix＋ランダム6文字で、完全IDまたは一意なsuffixを指定できます。CLI生成文は英語、TTYでは意味に応じて装飾し、NO_COLORまたは非TTYでは装飾しません。一覧の絞り込み・検索、Note個別参照と保存結果は [CLI契約](../reference/lifecycle-cli.md) を参照してください。
