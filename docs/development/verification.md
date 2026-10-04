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

対象ごとの責務と fixture の入口は [各層の検証入口](architecture.md#各層の検証入口) を参照する。この gate は全 test target を対象にしないため、full verification を代替しない。Lefthook は各 job の失敗時に commit を拒否し、staged Rust file がなければ Rust 検証を省略する。設定は [lefthook.yml](../../lefthook.yml) にある。

リリース前には通常 toolchain の全検証に加え、MSRVで次を実行する。

```sh
cargo +1.89.0 check --locked --all-targets --all-features
```

### Plugin version

Python 3 を使い、Lefthook は各 commit で `python3 scripts/update-plugin-versions.py` を実行する。対象は `plugins/` 配下の各 plugin と `examples/agent-workflow`。stage 済みのファイルの内容・パス・Git mode からハッシュを生成し、Claude と Codex の manifest に同じ `<base>+plugin.<hash>` を設定する。base は manifest の `version` の `+` より前の値を使い、変更するときは両方を揃えて stage する。ハッシュ計算からは両 manifest の `version` を除外する。

生成が必要な場合は manifest を更新して commit を止める。表示された manifest を stage して再実行する。index は変更せず、manifest に未 stage の編集があれば上書きせずに止まる。plugin 外の変更や未追跡ファイルは version に影響しない。

CI は書き換えなしの検査と独立した Git fixture による生成処理の検証を行う。ローカルでも次のコマンドで確認できる。`--check` は stage 済みの内容を検査する。

```sh
python3 scripts/update-plugin-versions.py --check
python3 scripts/test-plugin-versions.py
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

Rust の結合テストは独立 fixture と実 Git worktree を使い、公開 CLI、保存・統合、探索と初期化を検証する。テスト対象はこの checkout の binary を絶対パスで指定し、Git 環境を隔離する。実管理データや PATH 上の binary は切り替えない。対象別の入口は [各層の検証入口](architecture.md#各層の検証入口) を参照する。

### Declaration の独立fixture

[一括 declaration](../reference/declaration.md) は通常操作の意味を変えないため、形式や I/O の検証のためだけに Quint の状態や action を追加しない。Rust では次の境界を検証する。

| 対象 | 検証する性質 | 入口 |
| --- | --- | --- |
| コア | strict YAML、canonical 往復、競合判定、通常操作の制約、入力順によらない適用結果 | [declaration](../../crates/axon-core/src/declaration.rs) と [適用処理](../../crates/axon-core/src/declaration/import.rs) |
| 保存 adapter | 保存と書戻しの失敗を区別し、公開途中の process 喪失後も同じ file の再試行で収束すること | [declaration_file](../../src/declaration_file.rs) と [関係変更のテスト](../../src/declaration_file/relationship_tests.rs) |
| 公開 CLI | 取得から一括編集・再試行までの接続、拒否時の保存先と入力の保持 | [独立 fixture](../../tests/lifecycle/declaration.rs) |

```sh
cargo test --locked --workspace --lib declaration
cargo test --locked --test smoke declaration
```

対象の検証後も、必要な full verification は共通入口で行う。

## 設計変更の進め方

モデルで検証する場合は、まず確かめたい性質と前提を整理し、該当するモデルを更新・検証する。検証後に確定した設計と理由を関連する docs に反映してから実装する。モデルを使わない設計変更では、判断と理由を docs に反映してから実装する。

実装中に設計の不足や矛盾が見つかった場合も、この手順に戻る。検証結果は対象モデルと実行条件を明記して残す。モデルの検査はモデル内の性質を調べるものであり、Rust 実装の適合性は実装のテストで確認する。
