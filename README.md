# axon

ローカルで動く個人用 issue tracker。進行、採否、時期を別の軸として扱い、コーディングエージェントが安全に操作できる簡潔なテキスト CLI を提供する。

## 使う

```sh
cargo build --release
axon init
axon --help                     # 操作契約と全コマンドのリファレンス
axon -h                         # 短いコマンド一覧
```

個別コマンドの Usage は `axon help <command path>` で確認できる。操作の意味と出力契約の正は [docs/cli.md](docs/cli.md) であり、同じ内容が `axon --help` に埋め込まれている。

## ドキュメント

| ファイル | 内容 |
| --- | --- |
| [docs/cli.md](docs/cli.md) | CLI の操作契約と設計意図。`axon --help` の本文 |
| [docs/axes.md](docs/axes.md) | 状態モデルの軸。**なぜこの設計なのか**の記録。決着した論点が 24 件 |
| [docs/data-model.md](docs/data-model.md) | 永続化とスキーマ。SQLite 単体、git 管理外 |
| [docs/dry-run.md](docs/dry-run.md) | 運用シナリオを通した検証 |
| [docs/implementation.md](docs/implementation.md) | 実装方針と最小スコープ |
| [spec/axon.qnt](spec/axon.qnt) | Quint による形式仕様。状態機械として書き、17 個の性質を検査している |

設計の議論では、Quint によるモデル検査で考慮漏れが 2 件見つかっている (`orphaned` が推移しない問題、`blockedReason` がグループ依存を辿らない問題)。どちらも議論だけでは見落としていた。

## 名前

`axon` は軸索。`axis` (軸) と同語源で、**軸を分解したことが設計の核心**であることによる。軸索が信号を一方向に伝えるのは、依存グラフの伝播とも重なる。

## 適用範囲

**個人のタスク分解・管理に絞る。** プロダクト全体の ITS としては使わない。

両者は要件が違う (共有の要否、PR からの参照、保存形式の制約) ため、混ぜると設計が引きずられる。プロダクト用途が必要なら別のツールを使う。
