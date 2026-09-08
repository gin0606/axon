---
name: plan
description: 完全な計画 declaration を持つ新規 Issue または Group を Accepted として登録する。採用判断と declaration 内容がすでに与えられている場合に使い、未解決事項の記録、既存 Entity の判断、実装には使わない。
---

# 採用済みの Axon Entity を登録する

`axon-kit:conventions` を使う。Entity を作成する前に、そのモデル、作成、mutation contract を読む。

## 入力 contract

呼び出し側が、採用判断、意図する kind、完全な declaration、Resurface condition、および記録したい transition reason があればそれを与える。この capability は意味を変えず表現を改善してよい。呼び出し側の採用判断を独自の判断に置き換えること、別 Entity を再利用するかの選択、計画の拡張、実装の開始は行わない。

与えられた declaration が不完全または曖昧な場合は、書き込まず不足している入力を返す。

## 完全な declaration を組み立てる

1 つの採用済み作業項目には Issue、複数の Entity を含む明示的な計画境界には Group を使う。declaration は、後の実装者が作成時の会話なしに、作業が存在する理由と完了条件を復元できる内容にする。

呼び出し側が与えた declaration 内容と関係だけを使う。将来の調査・実装結果は初期 description ではなく Note に置く。

mutation の前に、会話 context なしで提案する declaration を読み直す。この capability が重要な要件または構造上の判断を補う必要がある場合は、書き込まず不足している入力を返す。

## 作成して検証する

Issue には `axon plan <title>`、Group には `axon group plan <title>` を実行する。`--parent <group-id>` を含め、outgoing dependency ごとに `--needs <entity-id>` を繰り返し、初期 description を `-m <description>` または `-F <snapshot>` で与える。file または stdin を使う前に、mutation contract の入力固定規則を適用する。

与えられた初期 condition を使う。`Always` では condition option を省略し、それ以外では `--manual`、`--at <YYYY-MM-DD>`、`--after <entity-id>`、`--command <shell-string>` のいずれか 1 つだけを選ぶ。ID には完全な ID または一意な suffix を指定できる。これらの入力、Entity、最初の完全な Revision は atomic に保存される。与えられた dependency や condition を設定するためだけに capture を経由しない。初期値は transition history を作らず、作成には reason option もない。reason を付けるために transition を捏造しない。明示的に要求された実際の transition は別操作のままとする。

初期 Command 文字列は実行せず保存される。確認で外部 command を実行しないよう、`axon show <id> --skip-command-evaluation` で検証する。該当する場合、readiness は未評価として報告し、呼び出し側がその観測を必要とするときだけ別途評価する。

明確な作成失敗では部分的な Entity は残らない。不確かな結果は、再試行の前に作成 contract に従って照合する。

採用後、Entity context と新しく記録された Declaration Revision を読む。固定された declaration、`Progress=NotStarted`、`Disposition=Accepted`、意図した Resurface condition、parent、dependency、claim がないこと、decision/progress transition が 0 件であることを検証する。ID、kind、declaration の要約、readiness への影響、該当する場合は変更された storage artifact、storage 結果の分類を返す。Entity を開始しない。
