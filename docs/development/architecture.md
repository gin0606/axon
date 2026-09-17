# 実装の層構造

仕様の正本は [literate spec](../../spec/lifecycle_proposal.md)。この文書はコードの配置、依存方向、検証の入口を示す。操作や保存の詳細契約は末尾の参照先に置く。

## crate と module の地図

Cargo workspace は root の `axon`、`crates/axon-core`、`crates/axon-recorder` を持つ。いずれも `publish = false` で、通常の Cargo コマンドは workspace の既定メンバーを対象にする。

```text
axon binary: src/main.rs → src/cli/
    ├── axon library: src/lib.rs → 保存 adapter
    │       └── axon-core: lifecycle / declaration / read
    ├── axon-core（axon library の再公開経由）
    └── axon-recorder: 任意の記録者情報の取得

root build.rs → ビルド時の Git・環境情報 → binary の version 文字列
```

| 層 | 実装の入口 | 責務 |
| --- | --- | --- |
| 保存から独立したコア | [crates/axon-core/src/lib.rs](../../crates/axon-core/src/lib.rs) | `lifecycle` は `Snapshot`、通常操作・検査、候補導出、因果順、canonical codec、三者比較を扱う。`declaration` は YAML の解析・出力、fingerprint、差分と適用を扱う。`read` は一覧・詳細・履歴・Note の検索と構造化された読み取り結果を返す |
| 保存 adapter | [src/lib.rs](../../src/lib.rs) | `sqlite` と `file` がコアを呼び、保存の原子性と障害境界を担う。`location::Store` の enum dispatch が backend を選ぶ。`file_merge` は統合 workspace と Git driver、`declaration_file` は宣言ファイルと正本の I/O を接続する |
| adapter 共通エラー | [src/error.rs](../../src/error.rs) | root の `axon::Error` / `axon::Result` と prefix 検証。コアのエラーを包み、保存・I/O の失敗を表す。互換の `axon::sqlite::{Error, Result}` は再公開 |
| CLI | [src/main.rs](../../src/main.rs)、[src/cli/mod.rs](../../src/cli/mod.rs) | `main.rs` は入口だけを持つ。`cli::mod` が dispatch、stdout/stderr、保存後の出力失敗と終了コードを処理する |
| 記録者取得 | [crates/axon-recorder/src/lib.rs](../../crates/axon-recorder/src/lib.rs) | 環境や transcript から任意の記録者情報を取得する。コアの lifecycle 判断や保存 adapter を所有しない |

`src/lib.rs` は `axon_core::{lifecycle, declaration, read}` を再公開するため、root library の利用側も同じコア API を使う。保存 adapter の crate 分割や Repository trait は導入せず、backend の切替は `location::Store` に集約する。

### CLI の内訳と読み取りの流れ

| module | 責務 |
| --- | --- |
| `args` | Clap のコマンド・引数定義と操作名 |
| `read` | 保存済み snapshot の取得、コアの読み取り API の呼出し、描画への受渡し |
| `write` | 登録、本文・関係・条件・状態変更、Note 追記、declaration import の接続 |
| `setup` | 初期化、保存検査、merge、docs、completion、記録者表示 |
| `store` | 保存先の取得、ID 解決、新規 ID の衝突確認 |
| `condition` | 候補の外部条件を実行する process、timeout、trace、呼出し内の評価結果保持 |
| `render` | 読み取り結果から CLI の文字列を組み立てる |
| `display` | 端末装飾、制御文字の可視化、表示時刻などの表示規則 |

読み取りは「保存 adapter → `Snapshot` → `axon::read` の構造化結果 → `cli::render` → 出力」と進む。コアの `read` は CLI の英語ラベル、ANSI、端末への出力を持たず、候補条件の評価は呼出し側の callback を受け取る。shell の実行は `cli::condition` に残す。保存された条件文字列と、その呼出しで得た評価結果を混同しない。

## 依存方向の規則

- コアは保存 adapter、CLI、記録者取得に依存しない。SQL、filesystem、外部コマンド評価をコアへ持ち込まない。adapter はコアの操作・検査を呼び、lifecycle の意味論を SQL や CLI に複製しない。
- `axon-core` の依存ツリーには、dev-dependencies を含めて `rusqlite`、`clap`、`axon-recorder`、root crate の `axon` を含めない。テストから root crate を参照して境界を逆転させることも禁止する。
- コアは `libc` を直接依存に持たない。既存の ID 生成は `rand` を使い、`rand → getrandom` 経由の OS 乱数・間接 `libc` 依存を許容する。「保存から独立」は環境入力がすべて注入済みであることを意味しない。
- root library はコアと保存に必要な I/O・SQL に依存してよいが、binary 専用の `cli` module には依存しない。描画と条件 process は CLI 側に置く。CLI の module 間参照は必要な名前を明示し、glob import で親 module の名前空間を共有しない。

