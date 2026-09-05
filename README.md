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

個別コマンドの Usage は `axon help <command path>` または `axon <command path> --help` で確認できる。`axon docs` は状態モデルと基本 workflow を端末向けに説明する。より詳しい英語の利用マニュアルは [利用ガイド](docs/guide/usage.md)、コマンドの振る舞いを定める日本語の開発者向け文書は [CLI 契約](docs/reference/cli.md) に分けている。

`completion` は `bash`、`elvish`、`fish`、`powershell`、`zsh` を受け付ける。生成したスクリプトは各シェルの補完ディレクトリに置くか、そのシェルの方法で読み込む。

Codex の linked worktree から共有 DB を更新する場合は、[Codex の sandbox 設定](docs/guide/codex.md)を一度だけ行う。

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

[ドキュメントの入口](docs/README.md) から、目的に合う文書を選べる。

- 使う: [利用ガイド](docs/guide/usage.md)
- 仕様を確かめる: [状態モデル](docs/reference/state-model.md)、[情報モデル](docs/reference/information-model.md)、[CLI](docs/reference/cli.md)、[宣言ファイル](docs/reference/declaration-file.md)
- 開発する: [アーキテクチャ](docs/development/architecture.md)、[検証方針](docs/development/verification.md)
- 設計理由を調べる: [設計判断](docs/design/decisions.md)

## 名前

`axon` は軸索。`axis` (軸) と同語源で、**軸を分解したことが設計の核心**であることによる。軸索が信号を一方向に伝えるのは、依存グラフの伝播とも重なる。

## 適用範囲

**個人のタスク分解・管理に絞る。** プロダクト全体の ITS としては使わない。

両者は要件が違う (共有の要否、PR からの参照、保存形式の制約) ため、混ぜると設計が引きずられる。プロダクト用途が必要なら別のツールを使う。
