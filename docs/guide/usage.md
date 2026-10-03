# 日常の操作

以下の `axon` は [初回手順](getting-started.md) で用意した実行ファイルを指します。構文は `axon <command> --help` で確認できます。

## 状況や記録を確認する

| 目的 | 操作 |
| --- | --- |
| 保存状態・条件・全直接関係 | `axon show ID --details --skip-conditions` |
| 未判断の候補 | `axon proposals` |
| 浮上した未着手と全着手中 | `axon tasks` |
| 保存済み全件（条件を実行しない） | `axon list` |
| 本文と直接の待ち理由 | `axon show ID` |
| Entityの現在の主題 | `axon list --search 語句` |
| 仕事の種類ごとの一覧 | `axon list --label bug` |
| 所属不明のNoteの内容 | `axon note search 語句` |
| Noteの原文を個別に読む | `axon note show ID NOTE_ID` |
| Noteの本文 | `axon note list ID` |
| 状態変更の経緯 | `axon log ID` |
| 記録者の詳細 | `axon log ID --recorder-details` / `axon note list ID --recorder-details` |

一覧と `axon show` の先頭行は `ID  Kind  Situation  Label  Title` の順です。`axon tasks` にはIssueとGroupが平らに並び、依存や祖先の採用待ちも含まれます。表示の状況を読み、着手できると決めつけないでください。Groupの状況は配下から導出され、`Empty` は計画を書く段階、`Confirmable` は最終確認して完了できる段階、`Ready` は配下に着手できる浮上したIssueがある段階です（再浮上条件を評価しない `axon list`・`axon show --skip-conditions` では浮上を問いません）。`InProgress` は配下の仕事が始まっていること、`Blocked` はそれ以外を示します。完了できず、配下に着手できるIssueも着手中のIssueもないGroupは、`axon show` の `Stalled` 節で理由を確認できます。`axon show` は `axon tasks` と同じく再浮上条件を評価するので、条件で隠れているIssueは `Unsurfaced`、隠れたIssueしか持たないGroupは `Blocked` と `Unsurfaced candidate:`、Group自身の条件で隠れていることは `Own condition unsatisfied:` で読めます。保存情報だけを読むときは `--skip-conditions` を付けます。候補に出ないことは操作禁止やEntityの不存在を意味しません。

## 登録と状態変更

