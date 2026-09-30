# 実装の層構造

振る舞いの契約は [docs/reference](../reference/lifecycle.md) の各文書、状態と遷移のモデルは [spec](../../spec/README.md) にある。この文書はコードの配置、依存方向、検証の入口を示す。操作や保存の詳細契約は末尾の参照先に置く。

## crate と module の地図

Cargo workspace は root の `axon`、`crates/axon-core`、`crates/axon-recorder`、`crates/axon-label-conversion` を持つ。いずれも `publish = false` で、通常の Cargo コマンドは workspace の既定メンバーを対象にする。`axon-label-conversion` は既定メンバーに含めないので、`cargo run` は `axon` を実行し、ツールは `-p axon-label-conversion` で指定する。`--workspace` の検証には含まれる。

```text
axon binary: src/main.rs → src/cli/
    ├── axon library: src/lib.rs → 保存 adapter
    │       └── axon-core: lifecycle / declaration / read
    ├── axon-core（axon library の再公開経由）
    └── axon-recorder: 任意の記録者情報の取得
axon-label-conversion binary: 一度限りの保存先の変換
    └── axon-core: 記録の codec
```

| 層 | 実装の入口 | 責務 |
| --- | --- | --- |
| 保存から独立したコア | [crates/axon-core/src/lib.rs](../../crates/axon-core/src/lib.rs) | `lifecycle` は記録の集合からの導出（head・衝突・現在値・違反・gap）、通常操作の前提検査と記録の生成、解決記録、候補導出、因果順、記録 1 件の canonical codec を扱う。`declaration` は YAML の解析・出力、fingerprint、差分と適用を扱う。`read` は一覧・詳細・履歴・Note の検索と構造化された読み取り結果を返す |
| 保存 adapter | [src/lib.rs](../../src/lib.rs) | `file` がコアを呼び、`.axon/records/` の記録 file の列挙・hash の照合・作成の原子性と障害境界を担う。`location` は保存先の探索と `axon init`、`declaration_file` は宣言ファイルと記録の集合の I/O を接続する |
| adapter 共通エラー | [src/error.rs](../../src/error.rs) | root の `axon::Error` / `axon::Result` と prefix 検証。コアのエラーを包み、保存・I/O の失敗を表す |
| CLI | [src/main.rs](../../src/main.rs)、[src/cli/mod.rs](../../src/cli/mod.rs) | `main.rs` は入口だけを持つ。`cli::mod` が dispatch、stdout/stderr、保存後の出力失敗と終了コードを処理する |
| 記録者取得 | [crates/axon-recorder/src/lib.rs](../../crates/axon-recorder/src/lib.rs) | 継承された環境変数だけから任意の記録者情報を取得する。コアの lifecycle 判断や保存 adapter を所有しない |
| label 導入前の保存先の変換 | [crates/axon-label-conversion/src/lib.rs](../../crates/axon-label-conversion/src/lib.rs) | `axon-records/v1` の記録を自前の最小の読み手で読み、label を足した記録をコアの encoder で因果順に書き直して新しい管理 root へ書き出し、変換前と照合する。保存 adapter と CLI に依存しない一度限りのツールで、手順は [保存先と worktree](../guide/storage.md#labelを導入する前の保存先を変換する) |

`src/lib.rs` は `axon_core::{lifecycle, declaration, read}` を再公開するため、root library の利用側も同じコア API を使う。保存形式は記録 1 件 1 file の一つで、保存 adapter の crate 分割や Repository trait、形式を選ぶ dispatch は持たない。保存先の選択は `location` の探索に集約する。Git の統合に介入する仕組みは持たず、統合の結果は読取時の導出が検査する。

### CLI の内訳と読み取りの流れ

| module | 責務 |
| --- | --- |
| `args` | Clap のコマンド・引数定義と操作名 |
| `read` | 記録の集合の取得、コアの読み取り API の呼出し、描画への受渡し |
| `write` | 登録、本文・関係・条件・状態変更、種類の変換、Note 追記、衝突の解決、`axon import` の接続 |
| `setup` | 初期化、保存検査、`axon docs`、`axon completion`、記録者表示 |
| `store` | 保存先の取得、ID 解決、新規 ID の衝突確認 |
| `condition` | 候補の外部条件を実行する process、timeout、trace、呼出し内の評価結果保持 |
| `render` | 読み取り結果から CLI の文字列を組み立てる |
| `display` | 端末装飾、制御文字の可視化、表示時刻などの表示規則 |

読み取りは「保存 adapter → 記録の集合 → コアの view → `axon::read` の構造化結果 → `cli::render` → 出力」と進む。コアの `read` は CLI の英語ラベル、ANSI、端末への出力を持たず、候補条件の評価は呼出し側の callback を受け取る。shell の実行は `cli::condition` に残す。保存された条件文字列と、その呼出しで得た評価結果を混同しない。

## 依存方向の規則

- コアは保存 adapter、CLI、記録者取得に依存しない。filesystem、外部コマンド評価をコアへ持ち込まない。adapter はコアの操作・検査を呼び、lifecycle の意味論を保存形式や CLI に複製しない。
- `axon-core` の依存ツリーには、dev-dependencies を含めて `clap`、`axon-recorder`、root crate の `axon` を含めない。テストから root crate を参照して境界を逆転させることも禁止する。
- コアは `libc` を直接依存に持たない。既存の ID 生成は `rand` を使い、`rand → getrandom` 経由の OS 乱数・間接 `libc` 依存を許容する。「保存から独立」は環境入力がすべて注入済みであることを意味しない。
- root library はコアと保存に必要な filesystem・Git 呼出しの I/O に依存してよいが、binary 専用の `cli` module には依存しない。描画と条件 process は CLI 側に置く。CLI の module 間参照は必要な名前を明示し、glob import で親 module の名前空間を共有しない。

Cargo の crate 境界を次で確認する。既定の出力は dev-dependencies も含む。最初のツリーで上記の禁止 crate がないこと、深さ 1 の出力で直接の `libc` がないことを検査する。root crate 内の module 間境界はコードレビューで確認する。

```sh
cargo tree -p axon-core
cargo tree -p axon-core --depth 1
```

## 各層の検証入口

| 対象 | コマンドと fixture |
| --- | --- |
| コア | `cargo test --locked -p axon-core`。`lifecycle`、`declaration`、`read` のメモリ上の単体テスト |
| 保存 adapter | `cargo test --locked -p axon --lib`。`src/file.rs`、`src/location.rs`、`src/declaration_file.rs` と関連 test module の保存・障害・process fixture |
| CLI 内部 | `cargo test --locked -p axon --bin axon`。表示、ID 解決、外部条件 process の単体テスト |
| 公開 CLI と保存の接続 | `cargo test --locked --test smoke`。`tests/smoke.rs` が `tests/lifecycle/{workflow,file,location,contracts,declaration,label_conversion}.rs` も読み込み、独立 fixture、実 Git worktree、公開出力を検証する。`label_conversion.rs` は binary が書いた保存先を v1 の形に戻して変換し、元の bytes に戻ることと拒否する入力を検証する |
| 記録者取得 | `cargo test --locked -p axon-recorder`。環境変数からの検出の単体テスト |
| label 導入前の保存先の変換 | `cargo test --locked -p axon-label-conversion`（対応 file の解析と label の挿入位置の単体テスト）と `cargo test --locked --test smoke label_conversion`。既定メンバーではないので、オプションなしの `cargo test` には含まれない |

層の整理でも公開コマンド・引数・出力 bytes・終了コード、lifecycle の意味論、canonical bytes は維持する。全体検証とモデルを再検証する条件は [検証方針](verification.md) に従う。

## 詳細契約の参照先

- [共通コア](lifecycle-core.md): 通常操作、記録の集合と導出、衝突と解決、codec の意味と制約。
- [Declaration](lifecycle-declaration.md): YAML と一括適用の契約、コアとファイル書戻しの境界。
- [CLIと保存の接続](lifecycle-cli.md): 公開操作、mutation と出力の境界、内部 Git 呼出し。
- [file保存とGit統合](lifecycle-file.md): 初期化と探索、記録 file と codec、writer、worktree、`axon storage check` と `axon resolve`。
- [候補と外部条件](../reference/candidates.md)、[記録者連携](lifecycle-recorder.md)、[CLI入出力契約](../reference/cli.md): 外部 process、任意 metadata、表示と入出力の詳細。
