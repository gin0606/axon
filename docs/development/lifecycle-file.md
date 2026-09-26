# file 保存と Git 統合

[保存と統合の契約](../reference/storage.md) を `src/file.rs` と `src/location.rs` が実装する。通常操作は [CLI と保存の接続](lifecycle-cli.md) と同じ共通コアの記録の集合からの導出・前提検査・記録の生成を通す。未知の format は自動変換しない。試用・検証は独立 fixture で行う。

## 初期化と探索

`axon init demo` は現在の管理 root に `.axon/records/`、`.axon/.gitignore`、`.axon/.gitattributes`、`.axon/header.json` を新規作成する。Git 内では現在の worktree root、Git 外では現在 directory を対象とし、Git 外で既存管理 root 内への入れ子の `axon init` は拒否する。`.axon/` に header、記録、内容の異なる `.gitignore` か `.gitattributes`、以前の形式の `state.jsonl` などがあれば、その path を示して拒否する。lock、`.tmp` で終わる file、空の `records/`、同じ内容（CRLF を LF と読んで比べる）の `.gitignore` と `.gitattributes` だけなら中断した初期化の残骸として作り直し、CRLF を含む file は `axon init` が書く内容で置き換える。repository root の `.gitignore`・`.gitattributes`・Git config を作成も変更もせず、stage も commit もしない。Git 内では初期化した保存先が untracked に見えることと、無視する運用・追跡する運用それぞれの手順、Axon の状態の取り消しに revert を使わないことを表示する。

```gitignore
# .axon/.gitignore
*.lock
*.tmp
```

```gitattributes
# .axon/.gitattributes
* -text
```