登録は `axon capture` だけです。`axon capture --label spike --title '懸念' -m '内容'` は未判断のIssue、`axon capture --accept --label feat --title '仕事' -m '目的と完了条件'` は未着手の採用済みIssueを登録します。`--label` は必須です（[labelで仕事の種類を示す](#labelで仕事の種類を示す)）。`--kind group` を足すとGroupになり、`--kind` と `--accept` は自由に組み合わせられます。作成時の `--parent G` と繰り返せる `--needs B` で関係を付けられます。

未判断を採用するには `axon accept ID`、未着手の採用を撤回するには `axon withdraw ID`。Issueの作業は `axon start ID`、中断は `axon release ID -r '理由'`、完了は `axon complete ID`、取りやめは `axon cancel ID -r '理由'`、取りやめの再検討は `axon reconsider ID`、完了の取消は `axon reopen ID -r '理由'`。`axon reopen` は完了したEntityを未着手へ戻します。完了済みの依存元がある場合は拒否されるため、先に依存元を`axon reopen`してください。IssueとGroupの種類は `axon convert ID --kind group` / `axon convert ID --kind issue` で変換します。Groupの本文は計画全体の成果と最終確認の対象を表すので、Issueの本文をそのままGroupにしたときは本文を読み直し、必要なら `axon write` で整えてください。変換はlifecycle遷移ではないため `-r/--reason` を受け付けません。結果は `axon note add ID -m '結果'` へ残し、本文変更は `axon write ID --title '題名' -m '本文'` で行います。

## labelで仕事の種類を示す

IssueとGroupは、仕事の種類を表すlabelを必ず一つ持ちます。値は次の7つだけです。`axon capture --label`・`axon label set`・一覧の `--label` では、省略や集合外の値は構文エラー（終了2）になり保存を変えません。declarationでは `label` の欠落や集合外の値がschemaの拒否になります。Groupのlabelは計画の主な種類を表し、配下のlabelとは独立です。

| label | 意味 |
| --- | --- |
| `bug` | 期待と異なる振る舞いを直す |
| `feat` | 新しい振る舞いや機能を加える |
| `chore` | 依存の更新、CI、設定など、振る舞いを変えない保守 |
| `docs` | 文書・help・skillを整備する |
| `test` | テストを追加・整理する |
| `refactor` | 振る舞いを変えずに構造を整理する |
| `spike` | 決めるために調べる（要検討の事項、仕様・計画・方針の検討） |

labelは `axon label set ID VALUE` で変更します。同じ値の指定は `No changes` の成功で、解除はありません。title・descriptionと同じく、`Completed`・`Cancelled` になったEntityのlabelは固定され、同じ値の指定も拒否されます。変更は `axon log` に `Label set: bug` のように残ります。`axon convert` はlabelを変えません。labelは分類で、優先度ではありません。候補一覧、状況、lifecycleの操作には影響しません。契約は [label](../reference/lifecycle.md#label) にあります。

## Groupと関係

`axon parent set A --parent G` / `axon parent unset A` で所属を変更し、`axon dep add A --needs B` / `axon dep rm A --needs B` で依存を変更します。祖先や依存先の状態、循環、終了した構成の制約はCLIが検査します。Groupには`axon start`・`axon release`を使わず、配下のIssueに着手します。Groupに置いた依存は、配下のIssueの`Start`の前提になります。

Groupの完了前には目的・完了条件、全子孫の終了、成果の統合と必要な検証を確認します。`axon show`の全子孫ツリーを確認し、必要な本文・Note・logを読んで不足を確認します。全子孫Noteの一括取得は必須ではありません。子の終了だけで親を自動完了せず、Groupに対する `axon complete` 自体を計画全体の最終確認済みという入力にします。

## 計画をまとめて登録・編集する

一括操作には `axon-declaration/v2` の YAML を使います。形式の詳細は `axon docs declaration` と [計画全体の取得と一括編集](../reference/declaration.md) を参照してください。

### 新規計画を登録する

未使用の出力先に雛形を作り、目的・完了条件・包含・依存を編集します。

```sh
axon docs declaration --example > new-plan.yaml
```

新規recordは `id: null`、`base: null`、file内で一意の `key` を持ちます。初期状態は未判断なら `lifecycle: undecided`、採用済みなら `lifecycle: not-started` です。雛形は採用済みなので、未判断の提案を登録する場合は変更してください。各recordの `label` も必須です。雛形はすべて `feat` なので、recordごとに仕事の種類に合う値へ書き換えてください。参照は `{ key: 名前 }` または完全IDの `{ id: ID }` で書きます。

編集後は次を一つずつ実行し、各結果を確認します。

```sh
axon import prepare new-plan.yaml
axon import check new-plan.yaml
axon import apply new-plan.yaml
axon import check new-plan.yaml
```

`axon import prepare` は保存先を変えず、最終IDを割り当て、外部参照を再生成して同じfileをcanonical形式に置き換えます。新規recordの `key -> 完全ID` の対応も一行ずつ表示します。コメントは保持しません。`axon import check` はfileも保存先も変えず、作成・titleの前後・descriptionの変更有無・labelの前後・親の前後・依存の増減を表示します。titleの改行や制御文字は`axon list`と同じ規則で可視化され、一行で表示されます。本文の全文はfile自体の差分で確認してください。`axon import apply` は最新の保存状態で再検証して全件を原子的に反映し、成功後に同じfileの `base` などを更新し、更新前に新規だったrecordの `key -> 完全ID` も表示します。最後の `axon import check` で差分がないことを確認します。

### 登録後の計画を修正する

`GROUP_ID` を対象のIDへ置き換え、未使用のfileへ取得します。既存の編集fileへリダイレクトして上書きしないでください。labelを導入する前の `axon-declaration/v1` のfileは拒否されます。v1の `base` はv2のfingerprintと一致しないため、既存Entityのrecordには手で `label` を足さず `base` も書き換えず、未使用のfileへ `axon export` で取り直して編集意図を移します。保存先にまだないrecordには `label` を足します。そうしたrecordだけのfileはschema行を `axon-declaration/v2` にします。取り直したfileがあれば、そこへ移します。`base: null` でも `axon import prepare` 済みで保存済みのrecordは取り直す側です。その見分け方と参照の書き換えは [declarationの契約](../reference/declaration.md#canonical-形式) にあります。

```sh
axon export GROUP_ID > plan-edit.yaml
```

Groupは自身と終了済みを含む全子孫、Issueは単体を取得します。複数IDを渡すと和集合になります。編集前のfileを別に保全してから `title`、`description`、`label`、`parent`、`needs` を修正し、必要な新規recordを足します。既存の `id`・`base`・`lifecycle`・kindは変更しません。編集後は同じ `axon import prepare` → `axon import check` → `axon import apply` → 再 `axon import check` を `plan-edit.yaml` に対して実行します。

`groups`・`issues` に載っていないEntityは触りません。fileからrecordを消しても、削除・取消・所属解除・依存解除にはなりません。Groupから外すにはそのEntityのrecordに `parent: null`、依存をすべて外すには `needs: []` を書きます。作業の取消や既存のlifecycle遷移には通常コマンドを使います。

編集集合のrecordは文面・label・親・outgoing dependencyの完全な宣言です。外部Entityの `references` は読み取り専用のcontextで、外から入る所属・依存は含みません。必要なら `axon show ID --details --skip-conditions` で調べます。既存Entityを編集集合へ加える場合は対象IDを追加して別fileへ再度`axon export`し、保全した編集意図を移してください。既存recordの `base` を手作りしません。再浮上条件・Noteは取り込まず、既存値を保持します。

### 競合と保存失敗を確認する

競合では元fileを保全して現在値と編集意図を比較します。`base` の改変やIDの振り直しで検査を通さず、再取得が必要な場合は別fileへ`axon export`します。

保存先とdeclaration fileの結果は別です。保存先が `Applied` でもfile更新だけが `Not applied` または `Result unknown` になることがあります。元processの終了後、同じ保存先と入力fileを照合し、確定したIDと内容を保持して `axon import check` します。同じfileの再 `axon import apply` は、最終値に一致するEntityを適用済み、`base` に一致するEntityを未適用として残りを反映し、全件が適用済みなら保存先をno-opにしてfile更新を完了します。どちらにも一致しないEntityがあれば競合です。結果不明のまま `axon import prepare` でIDを再割当てしたり、雛形から登録し直したりしません。詳細は [保存結果](../reference/cli.md#mutationの結果) を参照してください。

## 並行作業と引継ぎ

無視する運用では全worktreeがmain worktreeの保存先を共有するため、同じ未着手Entityへの並行`axon start`は一つだけ成功します。失敗側は`axon show ID --skip-conditions`・`axon log`で現在値を読み、実行中のworkerと調整してください。記録者情報を所有権として扱わず、作業終了を確認してから継続・`axon release`を判断します。

追跡する運用ではworktreeごとに保存先が分かれるため、同じEntityに対してそれぞれ`axon start`を実行できます。Gitで取り込むまで互いの作業は見えません。取り込んだ後、同じEntityへの両側の操作は衝突として `Conflicted` に見え、解決するまで `axon resolve` と `axon note add` 以外の変更は拒否されます。`axon resolve ID` でheadを読み、`axon resolve ID --head RECORD_ID` で片方の現在値を選びます。両側のNoteと記録は残ります。統合が生んだ構造の違反（完了したGroupへの子の流入など）は `axon show` の `Invalid` と `axon storage check` で読み、`axon reopen` などの通常操作で直します。同じworktreeでGit更新とAxon書込みを並行しないでください。手順は [Git統合と検査](../development/lifecycle-file.md#git-統合と検査) にあります。

## 再浮上

`axon condition set ID --command 'test -f ready.txt'` は条件を保存し、`axon condition unset ID` は解除します。保存時には実行しません。`axon proposals|tasks` と既定の `axon show` が必要な条件を `/bin/sh -c` で評価し、終了0は成立、1は未成立、その他は一覧または表示の失敗です。`axon show ID --skip-conditions` は評価しません。`--condition-timeout` と `--trace-conditions` の契約は [外部条件](../reference/candidates.md) を参照してください。

IDはprefix＋ランダム8文字で、完全IDまたは一意なsuffixを指定できます。乱数部分が6文字の既存のIDもそのまま使えます。CLI生成文は英語、TTYでは意味に応じて装飾します。非TTY、非空の`NO_COLOR`、`TERM=dumb`、`--no-color`では装飾しません。例えば`axon --no-color tasks`または`axon tasks --no-color`で、その呼び出しのhelpと診断を含めて装飾を無効化できます。一覧の絞り込み・検索、Note個別参照と保存結果は [CLIと表示の契約](../reference/cli.md) を参照してください。

`axon list|tasks|proposals`の `--label` は現在のlabelで絞り込み、`--kind`・`--search` など他の絞り込みとANDで組み合わせます。`--search` は現在のtitle・descriptionだけを検索し、Noteだけの一致は返しません。Noteに残る情報は `axon note search` で読みます。終了Entityを含む全Noteから、完全ID・日時と最初の一致の前後24文字を1 Noteにつき1行で表示します。`Excerpt:` と `…` は抜粋・省略を示し、改行や制御文字は可視化します。原文は `axon note show ID NOTE_ID` で取得できます。検索は大小文字を区別するliteral部分一致で、空白の除去やUnicode正規化はしません。
