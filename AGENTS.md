# axon

軸を分けたローカル issue tracker。このリポジトリは axon 自体の開発で、タスク管理にも axon を使う。

Axon 操作には、このリポジトリの協業方針として [`axon:conventions`](plugins/axon/skills/conventions/SKILL.md) を適用する。

## 作業時の参照先

仕様の正本は [spec/lifecycle_proposal.md](spec/lifecycle_proposal.md)。実装の入口は [共通コア](docs/development/lifecycle-core.md)、[SQLite CLI](docs/development/lifecycle-sqlite.md)、[file保存とGit統合](docs/development/lifecycle-file.md)。`src/lib.rs` と `src/main.rs` が現行実装です。

変更対象の契約は [docs/README.md](docs/README.md) から確認し、設計変更とモデル検証は [検証方針](docs/development/verification.md) に従ってください。`archive/three-axis` の旧コード・テストと、過去資料と明示されたreference/design・Quintモデルは現行仕様の規範にしません。旧Revision・claimの契約を新コアへ持ち込まないでください。

現在の件数やタスク状態など、確認元から取得できる現況を継続的な説明としてドキュメントに転記しない。調査・検証結果を残す場合は、時点と条件を明記する。

## ドッグフーディング

Axon の利用中に一般化できる不足や不整合を見つけたら、`axon:register` を使って未判断の課題として記録する。一時的な不慣れや単純な入力ミスは記録しない。元の作業を不必要に中断せず、記録した改善へ勝手に着手しない。
