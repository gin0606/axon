# file 保存と Git 統合

[file](../../src/file.rs) が記録 file の読取と公開、[location](../../src/location.rs) が保存先の探索と初期化を担う。操作は [共通コア](lifecycle-core.md) に検査させる。保存先の選択と統合の意味論は [保存と統合の契約](../reference/storage.md) に定める。

## 初期化と探索

探索順・確定条件・Git index の検査対象は [探索](../reference/storage.md#探索)、作成できる場所と残骸の扱いは [保存先と初期化](../reference/storage.md#保存先と初期化) を参照する。無視する運用と追跡する運用の手順は [利用ガイド](../guide/storage.md) にある。

探索で確定した保存先と、それを含む worktree を後続の I/O の対象にする。保存先の発見は CLI 実行ごとに一回で、実行途中の Git の toplevel や common directory の変更は検出しない。Git 呼出し時の環境と repository 境界は [CLI と保存の接続](lifecycle-cli.md#内部git呼出し) に従う。

初期化は worktree をまたいで直列化する。lock は Git 内では common Git directory の `axon-init.lock`、Git 外では管理 directory の `.axon/axon-init.lock` に置く。header の公開を最後にすることで、公開前に中断した保存先を未初期化として再実行できるようにする。

## 記録 file と codec

header file `.axon/header.json` は 1 行の JSON で、`{"format":"axon-records/v2","store":"store-…","prefix":"demo"}` の形とする。store ID は `axon init` が乱数で生成する。未知 format の読取や暗黙変換はしない。

記録 file は `.axon/records/<記録 ID の先頭 2 文字>/<記録 ID>` に置き、内容は canonical な 1 行の JSON と末尾の LF 一つである。記録 ID は file の bytes 全体の BLAKE3 hash の小文字 16 進 64 文字で、file 名と一致する。JSON の object のキーは決定的な順、空白なし、文字列の escape は最小、記録者 metadata の JSON 数値は任意精度の表現で保持し、整数の桁あふれや小数の丸めで内容や記録の同一性を変えない。同じ内容の記録は同じ bytes に encode され、同じ記録 ID になる。

記録は次の項目をこの順で持つ。

| key | 内容 |
| --- | --- |
| `entity` | 対象の Entity ID |
| `record` | 種類。`created`、`transition`、`edit`、`label`、`parent`、`dependency`、`condition`、`convert`、`import`、`resolve`、`note` |
| `operation` | `transition` だけ。`accept`、`withdraw`、`start`、`release`、`complete`、`cancel`、`reconsider`、`reopen` |
| `parents` | 親記録の ID の list。`created` と `note` は `[]`、`resolve` は全 head、それ以外は一つ |
| `nonce` | `note` だけ。乱数の小文字 16 進 32 文字。同じ本文・日時・記録者の Note を別の記録にする |
| `chosen` | `resolve` だけ。採った head の記録 ID |
| `at` | UTC の RFC 3339 日時（`Z`。小数秒は 0 なら省き、それ以外は 3・6・9 桁のうち値を表せる最短の桁数）。writer が入力表記の offset を UTC に正規化してから書く |
| `recorder` | `{"actor":"…","data":{…}}` または null |
| `reason` | 任意の理由の文字列または null |
| `after` | `note` 以外。操作後の現在値 `{"kind","lifecycle","owner","title","description","label","condition","parent","needs"}`。`kind` は `issue`・`group`、`lifecycle` は `undecided`・`not-started`・`in-progress`・`completed`・`cancelled`（declaration と同じ綴り）。`label` は `bug`・`feat`・`chore`・`docs`・`test`・`refactor`・`spike` のいずれかで常に書き、欠落と集合外の値は拒否する。`import` は `axon import apply` が既存 Entity に書く記録で、title・description・label・parent・needs の変更をまとめて一つの現在値で持つ。`owner` は `InProgress` の Issue の着手した actor（取得できなければ null）で、それ以外の状態では null。`condition` と `parent` は未設定なら null、`needs` は Entity ID の昇順の list |
| `body` | `note` だけ。Note の本文 |

decode は未知の key、欠けた key、種類と合わない key、規則外の値（title・reason の長さと文字種、ID の文字種、`InProgress` の Group、`InProgress` 以外の owner、`start` の記録者と異なる owner、その種類にありえない遷移）と、内容を encode した結果と一致しない bytes（空白、キーの順、日時や数値の綴りが違う file）を拒否する。記録 ID は bytes の hash なので、canonical でない bytes を受け入れると同じ内容が別の ID を持つことになる。親記録との照合は [記録の集合の検査](lifecycle-core.md#記録の集合と導出) が担う。いずれの違反も破損として扱う。

保存 adapter は file 名と hash、配置を照合する。一時 file の除外、破損による読取停止、改行変換が疑われる場合の診断は [保存先の破損](../reference/storage.md#保存先の破損) に従う。

## 書込の保証

通常の writer は、同じ OS lock の下で読取・検査・記録の公開を行う。lock と既存記録の不変性は [書込の契約](../reference/storage.md#書込) に定める。公開処理では次の順序を守る。

- 一時 file の内容と、記録を格納する directory の entry を同期してから、記録 ID への rename で公開する。directory は毎回同期し、中断した writer が作って未同期のまま残した entry も永続化する。
- 途中の同期は後続の書込より先に届く順序を保証し、最後の同期で全体の永続化を保証する。プラットフォームごとの実現方法は [file adapter](../../src/file.rs) にある。
- rename 後に失敗した場合も、公開済みの記録を永続化するために同期を試みる。

rename 前の失敗は `not applied`、rename 後の directory sync の失敗は `result unknown` と区別する。結果不明なら process 終了を確認し、記録の集合を読み直して記録の有無を照合する。Note や登録を推測で再実行しない。保存後の出力失敗は [CLI の失敗境界](lifecycle-cli.md#mutationと出力の境界) で扱う。

Git や editor は OS lock に従わないため、同じ worktree で checkout・merge・editor 保存と Axon の書込を並行しない。最終 sync 直後の非協調書込や network filesystem の透過的な保証は対象外。改行を含む path に置かれた Git worktree は保存先の発見でエラーにする。

## Git 統合と検査

追跡する運用では、両側が追加した記録 file を Git が merge の属性なしで統合する。Axon は Git から呼ばれず、`.axon/.gitattributes` の `* -text` 以外の属性と Git config を持たない。統合の結果は次の読取と `axon storage check` が検査する。

```sh
axon storage check
axon resolve
axon resolve ID --head RECORD_ID -r '残作業のある側を採る'
axon storage check
```

`axon storage check` の報告と終了コード、明示した管理 root の検査は [検査と解決の入口](../reference/storage.md#検査と解決の入口) と [CLI 契約](../reference/cli.md#衝突違反と解決) に従う。条件コマンドは実行せず、保存先も変更しない。`axon resolve` は通常 writer と同じ lock と公開処理を使い、共通コアが生成した解決記録を保存する。残った違反は通常操作で直す。

記録 file には本文に加え取得できた記録者情報が入り、Git で追跡するとこれらも共有される。保存項目は [記録者連携](lifecycle-recorder.md) を参照する。検証の入口は [各層の検証入口](architecture.md#各層の検証入口) にある。

## Declaration の一括反映

`axon import apply` は一回の lock の下で複数の記録を公開する。全件の一時 file を同期してから、新規 Entity の登録を親と依存先が先になる順に、次いで既存 Entity の変更を公開する。公開途中の失敗は、公開済みの記録を残した結果不明として扱う。

保存先と declaration の書戻しは別の保存境界である。接続時の注意点は [Declaration と保存の境界](lifecycle-declaration.md)、部分適用後の判定は [再試行の契約](../reference/declaration.md#再試行) に定める。
