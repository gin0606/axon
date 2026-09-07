# ドキュメントの入口

axon の利用手順、現在の契約、開発方法、設計経緯を目的別に分ける。
基本操作から知りたい場合は利用ガイド、実装を変更する場合は関係する契約と検証方針から読む。

## 使う

| 文書 | 読む目的 |
| --- | --- |
| [利用ガイド](guide/usage.md) (英語) | 初期化、Issue / Group の進行、判断、Note、宣言ファイルの操作 |
| [Codex の設定](guide/codex.md) | linked worktree から共有 DB を使うための sandbox 設定 |

コマンド構文と option は `axon help <command path>` / `axon <command path> --help`、
端末で読む状態モデルと基本 workflow は `axon docs` を使う。

## 仕様を確かめる

| 文書 | 定義する範囲 |
| --- | --- |
| [状態モデル](reference/state-model.md) | Entity、各軸、claim、包含、依存、Group の進行、導出値 |
| [情報モデル](reference/information-model.md) | 情報分類、declaration の所有と固定、Revision、Note、履歴、観測 |
| [手動移行](reference/migration.md) | 通常操作のschema更新と現行SQLiteのbackend変換・検証・切替 |
| [Backend と file 保存](reference/file-storage.md) | 設定なしの探索、init、Git integration、保存と merge の失敗境界 |
| [CLI 契約](reference/cli.md) | コマンド境界、反復実行、原子性、入出力、管理 root と ID の解決 |
| [宣言ファイル](reference/declaration-file.md) | strict YAML、所有範囲、競合検査、export / prepare / check / apply |

状態モデルと情報モデルが意味を定義し、CLI と宣言ファイルはその意味を操作へ対応させる。
利用ガイドの要約や設計経緯を、追加の規範として扱わない。

## 開発する

| 文書 | 読む目的 |
| --- | --- |
| [アーキテクチャ](development/architecture.md) | 型境界、SQLite、管理 root、状態更新と履歴、actor |
| [分岐履歴](development/branch-history.md) | 因果参照、current/last Revision、codecと移行の境界 |
| [検証方針](development/verification.md) | 設計変更時の手順、モデルの担当範囲、検査コマンド、過去の検査条件と結果 |

形式モデルは [core](../spec/axon.qnt)、[Group](../spec/group_plan.qnt)、
[情報モデル](../spec/information_model.qnt)。対象範囲と更新条件は検証方針に従う。
DB schema の正は [src/db.rs](../src/db.rs)、CLI の Usage は Clap の定義に置く。

## 設計理由を調べる

| 文書 | 読む目的 |
| --- | --- |
| [設計判断](design/decisions.md) | 軸を分けた理由、代替案、Group・情報モデル・永続化の導入経緯 |
| [file backend の設計](design/file-backend.md) | worktree ごとの保存、履歴の分岐、三方向 merge、手動移行の新仕様案 |
| [保存先と初期化の簡素化](design/storage-discovery.md) | config 廃止、元の SQLite 配置、一 repository 一 backend と保証の範囲 |
| [CLI 操作案内の棚卸し](design/cli-guidance-audit.md) | 2026-09-05 時点の全コマンドの案内評価 |
| [初期設計のドライラン](design/initial-dry-run.md) | 初期モデルを運用シナリオに当てはめた検討記録 |

## 更新するとき

- 現在のルールは担当する契約文書で定義する。別の文書で説明するときは要約とリンクにする。
- 契約は現在形で書く。理解に必要な短い理由は契約の近くに残し、長い比較や過去の経緯は design に置く。
- 検証方法は development、検証結果には実施時点と対象・条件を記す。
- 未決事項の採否や作業状況は Axon で管理し、docs に現況一覧を複製しない。
- 文書を移動・分割したら、README、AGENTS.md、モデル冒頭などの参照元も更新する。
