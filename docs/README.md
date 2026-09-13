# ドキュメント

単一 lifecycle の再構築の正本は [literate spec](../spec/lifecycle_proposal.md)、新しい実装の入口は [共通コア](development/lifecycle-core.md) です。利用ガイドは新lifecycleに対応しています。旧三軸のreference/design・モデルは過去資料として区別します。継続する入出力契約は [現行CLI契約](reference/lifecycle-cli.md) に明示し、過去資料の一括廃止から契約の廃止を推論しません。新 binary と `tests/smoke.rs` は [SQLite CLI](development/lifecycle-sqlite.md) の新仕様を対象にします。`archive/three-axis/src` の旧 module と `archive/three-axis/tests` は過去の三軸実装・検証資料です。

新binaryの利用は [使い始める](guide/getting-started.md) から確認してください。

## 利用者向け

| 文書 | 読む目的 |
| --- | --- |
| [使い始める](guide/getting-started.md) | 新binary選択、独立SQLite試用、手動持込み、同梱skill |
| [日常の操作](guide/usage.md) | 候補、状態変更、Group最終確認、記録参照 |
| [状態と用語](guide/concepts.md) | 単一lifecycleと関係 |
| [保存先とworktree](guide/storage.md) | SQLite共有、fileのworktree隔離、探索・初期化の境界 |
| [Agentからのアクセス](guide/codex.md) | ホスト権限と保存先 |
| [記録者連携](development/lifecycle-recorder.md) | 自動取得と詳細参照 |
| [CLI入出力契約](reference/lifecycle-cli.md) | ID・引数・詳細参照・英語表示・装飾・保存結果 |

## 開発者向け

| 文書 | 定義する範囲 |
| --- | --- |
| [正本spec](../spec/lifecycle_proposal.md) | lifecycle・構造・候補・情報・表示・保存・統合の契約とモデル |
| [共通コア](development/lifecycle-core.md) | 通常操作、記録、codec、三者比較 |
| [SQLite CLI](development/lifecycle-sqlite.md) | 公開操作とSQLite adapter |
| [file保存とGit統合](development/lifecycle-file.md) | writer、worktree、merge CLI・driver |
| [候補と外部条件](development/lifecycle-candidates.md) | triage/tasksとprocess評価 |
| [記録者連携](development/lifecycle-recorder.md) | 自動取得と保存済み詳細 |
| [検証方針](development/verification.md) | CI、独立fixture、モデル検証の分担 |

保存schemaは [SQLite adapter](../src/sqlite.rs) と [file adapter](../src/file.rs)、Usageは [Clap定義](../src/main.rs) を確認します。

### 設計検討資料

[一括declarationの再設計資料](development/declaration-design-notes.md) は、一括編集を検討するときの目的・保存境界・協業方針をまとめた資料です。採用済みの公開契約や通常操作の手順には使いません。

## 過去資料

`reference/` のlifecycle-cli.md以外、`design/`、`development/architecture.md`・`branch-history.md` は置換前の三軸CLIの契約・設計です。`development/audits/` は各文書に記した対象・時点に限定した調査です。現行操作の手順には使いません。旧コード・テストは [archive/three-axis](../archive/three-axis/README.md) に隔離し、Cargo・CIの対象から外しています。旧Quintモデル `axon.qnt`・`group_plan.qnt`・`information_model.qnt`・`branch_history.qnt` も過去資料です。新モデルは正本specから生成します。

## 更新するとき

- 現在のルールは担当する契約文書で定義する。別の文書で説明するときは要約とリンクにする。
- 契約は現在形で書く。理解に必要な短い理由は契約の近くに残し、長い比較や過去の経緯は design に置く。
- 検証方法は development、検証結果には実施時点と対象・条件を記す。
- 未決事項の採否や作業状況は Axon で管理し、docs に現況一覧を複製しない。
- 文書を移動・分割したら、README、AGENTS.md、モデル冒頭などの参照元も更新する。
- 利用者向け文書は日本語で書き、CLIの識別子は実際の表記を併記する。段落内には手動改行を入れず、表示幅による折り返しに任せる。

候補の `triage/tasks` と外部コマンドの実行契約は [候補一覧と外部条件](development/lifecycle-candidates.md) を参照する。

[file保存とGit統合](development/lifecycle-file.md) は新lifecycleのwriter・merge CLI・Git driverの入口です。
