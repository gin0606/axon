# axon

軸を分けたローカル issue tracker。このリポジトリは axon 自体の開発で、タスク管理にも axon を使う。

Axon 操作には、個人用協業方針として [`axon:conventions`](plugins/axon/skills/conventions/SKILL.md) を適用する。

## 作業時の参照先

変更対象に関係する設計判断と契約を確認する。全体の入口は [docs/README.md](docs/README.md)。設計変更とモデル検証は [検証方針](docs/development/verification.md) に従う。

- 状態の意味、関係、導出値: [状態モデル](docs/reference/state-model.md)
- 情報分類と操作範囲: [情報モデル](docs/reference/information-model.md)
- 永続化、状態更新、型境界: [アーキテクチャ](docs/development/architecture.md)
- CLI の仕様: [CLI 契約](docs/reference/cli.md)
- 宣言ファイルの形式と適用: [宣言ファイル](docs/reference/declaration-file.md)
- 設計理由と代替案: [設計判断](docs/design/decisions.md)

現在の件数やタスク状態など、確認元から取得できる現況を継続的な説明としてドキュメントに転記しない。調査・検証結果を残す場合は、時点と条件を明記する。

## ドッグフーディング

Axon の利用中に一般化できる不足や不整合を見つけたら、`axon:register` を使って未判断の課題として記録する。一時的な不慣れや単純な入力ミスは記録しない。元の作業を不必要に中断せず、記録した改善へ勝手に着手しない。
