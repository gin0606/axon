# axon

IssueとGroupで個人の仕事や計画を管理するローカルCLIです。未判断、未着手、着手中、完了、取りやめの単一lifecycleを使い、包含・依存、再浮上条件、Noteと状態変更履歴を扱います。

[使い始める](docs/guide/getting-started.md) で、このcheckoutの新binaryを独立したSQLiteまたはfile保存先で試せます。既存binaryや実データは自動で切り替えません。[日常の操作](docs/guide/usage.md)、[文書一覧](docs/README.md)、[正本spec](spec/lifecycle_proposal.md) も参照してください。

## インストール

Rust 1.89以上で `cargo build --locked --bin axon` を実行し、生成されたbinaryの絶対パスを指定します。詳しい試用手順と手動データ持込みの境界は [導入ガイド](docs/guide/getting-started.md) にあります。

## Agent向けskill

[axon-kit](plugins/axon-kit/skills) は操作契約、[axon](plugins/axon/skills) は任意の個人用協業方針です。このcheckoutのskillとbinaryを組み合わせて利用してください。

## 開発

[共通コア](docs/development/lifecycle-core.md)、[SQLite CLI](docs/development/lifecycle-sqlite.md)、[候補と外部条件](docs/development/lifecycle-candidates.md)、[記録者連携](docs/development/lifecycle-recorder.md)、[検証方針](docs/development/verification.md) を参照してください。[file保存とGit統合](docs/development/lifecycle-file.md) も利用できます。置換前のコード・テストは [過去資料](archive/three-axis/README.md) に隔離しています。
