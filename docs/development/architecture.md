# 層の依存方向

振る舞いの契約は [docs/reference](../reference/lifecycle.md) の各文書、状態と遷移のモデルは [spec](../../spec/README.md) にある。crate と module の配置は [Cargo.toml](../../Cargo.toml) と各 crate の `lib.rs` から辿る。この文書は、コードの配置からは読み取れない層の境界と依存方向の規則を示す。

## 層の責務

- 共通コア（`crates/axon-core`）は、記録の集合からの導出、通常操作の前提検査と記録の生成、declaration、読み取り結果の構造化を担う。保存先・CLI・外部 process から独立し、同じ判断を adapter と CLI の両方から使えるようにする。
- 保存 adapter（root の `axon` library）は、記録 file・保存先の探索と初期化・declaration file の I/O を担い、検査と記録の生成はコアに任せる。
- CLI（`axon` binary）は、引数、描画、端末、外部条件の process、終了コードを担う。
- 記録者取得（`crates/axon-recorder`）は、継承された環境から任意の記録者情報を返すだけで、lifecycle の判断と保存を持たない。
- デスクトップアプリ（`crates/axon-gui`）は、登録した管理 root の記録を root library で読み、コアの値を表示用の文字列と GPUI の要素に変換する。記録を書かず、書くのはアプリのデータ領域にある登録の一覧と、次の起動で復元する画面の状態だけである。`axon gui` がアプリへ渡すリンクの形式は、CLI とアプリが同じ定義を使うよう root library が持つ。

## 依存方向の規則

- コアは保存 adapter、CLI、記録者取得に依存しない。filesystem、外部コマンド評価をコアへ持ち込まない。adapter はコアの操作・検査を呼び、lifecycle の意味論を保存形式や CLI に複製しない。
- `axon-core` の依存ツリーには、dev-dependencies を含めて `clap`、`axon-recorder`、root crate の `axon` を含めない。テストから root crate を参照して境界を逆転させることも禁止する。
- コアは `libc` を直接依存に持たない。既存の ID 生成は `rand` を使い、`rand → getrandom` 経由の OS 乱数・間接 `libc` 依存を許容する。「保存から独立」は環境入力がすべて注入済みであることを意味しない。
- root library はコアと保存に必要な filesystem・Git 呼出しの I/O に依存してよい。描画と条件 process は CLI 側に置く。CLI の module 間参照は必要な名前を明示し、glob import で親 module の名前空間を共有しない。

- GUI は root library とそれが再公開するコアに依存し、コア・root library・CLI は GUI と GPUI に依存しない。GPUI の型をコアと root library へ持ち込まない。GUI は workspace の `default-members` に含めず、CLI 単体のビルドに GPUI を要求しない。

`axon-core` の禁止 crate と直接の `libc` は、full verification の [scripts/check-core-deps](../../scripts/check-core-deps) が検査する。root crate 内の module 間境界はコードレビューで確認する。
