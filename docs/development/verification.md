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

`--lib` は共通コア・記録の集合の導出と衝突・codec のテストを実行する。`--bin axon` は端末表示と条件プロセスの起動・trace flush失敗、`--test smoke` は binary の登録から Group 完了、Note・log、並行操作、未対応 format と破損の拒否、保存先探索・`axon init`、入出力失敗を独立 fixture で検証する。

`cargo test` はworkspaceの記録者crate単体テストも実行する。保存・統合は `tests/lifecycle/file.rs`、保存先の初期化と探索は `tests/lifecycle/location.rs` をsmokeから実行し、実worktreeでのGit操作、index、並行writer、破損の報告を検証する。

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

モデルで検証する状態・関係・候補・情報・統合の意味論は `spec/*.qnt` にあり、各モデルの対象範囲・探索の設定・検証する性質・再現手順は [モデル](../../spec/README.md) が案内します。契約は `reference/` の各文書が定義し、lifecycleと包含・dependency・情報は [lifecycle](../reference/lifecycle.md)、候補と外部条件の評価は [候補と外部条件](../reference/candidates.md)、記録の集合・衝突・違反・解決は [保存と統合](../reference/storage.md) が担います。意味を変える場合は該当モデルを更新し、型検査とinvariant/witness検査を行ってから、契約文書と実装へ反映します。統合の規則（衝突、違反と免除、解決、gap）を変えるときは `record_integration` の `run` テストと探索も再実行します。モデルの対象外であるfilesystem、保存先の探索と初期化、実process、Gitが記録fileをどう扱うか、記録者はRustで検査します。モデルの意味を変えない文書・テスト整理にモデルの再実行は必須にしません。

各モデルはそれぞれの検証範囲を持ち、変更に関係のある検査を選んで実行します。モデルの新設や検証範囲の拡張は一律に必須とせず、設計上の不確実性に応じて判断します。検査の実行条件とその結果は [モデル](../../spec/README.md) に置き、backend・sample数・stepsを変えた検査はその実行条件も結果とともに記録します。seedは固定せず、実行ごとに異なる経路を探索させます。同じseedを使い続けても、モデルが変わらない限り同じ経路をなぞるだけで新しい情報は得られません。`quint run` は bounded random simulation であり、反例が見つからなかったことは全状態についての証明ではありません。検査の成功はexit statusだけではなく、列挙した全invariantに反例がなく、列挙した全witnessがいずれかの探索で1 trace以上観測されたことを確認します。通常探索で観測率の低いwitnessは `lifecycle_reachability` の入口、`candidate_evaluation` の補助入口、`record_integration_paths` の入口が担保するため、それぞれ通常探索と合わせて1つの検査として扱います。反例が出た場合は、quintが出力する再現用のseedを結果に添えます。

`tests/lifecycle/workflow.rs` は独立fixtureで登録、候補選択、並行着手・Note、Group最終確認を一巡します。`tests/lifecycle/file.rs` は実Git worktreeで分岐し、merge・rebase・cherry-pick・revert・squashの後の `axon storage check` の報告（衝突・違反・gap、無ければ報告なし）、`axon resolve` と通常操作（終了したGroupへ流入した子の違反の `axon reopen` など）による修復、通常操作への復帰まで検証します。`core.autocrlf=true` のcloneで `.axon/.gitattributes` が記録fileを改行変換から守ること、この属性のない保存先が破損として止まり改行変換の可能性とガイドへの案内を示すこと、ガイドの手順でLFに戻ることも同じfileで検証します。`tests/lifecycle/location.rs` は `axon init` の出力と作るfile、repository rootの `.gitignore`・`.gitattributes`・Git configを作成も編集もしないこと、表示された無視する運用の手順に従うと保存先がGitに無視されること、`git add .axon` で記録・header・`.axon/.gitignore`・`.axon/.gitattributes` だけが追跡されlockと一時fileが追跡されないこと、初期化直後のuntrackedな保存先がGitのcheckout・mergeの上書きから保護され、無視した後は警告なしに置き換わること、実linked worktreeからの保存先の共有、worktreeをまたぐ並行`axon start`と確定した保存先の隣に置くlock、linked worktreeでの`axon init`の拒否、bare repositoryに付けたworktreeとsubmoduleで探索が2段目へ落ちないこと、探索の確定と停止、`axon storage check ROOT` が探索と同じくGit indexのunmergedと探索が拒否するGitの境界を報告すること、headerが作業treeにない保存先でも `axon storage check` と通常操作がheaderの欠落を併記せずにunmergedを報告し、そのindexを持つworktree（linked worktreeからmain worktreeの保存先を使う場合はmain worktree）と各pathをworktreeの先頭からの相対pathで一行ずつ示すこと、symlinkの拒否を検証します。いずれも `tests/smoke.rs` から読み込みます。テストはこのcheckoutのbinaryを絶対パスで実行し、Git環境を隔離します。実データやPATH上のbinaryを切り替えません。

### Declaration の独立fixture

[一括declaration](lifecycle-declaration.md) の形式と適用契約はRustで検証します。`crates/axon-core/src/declaration.rs` と `crates/axon-core/src/declaration/import.rs` の単体テストはstrict YAML、canonical往復、fingerprint、差分と共通コアの制約を扱います。`tests/lifecycle/declaration.rs` は `smoke` に含まれ、独立fixtureで`axon export`、雛形、`axon import prepare` → `axon import check` → `axon import apply` → 再度`axon import check`、新規登録と既存subtree編集、競合、保存先・入力の非変更を検査します。

```sh
cargo test --locked --workspace --lib declaration
cargo test --locked --test smoke declaration
```

`src/declaration_file.rs` の単体テストは、保存成功後のfile書戻し失敗と再度`axon import apply`、入力bytesの変化、保存結果の診断などI/O境界を検査します。 process fixtureは同じlib test binaryを子processにし、記録fileのrename前、複数のEntityの記録のrenameの途中、保存後の書戻し前・書戻し後で強制終了します。barrier待ちは最大10秒、到達後すぐにkillして終了を回収し、記録の集合・入力bytesと同じfileの再度`axon import apply`への収束を検査します。`src/declaration_file/relationship_tests.rs` の行列は`Cancelled` Groupへの所属拒否、`Cancelled` Entityの依存差替え、新規Groupへの移動、親子反転、進行中subtreeの移動を検査します。recordの正順・逆順・巡回順で共通コアの適用結果を比較し、`axon import prepare`でcanonical化した各入力を適用して結果の一致を確認します。これらは`--lib`としてfast gateにも含まれます。対象の検証後も、必要なfull verificationは上記の共通入口で行います。実データやPATH上のbinaryは変更しません。この機能は通常操作の意味を変えないため、検証のためだけにQuintの状態やactionを追加しません。

## 設計変更の進め方

モデルで検証する場合は、まず確かめたい性質と前提を整理し、該当するモデルを更新・検証する。検証後に確定した設計と理由を関連する docs に反映してから実装する。モデルを使わない設計変更では、判断と理由を docs に反映してから実装する。

実装中に設計の不足や矛盾が見つかった場合も、この手順に戻る。検証結果は対象モデルと実行条件を明記して残す。モデルの検査はモデル内の性質を調べるものであり、Rust 実装の適合性は実装のテストで確認する。