`.axon/.gitattributes` は checkout 時の改行変換で記録 file が破損になるのを防ぐ。目的と効く範囲は [保存と統合の契約](../reference/storage.md#保存先と初期化) にある。

無視する運用は、利用者が `.git/info/exclude` や global の ignore file に `.axon/` の行を書いて選ぶ。追跡する運用は、利用者が `git add .axon` で記録、header、`.axon/.gitignore`、`.axon/.gitattributes` を stage して commit して選ぶ。二つの運用は一つの repository では混ぜない。無視されている保存先は Git の checkout・merge の上書き保護を受けず、警告なしに置き換わる。Axon はこの混在を検出しない。理由と境界は [保存と統合の契約](../reference/storage.md#無視する運用と追跡する運用) にある。

探索は Git 内では現在の worktree root の `.axon`、次に main worktree の `.axon` の順に見る。2 段目は linked worktree で、Git common directory が main worktree 直下の `.git` directory である場合だけ使い、bare repository に付けた worktree と submodule では使わない。各段は `header.json` があれば確定し、header がなく記録やその他の file（以前の形式の `state.jsonl` を含む）があれば破損または未知の format として停止し、中断した初期化の残骸しかない `.axon` では確定せず次へ進む。確定した保存先が破損・読取不能なら停止し、別の保存先へ fallback しない。Git 外では最寄りの `header.json` を持つ祖先で止まる。段の意味と、追跡する運用で保存先を持たない branch の linked worktree が main worktree の保存先に書く副作用は [保存と統合の契約](../reference/storage.md#探索) に定める。

`axon init` は 2 段目を使わず、Git 内では現在の worktree root だけを対象にする。2 段目が働く構成の linked worktree での `axon init` は、main worktree に保存先があればその path を示して拒否し、なければ作成したうえで他の worktree からは見えないことを表示する。main worktree の判定は、common directory の親で `git rev-parse --is-bare-repository` を含む一回の Git 呼出しで行う。bare repository なら 2 段目を使わず、それ以外の理由で Git が失敗した場合は未初期化として扱わずにエラーとする。

OS lock、Git index の unmerged 検査、管理 directory が通常の directory であること（symlink の拒否）の検査は、確定した保存先とそれを含む worktree に対して行う。通常 writer の lock は確定した保存先の `.axon/write.lock`、`axon init` の lock は Git 内では common Git directory の `axon-init.lock`、Git 外では管理 directory の `.axon/axon-init.lock` で、worktree をまたぐ並行初期化も直列化する。

`axon init` はその OS lock 下で既存の保存先を確認し、記録の directory を作り、`.axon/.gitignore` と `.axon/.gitattributes` をそれぞれ一時 file から rename で作り、header を一時 file `header.json.tmp` に書いて sync し、`header.json` へ rename して directory を sync する。header の公開前に中断した保存先は未初期化のままで、再実行で作り直せる。

## 記録 file と codec

header file `.axon/header.json` は 1 行の JSON で、`{"format":"axon-records/v1","store":"store-…","prefix":"demo"}` の形とする。store ID は `axon init` が乱数で生成する。未知 format の読取や暗黙変換はしない。

記録 file は `.axon/records/<記録 ID の先頭 2 文字>/<記録 ID>` に置き、内容は canonical な 1 行の JSON と末尾の LF 一つである。記録 ID は file の bytes 全体の BLAKE3 hash の小文字 16 進 64 文字で、file 名と一致する。JSON の object のキーは決定的な順、空白なし、文字列の escape は最小、記録者 metadata の JSON 数値は任意精度の表現で保持し、整数の桁あふれや小数の丸めで内容や記録の同一性を変えない。同じ内容の記録は同じ bytes に encode され、同じ記録 ID になる。

記録は次の項目をこの順で持つ。

| key | 内容 |
| --- | --- |
| `entity` | 対象の Entity ID |
| `record` | 種類。`created`、`transition`、`edit`、`parent`、`dependency`、`condition`、`convert`、`import`、`resolve`、`note` |
| `operation` | `transition` だけ。`accept`、`withdraw`、`start`、`release`、`complete`、`cancel`、`reconsider`、`reopen` |
| `parents` | 親記録の ID の list。`created` と `note` は `[]`、`resolve` は全 head、それ以外は一つ |
| `nonce` | `note` だけ。乱数の小文字 16 進 32 文字。同じ本文・日時・記録者の Note を別の記録にする |
| `chosen` | `resolve` だけ。採った head の記録 ID |
| `at` | UTC の RFC 3339 日時（`Z`。小数秒は 0 なら省き、それ以外は 3・6・9 桁のうち値を表せる最短の桁数）。writer が入力表記の offset を UTC に正規化してから書く |
| `recorder` | `{"actor":"…","data":{…}}` または null |
| `reason` | 任意の理由の文字列または null |
| `after` | `note` 以外。操作後の現在値 `{"kind","lifecycle","owner","title","description","condition","parent","needs"}`。`kind` は `issue`・`group`、`lifecycle` は `undecided`・`not-started`・`in-progress`・`completed`・`cancelled`（declaration と同じ綴り）。`import` は `axon import apply` が既存 Entity に書く記録で、title・description・parent・needs の変更をまとめて一つの現在値で持つ。`owner` は `InProgress` の Issue の着手した actor（取得できなければ null）で、それ以外の状態では null。`condition` と `parent` は未設定なら null、`needs` は Entity ID の昇順の list |
| `body` | `note` だけ。Note の本文 |

decode は未知の key、欠けた key、種類と合わない key、規則外の値（title・reason の長さと文字種、ID の文字種、`InProgress` の Group、`InProgress` 以外の owner、`start` の記録者と異なる owner、その種類にありえない遷移）と、内容を encode した結果と一致しない bytes（空白、キーの順、日時や数値の綴りが違う file）を拒否する。記録 ID は bytes の hash なので、canonical でない bytes を受け入れると同じ内容が別の ID を持つことになる。遷移元を要する検査（その時点の種類の規則に反する遷移、その種類が変えてよい項目以外の変更。[記録の集合と導出](lifecycle-core.md#記録の集合と導出)）は記録の集合の導出で親記録の現在値と照合して行い、親記録が欠けていれば行わない。いずれの違反も破損として扱う。記録 ID は内容に含めず、bytes から計算する。途中で切れた file、空の file、名前と hash が一致しない file、名前が記録 ID の形でない file は保存先の破損として [保存と統合の契約](../reference/storage.md#保存先の破損) に従って報告し、読取を止める。名前と hash が一致しない file の内容が CRLF を LF に戻すと名前の hash と一致するなら、その file の理由にその旨を書き、報告の末尾に改行変換の可能性と利用ガイドへの案内を添える。内容は戻さない。名前が `.tmp` で終わる file は無視する。

記録の集合からの導出（head、衝突、settled、現在値、実効 lifecycle、gap、違反）は共通コアの [記録の集合](lifecycle-core.md#記録の集合と導出) が行い、file の列挙順に依存しない。同じ日時の Note の表示順など同順位の並びは記録 ID で固定する。

## 書込の保証

writer は `.axon/write.lock` の OS lock を取得してから記録の集合を読み、通常操作の前提を検査して新しい記録を一つ作る。記録 ID を計算し、目的の subdirectory を必要なら作り、`<記録 ID>.tmp` に bytes を書いて sync し、`<記録 ID>` へ rename して directory を sync して成功する。既存の記録 file は書き直さず、置き換えず、削除しない。lock file は置換・削除しない。process 終了時は OS が lock を解放する。同値の操作は記録を作らず No changes で終わる。保存先の発見は CLI 実行ごとに一回で、実行の途中で Git の toplevel や common directory が変わったことは検出しない。

rename 前の失敗は `not applied`、rename 後の directory sync の失敗は `result unknown` と区別する。結果不明なら process 終了を確認し、記録の集合を読み直して記録の有無を照合する。Note や登録を推測で再実行しない。出力失敗は `storage applied; output failed` で区別する。

通常読取も記録の集合全体を読み、破損・衝突・違反・gap を導出する。Git index で `.axon/` の下の path が unmerged なら内容が有効でも拒否する。改行を含む path に置かれた Git worktree は保存先の発見でエラーにする。Git や editor は OS lock に従わないため、同じ worktree で checkout・merge・editor 保存と Axon の書込を並行しない。最終 sync 直後の非協調書込や network filesystem の透過的な保証は対象外。

## Git 統合と検査

追跡する運用では、両側が追加した記録 file を Git が merge の属性なしで統合する。Axon は Git から呼ばれず、`.axon/.gitattributes` の `* -text` 以外の属性と Git config を持たない。統合の結果は次の読取と `axon storage check` が検査する。

```sh
axon storage check
axon resolve
axon resolve ID --head RECORD_ID -r '残作業のある側を採る'
axon storage check
```

`axon storage check [ROOT]` は破損、衝突、違反、gap を報告し、破損・衝突・違反のいずれかがあれば非 0 で終了する（gap は情報）。破損があれば記録から導出する検査は行わない。引数なしでは探索で確定した保存先、`ROOT` を与えればその管理 root を探索せずに検査する。条件コマンドを実行せず、保存先を変更しない。

`axon resolve ID --head RECORD_ID` は通常の writer と同じ lock と境界で解決記録を一つ書く。head の一覧は保存先の現在の記録から計算し、指定した記録 ID が対象 Entity の head でなければ拒否する。解決記録の後に残る違反は通常操作で直す。cherry-pick と revert による gap の扱いと、偽の衝突の解決は [保存と統合の契約](../reference/storage.md#記録の欠けgap) に定める。

記録 file には本文に加え取得できた記録者情報が入る。Git で追跡するとこれらも共有される。記録者の保存項目は [記録者連携](lifecycle-recorder.md) を参照する。

検証入口は `cargo test --workspace --lib --bin axon --test smoke`。並行 writer、rename 前後の障害、初期化と探索、実 worktree での merge・rebase・cherry-pick・revert・squash、index guard、破損の報告を独立 fixture で扱う。保存と CLI 接続で lifecycle の意味は変更せず、Quint の状態を増やさない。統合の意味論は [`spec/record_integration.qnt`](../../spec/record_integration.qnt) が扱う。

## Declaration の一括反映

`axon import apply FILE` は `src/declaration_file.rs::apply` から共通コアの通常操作の列で候補を検証し、OS lock 取得後に declaration を読み、全件の検証と一回の保存境界で記録 file を追加する。書く記録は変更のある Entity ごとに一つで、新規 Entity は最終値（parent・needs を含む）を持つ `created`、既存 Entity は最終値を持つ `import` とし、検証に使った通常操作の列を個々の記録にはしない。一つの保存境界で複数の記録 file を作る唯一の経路で、全件の一時 file を書いて sync してから、新規 Entity の `created` を親と依存先が先になる順に、次いで既存 Entity の `import` の順に rename し、rename の失敗で途中で止まった場合は rename 済みの記録を残したまま `result unknown` と診断し、process の喪失で止まった場合も同じ状態になる。Entity ごとに記録が一つなので、rename 済みの Entity は最終値、未 rename の Entity は元の値のままであり、再試行は [Declaration](../reference/declaration.md#再試行) の Entity ごとの適用済み判定で残りを反映する。衝突・違反のある保存先では拒否するが、途中で止まった反映の再試行では、残りの反映で消える違反を拒否の対象にしない。保存成功後の declaration rewrite は別の保存境界であり、元 bytes の再照合、rename、directory sync を行う。保存先 Applied と declaration 未更新・結果不明を別々に表示する。再試行は全編集 Entity の最終値一致なら記録を増やさず、保存した記録の集合から base と外部参照を更新する。
