# 日常の操作

以下の `axon` は [初回手順](getting-started.md) で選んだ新binaryを指します。構文は `axon <command> --help` で確認できます。

## 状況や記録を確認する

| 目的 | 操作 |
| --- | --- |
| 保存状態・条件・全直接関係 | `axon show ID --details` |
| 未判断の候補 | `axon proposals` |
| 浮上した未着手と全着手中 | `axon tasks` |
| 保存済み全件（条件を実行しない） | `axon list` |
| 本文と直接の待ち理由 | `axon show ID` |
| Entityの現在の主題 | `axon list --search 語句` |
| 所属不明のNoteの内容 | `axon note search 語句` |
| Noteの原文を個別に読む | `axon note show ID NOTE_ID` |
| Noteの本文 | `axon note list ID` |
| 状態変更の経緯 | `axon log ID` |
| 記録者の詳細 | `axon log ID --recorder-details` / `axon note list ID --recorder-details` |

`axon tasks` には依存や親の着手待ちも含まれます。表示の状況を読み、着手できると決めつけないでください。候補に出ないことは操作禁止やEntityの不存在を意味しません。

## 登録と状態変更

登録は `axon capture` だけです。`axon capture --title '懸念' -m '内容'` は未判断のIssue、`axon capture --accept --title '仕事' -m '目的と完了条件'` は未着手の採用済みIssueを登録します。`--kind group` を足すとGroupになり、`--kind` と `--accept` は自由に組み合わせられます。作成時の `--parent G` と繰り返せる `--needs B` で関係を付けられます。

未判断を採用するには `axon accept ID`、未着手の採用を撤回するには `axon withdraw ID`。作業は `axon start ID`、中断は `axon release ID -r '理由'`、完了は `axon complete ID`、取りやめは `axon cancel ID -r '理由'`、取りやめの再検討は `axon reconsider ID`。完了したEntityは再開しません。結果は `axon note add ID -m '結果'` へ残し、本文変更は `axon write ID --title '題名' -m '本文'` で行います。

## Groupと関係

`axon parent set A --parent G` / `axon parent unset A` で所属を変更し、`axon dep add A --needs B` / `axon dep rm A --needs B` で依存を変更します。親や依存先の状態、循環、終了した構成の制約はCLIが検査します。Groupを`Start`しても子は`Start`されません。

Groupの完了前には目的・完了条件、全子孫の終了、成果の統合と必要な検証を確認します。`axon show`の全子孫ツリーを確認し、必要な本文・Note・logを読んで不足を確認します。全子孫Noteの一括取得は必須ではありません。子の終了だけで親を自動完了せず、Groupに対する `axon complete` 自体を計画全体の最終確認済みという入力にします。

## 計画をまとめて登録・編集する

