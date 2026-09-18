# 検証方針とモデル検査

## Rust toolchain

通常の開発と検証には `rust-toolchain.toml` で固定した Rust を使う。mise を使う場合も Rust の idiomatic version file としてこの指定を読み込む。ソースからのビルドに必要な最低 Rust version（MSRV）は、`Cargo.toml` の `rust-version` を正本とする。

MSRVを変更するときは、`Cargo.toml`、[導入ガイド](../guide/getting-started.md#インストールと対応環境)、以下の検証コマンドを同じ変更で更新する。

## Full verification

pull requestとmainへのpushでは、GitHub Actionsがrepositoryの `rust-toolchain.toml` を使って次のfull verificationを個別のstepとして実行する。ローカルでも同じcommandを順に実行する。

```sh
cargo fmt --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
```

CIのcacheはCargo dependencyとbuild artifactだけに使い、成功済みのtest結果を根拠にfull verificationを省略しない。

## Fast pre-commit gate

Rust fileがstagedされているcommitでは、Lefthookがrustfmt、全target・全featureのClippyと、次の高速test集合を並列に実行する。

```sh
cargo test --workspace --lib --bin axon --test smoke
```

`--lib` は単一 lifecycle の共通コア・分岐・codec のテストを実行する。`--bin axon` は端末表示と条件プロセスの起動・trace flush失敗、`--test smoke` はSQLite/file binary の登録から Group 完了、Note・log、並行操作、schema 拒否、保存先探索・`axon init`、入出力失敗を独立 fixture で検証する。

`cargo test` はworkspaceの記録者crate単体テストも実行する。file保存・統合は `tests/lifecycle/file.rs` をsmokeから実行し、実worktree、driver、index、並行writer、drift拒否を検証する。

オプションなしの`cargo test`は引き続きsmokeを含む全test targetの標準入口であり、上記を含まない契約はfull verificationで検査する。Lefthookの各jobは失敗時にcommitを拒否し、staged Rust fileがない場合は既存の`*.rs` globによってRust検証を省略する。

リリース前には通常 toolchain の全検証に加え、MSRVで次を実行する。

```sh
cargo +1.89.0 check --locked --all-targets --all-features
```

## Rust coverage

`mise.toml` で固定した `cargo-llvm-cov` を使い、全target・全featureを次の一つのcommandで計測する。数値thresholdは設けず、未到達箇所を次の改善判断へ使う。

```sh
cargo llvm-cov --locked --all-targets --all-features --summary-only
```

全test targetを実行し、coverage用にtest自体をskipしない。意図的に途中終了させる子processだけは、merge不能なprofileを生成しないようcoverage出力を破棄する。親testは通常どおり実行し、終了code、lock解放、公開済みfileの完全性、失敗境界を検証する。通常の `cargo test` ではこのprofile制御は作用しない。

## モデルと運用検証

モデルで検証する状態・関係・候補・情報の意味論は `spec/*.qnt` にあり、各モデルの対象範囲・探索の設定・検証する性質・再現手順は [モデル](../../spec/README.md) が案内します。契約は `reference/` の各文書が定義し、lifecycleと包含・dependency・情報は [lifecycle](../reference/lifecycle.md)、候補と外部条件の評価は [候補と外部条件](../reference/candidates.md) が担います。意味を変える場合は該当モデルを更新し、型検査とinvariant/witness検査を行ってから、契約文書と実装へ反映します。モデルの対象外であるfilesystem、SQLite、実process、Git統合、記録者はRustで検査します。モデルの意味を変えない文書・テスト整理にモデルの再実行は必須にしません。

各モデルはそれぞれの検証範囲を持ち、変更に関係のある検査を選んで実行します。モデルの新設や検証範囲の拡張は一律に必須とせず、設計上の不確実性に応じて判断します。検査の再現条件とその結果は [モデル](../../spec/README.md) に置き、backend・sample数・seedを変えた検査はその実行条件も結果とともに記録します。`quint run` は bounded random simulation であり、反例が見つからなかったことは全状態についての証明ではありません。検査の成功はexit statusだけではなく、列挙した全invariantに反例がなく、列挙した全witnessが出力上1 trace以上で観測されたことを確認します。

`tests/lifecycle/workflow.rs` は両backendの独立fixtureで登録、候補選択、並行着手・Note、Group最終確認を一巡します。`tests/lifecycle/file.rs` は実Git worktreeで分岐し、自動統合と衝突、`axon merge prepare|check|apply`、stage後の通常操作まで検証します。SQLiteの共有worktreeでの並行着手もworkflow fixtureに含みます。テストはこのcheckoutのbinaryを絶対パスで実行し、Git環境を隔離します。実データやPATH上のbinaryを切り替えません。

### Declaration の独立fixture

[一括declaration](lifecycle-declaration.md) の形式と適用契約はRustで検証します。`crates/axon-core/src/declaration.rs` と `crates/axon-core/src/declaration/import.rs` の単体テストはstrict YAML、canonical往復、fingerprint、差分と共通コアの制約を扱います。`tests/lifecycle/declaration.rs` は `smoke` に含まれ、両backendの独立fixtureで`axon export`、雛形、`axon import prepare` → `axon import check` → `axon import apply` → 再度`axon import check`、新規登録と既存subtree編集、競合、保存先・入力の非変更を検査します。

```sh
cargo test --locked --workspace --lib declaration
cargo test --locked --test smoke declaration
```

`src/declaration_file.rs` の単体テストは、保存成功後のfile書戻し失敗と再度`axon import apply`、入力bytesの変化、保存結果の診断などI/O境界を検査します。 process fixtureは同じlib test binaryを子processにし、SQLite transaction内（UPDATE後commit前）、file rename前、両backendの保存後書戻し前・書戻し後で強制終了します。barrier待ちは最大10秒、到達後すぐにkillして終了を回収し、完全snapshot・入力bytesと同じfileの再度`axon import apply`への収束を検査します。`src/declaration_file/relationship_tests.rs` の行列は両backendで`Cancelled` Groupへの所属拒否、`Cancelled` Entityの依存差替え、新規Groupへの移動、親子反転、進行中subtreeの移動を検査します。recordの正順・逆順・巡回順で共通コアの適用結果を比較し、`axon import prepare`でcanonical化した各入力をbackendへ適用して結果の一致を確認します。これらは`--lib`としてfast gateにも含まれます。SQLite commit境界の失敗注入は `src/sqlite.rs` にあります。対象の検証後も、必要なfull verificationは上記の共通入口で行います。実データやPATH上のbinaryは変更しません。この機能は通常操作の意味を変えないため、検証のためだけにQuintの状態やactionを追加しません。

## 設計変更の進め方

モデルで検証する場合は、まず確かめたい性質と前提を整理し、該当するモデルを更新・検証する。検証後に確定した設計と理由を関連する docs に反映してから実装する。モデルを使わない設計変更では、判断と理由を docs に反映してから実装する。

実装中に設計の不足や矛盾が見つかった場合も、この手順に戻る。検証結果は対象モデルと実行条件を明記して残す。モデルの検査はモデル内の性質を調べるものであり、Rust 実装の適合性は実装のテストで確認する。
