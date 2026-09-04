# axon

ローカルで動く個人用 issue tracker。issue と明示的な計画 group を共通 Entity として扱い、進行、採否、時期を別の軸に保つ。

## 使う

```sh
cargo build --release
axon init
axon                            # 分類したコマンド概要
axon help                       # axon、-h、--help と同じ概要
axon docs                       # 状態モデルと基本 workflow
axon completion zsh > _axon    # シェル補完スクリプトを生成
```

個別コマンドの Usage は `axon help <command path>` または `axon <command path> --help` で確認できる。`axon docs` は状態モデルと基本 workflow を端末向けに説明する。より詳しい英語の利用マニュアルは [docs/help.md](docs/help.md)、操作の意味と設計理由を記録する日本語の開発者向け文書は [docs/cli.md](docs/cli.md) に分けている。

`completion` は `bash`、`elvish`、`fish`、`powershell`、`zsh` を受け付ける。生成したスクリプトは各シェルの補完ディレクトリに置くか、そのシェルの方法で読み込む。

Codex の linked worktree から共有 DB を更新する場合は、[Codex の sandbox 設定](docs/codex.md)を一度だけ行う。

## Agent skills

[`plugins/axon-kit`](plugins/axon-kit) は、axon の状態・情報モデルと安全な tracker 操作を提供する公式の Axon Skill Kit である。`$axon-kit:conventions`、`capture`、`plan`、`triage`、`work-state`、`add-note`、`declaration` を、利用者固有の実装フローと分けて提供する。

[`plugins/axon`](plugins/axon) は、公式 kit に個人用の判断と協業方針を重ねる。`conventions`、`register`、`triage`、`work-state`、`declaration` で、自律実行と重要な意思決定の境界、重複確認、構造整理、artifact 保護、申し送りを扱う。

2 plugin は [repo-local marketplace](.agents/plugins/marketplace.json) から開発できる。ローカルの Codex 設定へ追加するときは repository root で次を実行する。

```sh
codex plugin marketplace add .
codex plugin add axon-kit@axon
codex plugin add axon@axon
```

ローカルの Claude Code 設定へ追加するときは repository root で次を実行する。

```sh
claude plugin marketplace add ./
claude plugin install axon-kit@axon
claude plugin install axon@axon
```

`axon` plugin は `axon-kit` plugin を前提とし、必要な kit skill を明示的に併用する。継承や同名 skill の上書きではない。

## reason と履歴

`-r` / `--reason` はすべて任意。状態から意図を復元できない操作だけが受け取る。`release` の理由は `show` の進行履歴に、`decide` / `when` の理由は `log` の判断履歴に保存される。`start` と `done` は reason を受け取らない。作業結果や申し送りは `axon note add <id> -m <body>` で追記し、Entity が何であるかを定める description とは分ける。

title、description、parent、outgoing dependency は Entity の plan declaration である。Accepted / Rejected では固定され、変更には Undecided への戻しと再判断が必要になる。判断対象になった全文は Declaration Revision として残り、`axon revision list|show|diff` で確認できる。Note は状態を問わず追記でき、`axon show` では description と全 Note を省略せず表示する。

## ドキュメント

| ファイル | 内容 |
| --- | --- |
| [docs/cli.md](docs/cli.md) | CLI の操作意味と設計意図を記録する日本語の開発者向け文書 |
| [docs/help.md](docs/help.md) | 状態モデルと基本的な操作を説明する英語の利用マニュアル |
| [docs/axes.md](docs/axes.md) | 状態モデルの軸。**なぜこの設計なのか**の記録。決着した論点が 24 件 |
| [docs/declaration-file.md](docs/declaration-file.md) | export / import が扱う strict YAML の形式契約 |
| [docs/information-model.md](docs/information-model.md) | plan declaration、Revision、Note、状態、履歴の規範契約 |
| [docs/data-model.md](docs/data-model.md) | 永続化とスキーマ。SQLite 単体、git 管理外 |
| [docs/codex.md](docs/codex.md) | Codex の linked worktree から共有 DB を更新するための sandbox 設定 |
| [docs/dry-run.md](docs/dry-run.md) | 運用シナリオを通した検証 |
| [docs/implementation.md](docs/implementation.md) | 実装方針と最小スコープ |
| [spec/axon.qnt](spec/axon.qnt) | Quint による形式仕様。A / B / C / D の思想的コアだけを状態機械として検査する |
| [spec/group_plan.qnt](spec/group_plan.qnt) | 明示的な計画 group の拡張仕様。Entity、包含、状態遷移、導出値をコアと分けて検査する |
| [spec/information_model.qnt](spec/information_model.qnt) | 情報分類、declaration 固定、Revision、Note の変更範囲を検査する |

設計の議論では、Quint のシミュレーションで考慮漏れが 3 件見つかっている (`orphaned` が推移しない問題、`blocking cause` がグループ依存を辿らない問題、Ended group 自身を親変更できる問題)。いずれも議論だけでは見落としていた。

## 名前

`axon` は軸索。`axis` (軸) と同語源で、**軸を分解したことが設計の核心**であることによる。軸索が信号を一方向に伝えるのは、依存グラフの伝播とも重なる。

## 適用範囲

**個人のタスク分解・管理に絞る。** プロダクト全体の ITS としては使わない。

両者は要件が違う (共有の要否、PR からの参照、保存形式の制約) ため、混ぜると設計が引きずられる。プロダクト用途が必要なら別のツールを使う。
