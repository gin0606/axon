# axon

Issue と Group で個人の仕事や計画を管理するローカル issue tracker。このリポジトリは axon 自体の開発で、タスク管理にも axon を使う。

Axon 操作には、このリポジトリの協業方針として [`axon:conventions`](plugins/axon/skills/conventions/SKILL.md) を適用する。

## 作業時の参照先

振る舞いの契約は [docs/reference](docs/reference/lifecycle.md) の各文書、状態と遷移の Quint モデルは [spec](spec/README.md) にある。層の境界と依存方向の規則は [層の依存方向](docs/development/architecture.md) にある。

変更対象の契約は [docs/README.md](docs/README.md) から確認し、設計変更とモデル検証は [検証方針](docs/development/verification.md) に従ってください。契約がその形になっている理由は [設計判断](docs/design/decisions.md) にあります。

現在の件数やタスク状態など、確認元から取得できる現況を継続的な説明としてドキュメントに転記しない。調査・検証結果を残す場合は、時点と条件を明記する。

## ドッグフーディング

Axon の利用中に一般化できる不足や不整合を見つけたら、`axon:register` を使って未判断の課題として記録する。一時的な不慣れや単純な入力ミスは記録しない。元の作業を不必要に中断せず、記録した改善へ勝手に着手しない。
