# 検証方針とモデル検査

## Rust toolchain

通常の開発と検証には `rust-toolchain.toml` で固定した Rust を使う。mise を使う場合も Rust の idiomatic version file としてこの指定を読み込む。ソースからのビルドに必要な最低 Rust version（MSRV）は、root の `Cargo.toml` の `workspace.package.rust-version` を正本とし、各 crate はこれを継承する。

## Full verification

full verificationの正本は [scripts/full-verification](../../scripts/full-verification) にある。pull requestとmainへのpushでは、GitHub Actionsがrepositoryの `rust-toolchain.toml` を使ってこのscriptを実行する。ローカルでも `scripts/full-verification` で同じ検証を実行できる。

mainへのpushでは、Lefthookのpre-push（[.lefthook/pre-push/verify-main-push](../../.lefthook/pre-push/verify-main-push)）がpushするcommitを一時worktreeに取り出し、そのcommitのscriptと `rust-toolchain.toml` で検証して、失敗したらpushを拒否する。検証するsource codeに作業ツリーの未commitの変更は含まれない。hook自体、Lefthook・miseの設定は、pushを実行したcheckoutのものを使う。main以外へのpushでは実行しない。この検証はclient側のhookなので、`--no-verify` やhookを導入していないcloneからのpushでは実行されない。その場合もpush後のCIが失敗を検出する。

full verificationはデスクトップアプリ（`crates/axon-gui`）も対象にする。UIテストはGPUIのtest platform上のheadless windowで動き、ディスプレイやGPUを使わない。LinuxのCIはGPUIのビルドに要るsystem packageを導入してから実行する。GUIと同じbuildでは依存のfeatureが統合され、CLIとコアのserde_jsonにもGPUIが要求する `preserve_order` が入る。配布するCLIはこれを含まない構成でbuildするため、`default-members` だけを対象にしたtestも同じscriptで実行する。

CIのcacheはCargo dependencyとbuild artifactだけに使い、成功済みのtest結果を根拠にfull verificationを省略しない。

## Fast pre-commit gate

Rust fileがstagedされているcommitでは、Lefthookが [lefthook.yml](../../lefthook.yml) の高速な検査を実行する。この gate は全 test target を対象にしないため、full verification を代替しない。

リリース前には通常 toolchain の全検証に加え、`scripts/check-msrv` で MSRV での検査を実行する。GUI は依存の要求で workspace の MSRV より新しい版を自身の `rust-version` に宣言し、`scripts/check-msrv` はその版でも GUI を検査する。

### Plugin version

`plugins/` 配下の各 plugin と `examples/agent-workflow` は、内容が変わるたびに Claude と Codex の manifest の version を変える。Lefthook が各 commit で [scripts/update-plugin-versions.py](../../scripts/update-plugin-versions.py) を実行し、stage 済みの内容から両 manifest に同じ version を生成する。manifest を更新したときは commit を止めるので、表示された manifest を stage して再実行する。CI は `--check` による検査と `scripts/test-plugin-versions.py` を実行する。

## Rust coverage

`mise.toml` で固定した `cargo-llvm-cov` を使い、全target・全featureを次の一つのcommandで計測する。数値thresholdは設けず、未到達箇所を次の改善判断へ使う。

```sh
cargo llvm-cov --locked --all-targets --all-features --summary-only
```

全test targetを実行し、coverage用にtest自体をskipしない。意図的に途中終了させる子processだけは、merge不能なprofileを生成しないようcoverage出力を破棄する。親testは通常どおり実行し、終了code、lock解放、公開済みfileの完全性、失敗境界を検証する。通常の `cargo test` ではこのprofile制御は作用しない。

## モデルと運用検証

モデルで検証する状態・関係・候補・情報・統合の意味論は `spec/*.qnt` にあり、各モデルの対象範囲・探索の設定・検証する性質・再現手順は [モデル](../../spec/README.md) が案内します。契約は `reference/` の各文書が定義し、lifecycleと包含・dependency・情報は [lifecycle](../reference/lifecycle.md)、候補と外部条件の評価は [候補と外部条件](../reference/candidates.md)、記録の集合・衝突・違反・解決は [保存と統合](../reference/storage.md) が担います。意味を変える場合は該当モデルを更新し、型検査とinvariant/witness検査を行ってから、契約文書と実装へ反映します。統合の規則（衝突、違反と免除、解決、gap）を変えるときは `record_integration` の `run` テストと探索も再実行します。モデルの対象外であるfilesystem、保存先の探索と初期化、実process、Gitが記録fileをどう扱うか、記録者はRustで検査します。モデルの意味を変えない文書・テスト整理にモデルの再実行は必須にしません。

各モデルはそれぞれの検証範囲を持ち、変更に関係のある検査を選んで実行します。モデルの新設や検証範囲の拡張は一律に必須とせず、設計上の不確実性に応じて判断します。検査の実行条件とその結果は [モデル](../../spec/README.md) に置き、backend・sample数・stepsを変えた検査はその実行条件も結果とともに記録します。seedは固定せず、実行ごとに異なる経路を探索させます。同じseedを使い続けても、モデルが変わらない限り同じ経路をなぞるだけで新しい情報は得られません。`quint run` は bounded random simulation であり、反例が見つからなかったことは全状態についての証明ではありません。検査の成功はexit statusだけではなく、列挙した全invariantに反例がなく、列挙した全witnessがいずれかの探索で1 trace以上観測されたことを確認します。通常探索で観測率の低いwitnessは `lifecycle_reachability` の入口、`candidate_evaluation` の補助入口、`record_integration_paths` の入口が担保するため、それぞれ通常探索と合わせて1つの検査として扱います。反例が出た場合は、quintが出力する再現用のseedを結果に添えます。

Rust の結合テストは独立 fixture と実 Git worktree を使い、公開 CLI、保存・統合、探索と初期化を検証する。テスト対象はこの checkout の binary を絶対パスで指定し、Git 環境を隔離する。実管理データや PATH 上の binary は切り替えない。

### 候補と外部条件

[候補と外部条件](../reference/candidates.md) の候補集合と評価順・共有は、[候補選択](../../crates/axon-core/src/lifecycle/candidates.rs) の単体テストで boolean oracle と比較する。外部コマンドの起動・タイムアウト・中断・出力上限は、実プロセスを使う独立 fixture で検証する。

### Declaration と記録者情報

[一括 declaration](../reference/declaration.md) と記録者情報は通常操作の意味を変えないため、形式や I/O の検証のためだけに Quint の状態や action を追加しない。declaration は Rust で、コアでは strict YAML・canonical 往復・競合判定・入力順によらない適用結果を、保存 adapter では保存と書戻しの失敗の区別と、公開途中の process 喪失後に同じ file の再試行で収束することを、公開 CLI では拒否時に保存先と入力を保持することを検証する。

## 設計変更の進め方

モデルで検証する場合は、まず確かめたい性質と前提を整理し、該当するモデルを更新・検証する。検証後に確定した設計と理由を関連する docs に反映してから実装する。モデルを使わない設計変更では、判断と理由を docs に反映してから実装する。

実装中に設計の不足や矛盾が見つかった場合も、この手順に戻る。検証結果は対象モデルと実行条件を明記して残す。モデルの検査はモデル内の性質を調べるものであり、Rust 実装の適合性は実装のテストで確認する。
