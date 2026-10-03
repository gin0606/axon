# axon

IssueとGroupで個人の仕事や計画を管理するローカルCLIです。未判断、未着手、着手中、完了、取りやめのlifecycleを使い、包含・依存、再浮上条件、Noteと状態変更履歴を扱います。各Entityは仕事の種類を表す固定集合のlabelを必ず一つ持ちます（[値と意味](docs/guide/usage.md#labelで仕事の種類を示す)）。登録時に `axon capture --label` で付け、一覧の行で読み、`--label` で絞り込めます。

[使い始める](docs/guide/getting-started.md) で、このcheckoutでビルドしたbinaryを独立した保存先で試せます。利用中の保存先や実データは自動で切り替えません。[日常の操作](docs/guide/usage.md)、[文書一覧](docs/README.md)、[モデル](spec/README.md) も参照してください。

## インストール

Rust 1.89以上で `cargo build --locked --bin axon` を実行し、生成されたbinaryの絶対パスを指定します。詳しい試用手順は [導入ガイド](docs/guide/getting-started.md) にあります。

## Agent向けskill

[axon-kit](plugins/axon-kit/skills) は、Axonの情報モデル・操作契約と基本スキルを提供します。[axon](plugins/axon/skills) は、その上に構築した、Axonの開発者が想定する使い方をまとめた人とエージェントの協業ワークフローです。対応するCLIとpluginを導入すれば、他のrepositoryでも利用できます。skillの参照資料はplugin内に同梱しています。

`axon` はそのまま利用できるほか、スキルをコピーして変更したり、`axon` や `axon-kit` を組み合わせて個人・プロジェクト用のスキルを作成したりできます。`axon` の協業方針はAxon本体の仕様ではなく、利用者が変更・置き換えできるものです。

独自スキルでも `axon-kit` の操作契約を守ります。`axon` のスキルを呼び出す場合は、その協業方針に従います。方針を変えたい部分は独自スキルとして実装し、`axon-kit` を使います。

[ワークフローのサンプル](examples/agent-workflow/README.md) は、`axon`と`axon-kit`を使って計画の登録からIssue・Groupの実装・検証・commitまで進める`axon-workflow` pluginです。そのまま導入して試したり、自分の運用に合わせて変更したりできます。

`axon export ID...` で計画を canonical YAML として取得できます。新規計画の雛形は `axon docs declaration --example`、field と編集手順は `axon docs declaration` を参照してください。

## 開発

[層構造の地図](docs/development/architecture.md)、[共通コア](docs/development/lifecycle-core.md)、[Declaration](docs/development/lifecycle-declaration.md)、[CLIと保存の接続](docs/development/lifecycle-cli.md)、[file保存とGit統合](docs/development/lifecycle-file.md)、[候補と外部条件](docs/reference/candidates.md)、[記録者連携](docs/development/lifecycle-recorder.md)、[検証方針](docs/development/verification.md)、[設計判断](docs/design/decisions.md) を参照してください。