Cargo の crate 境界を次で確認する。既定の出力は dev-dependencies も含む。最初のツリーで上記の禁止 crate がないこと、深さ 1 の出力で直接の `libc` がないことを検査する。root crate 内の module 間境界はコードレビューで確認する。

```sh
cargo tree -p axon-core
cargo tree -p axon-core --depth 1
```

## 各層の検証入口

| 対象 | コマンドと fixture |
| --- | --- |
| コア | `cargo test --locked -p axon-core`。`lifecycle`、`declaration`、`read` のメモリ上の単体テスト |
| 保存 adapter | `cargo test --locked -p axon --lib`。`src/sqlite.rs`、`src/file.rs`、`src/location.rs`、`src/file_merge.rs`、`src/declaration_file.rs` と関連 test module の保存・障害・process fixture |
| CLI 内部 | `cargo test --locked -p axon --bin axon`。表示、ID 解決、外部条件 process の単体テスト |
| 公開 CLI と backend の接続 | `cargo test --locked --test smoke`。`tests/smoke.rs` が `tests/lifecycle/{workflow,file,contracts,declaration}.rs` も読み込み、両 backend の独立 fixture、実 Git worktree、公開出力を検証する |
| 記録者取得 | `cargo test --locked -p axon-recorder`。検出と transcript 取得の単体テスト |

層の整理でも公開コマンド・引数・出力 bytes・終了コード、lifecycle の意味論、canonical bytes は維持する。全体検証とモデルを再検証する条件は [検証方針](verification.md) に従う。

## 詳細契約の参照先

- [共通コア](lifecycle-core.md): 通常操作、情報・因果順、codec、三者比較の意味と制約。
- [Declaration](lifecycle-declaration.md): YAML と一括適用の契約、コアとファイル書戻しの境界。
- [SQLite CLI](lifecycle-sqlite.md): 公開操作、SQLite transaction、保存先探索と失敗境界。
- [file保存とGit統合](lifecycle-file.md): writer、公開、worktree、統合 workspace と Git driver。
- [候補と外部条件](lifecycle-candidates.md)、[記録者連携](lifecycle-recorder.md)、[CLI入出力契約](../reference/lifecycle-cli.md): 外部 process、任意 metadata、表示と入出力の詳細。

## ビルド時のソース由来

`--version` / `-V` は package version を先頭に、ビルド時の完全 commit hash と
`source clean|modified|unknown` を補足する。build script が文字列を埋め込み、実行時は
Git・環境変数・DB を参照しない。canonical build も通常の Cargo build と同じ仕組みを使う。

配布工程では `AXON_BUILD_COMMIT`（40 桁または 64 桁の ASCII 十六進数、または `unknown`）と
`AXON_BUILD_SOURCE_STATE`（`clean`、`modified`、`unknown`）を指定できる。
どちらか一方でも存在すれば明示指定モードとし、省略した項目は `unknown` にする。
Git 自動取得とは混ぜない。不正値・空文字・非 Unicode 値は警告して当該項目を `unknown` とする。
たとえば上流 commit に配布側パッチを当てる場合は、その commit と `modified` を明示する。

指定がなければ Cargo package root 自身が Git worktree root の場合だけ自動取得する。
linked worktree にも対応する。親 repository 内へ展開した source archive は親の HEAD を採用しない。
Git を見つけられない場合や各照会が失敗した場合は、取得不能な項目を `unknown` にする。
Git の repository/index/pathspec を切り替える環境変数は照会から除外する。

modified は staged / unstaged の tracked file と Git が無視しない untracked file を対象にする。
package root の `.axon`（管理データ）と `target`（標準 build artifact）は tracked でも除外する。
その他の生成物は `.gitignore` 等で除外する。無視されたソース、Git の assume-unchanged /
稼働中の並行編集などまで検知する保証はない。Git 照会は build 開始時の観測である。
commit のみ、変更の有無のみ、metadata のみの変化でも情報を更新するため、build script は
毎回の Cargo build で再実行する（由来の照会と axon crate 再コンパイルのコストがある）。

これは完全なソース内容や binary の同一性の証明ではない。正式 release では version が通常の
識別の主となり、commit は補助情報である。commit と modified だけでは配布側のパッチ、
ビルド設定、未 commit 差分の内容を識別できない。厳密な監査では binary hash と package 情報も記録する。