一括操作には `axon-declaration/v1` の YAML を使います。SQLite・file の両方式で手順は同じです。形式の詳細は `axon docs declaration` と [CLI契約](../reference/lifecycle-cli.md#計画全体の取得と一括編集) を参照してください。

### 新規計画を登録する

未使用の出力先に雛形を作り、目的・完了条件・包含・依存を編集します。

```sh
axon docs declaration --example > new-plan.yaml
```

新規recordは `id: null`、`base: null`、file内で一意の `key` を持ちます。初期状態は未判断なら `lifecycle: undecided`、採用済みなら `lifecycle: not-started` です。雛形は採用済みなので、未判断の提案を登録する場合は変更してください。参照は `{ key: 名前 }` または完全IDの `{ id: ID }` で書きます。

編集後は次を一つずつ実行し、各結果を確認します。

```sh
axon import prepare new-plan.yaml
axon import check new-plan.yaml
axon import apply new-plan.yaml
axon import check new-plan.yaml
```

`axon import prepare` は保存先を変えず、最終IDを割り当て、外部参照を再生成して同じfileをcanonical形式に置き換えます。新規recordの `key -> 完全ID` の対応も一行ずつ表示します。コメントは保持しません。`axon import check` はfileも保存先も変えず、作成・titleの前後・descriptionの変更有無・親の前後・依存の増減を表示します。titleの改行や制御文字は`axon list`と同じ規則で可視化され、一行で表示されます。本文の全文はfile自体の差分で確認してください。`axon import apply` は最新の保存状態で再検証して全件を原子的に反映し、成功後に同じfileの `base` などを更新し、更新前に新規だったrecordの `key -> 完全ID` も表示します。最後の `axon import check` で差分がないことを確認します。

### 登録後の計画を修正する

`GROUP_ID` を対象のIDへ置き換え、未使用のfileへ取得します。既存の編集fileへリダイレクトして上書きしないでください。

```sh
axon export GROUP_ID > plan-edit.yaml
```

Groupは自身と終了済みを含む全子孫、Issueは単体を取得します。複数IDを渡すと和集合になります。編集前のfileを別に保全してから `title`、`description`、`parent`、`needs` を修正し、必要な新規recordを足します。既存の `id`・`base`・`lifecycle`・kindは変更しません。編集後は同じ `axon import prepare` → `axon import check` → `axon import apply` → 再 `axon import check` を `plan-edit.yaml` に対して実行します。

`groups`・`issues` に載っていないEntityは触りません。fileからrecordを消しても、削除・取消・所属解除・依存解除にはなりません。Groupから外すにはそのEntityのrecordに `parent: null`、依存をすべて外すには `needs: []` を書きます。作業の取消や既存のlifecycle遷移には通常コマンドを使います。

編集集合のrecordは文面・親・outgoing dependencyの完全な宣言です。外部Entityの `references` は読み取り専用のcontextで、外から入る所属・依存は含みません。必要なら `axon show ID --details` で調べます。既存Entityを編集集合へ加える場合は対象IDを追加して別fileへ再度`axon export`し、保全した編集意図を移してください。既存recordの `base` を手作りしません。再浮上条件・Noteは取り込まず、既存値を保持します。

### 競合と保存失敗を確認する

競合では元fileを保全して現在値と編集意図を比較します。`base` の改変やIDの振り直しで検査を通さず、再取得が必要な場合は別fileへ`axon export`します。

保存先とdeclaration fileの結果は別です。保存先が `Applied` でもfile更新だけが `Not applied` または `Result unknown` になることがあります。元processの終了後、同じ保存先と入力fileを照合し、確定したIDと内容を保持して `axon import check` します。編集集合の全Entityが最終値に一致する場合は、同じfileの再 `axon import apply` が保存先をno-opにしてfile更新を完了します。一部だけ一致する場合は競合です。結果不明のまま `axon import prepare` でIDを再割当てしたり、雛形から登録し直したりしません。詳細は [保存結果](../reference/lifecycle-cli.md#mutationの結果) を参照してください。

## 並行作業と引継ぎ

SQLiteを共有するworktreeでは、同じ未着手Entityへの並行`axon start`は一つだけ成功します。失敗側は`axon show`・`axon log`で現在値を読み、実行中のworkerと調整してください。記録者情報を所有権として扱わず、作業終了を確認してから継続・`axon release`を判断します。

fileの別worktreeでは同じEntityに対してそれぞれ`axon start`を実行できます。Gitで取り込むまで互いの作業は見えません。異なる現在値の衝突はEntity全体で選び、両側のNoteと実操作の履歴を保持します。同じworktreeでGit更新とAxon書込みを並行しないでください。統合後は [明示的な統合](../development/lifecycle-file.md#明示的な統合) に従って検査・stageし、`axon show`・Note・logで成果を確認して通常操作へ戻ります。

## 再浮上

`axon condition set ID --command 'test -f ready.txt'` は条件を保存し、`axon condition unset ID` は解除します。保存時には実行しません。`axon proposals|tasks`だけが必要な条件を `/bin/sh -c` で評価し、終了0は成立、1は未成立、その他は一覧の失敗です。`--condition-timeout` と `--trace-conditions` の契約は [外部条件](../development/lifecycle-candidates.md) を参照してください。

IDはprefix＋ランダム6文字で、完全IDまたは一意なsuffixを指定できます。CLI生成文は英語、TTYでは意味に応じて装飾し、NO_COLORまたは非TTYでは装飾しません。一覧の絞り込み・検索、Note個別参照と保存結果は [CLI契約](../reference/lifecycle-cli.md) を参照してください。

`axon list|tasks|proposals`の `--search` は現在のtitle・descriptionだけを検索し、Noteだけの一致は返しません。Noteの情報は `axon note search` へ移行してください。終了Entityを含む全Noteから、完全ID・日時と最初の一致の前後24文字を1 Noteにつき1行で表示します。`Excerpt:` と `…` は抜粋・省略を示し、改行や制御文字は可視化します。原文は `axon note show ID NOTE_ID` で取得できます。検索は大小文字を区別するliteral部分一致で、空白の除去やUnicode正規化はしません。
