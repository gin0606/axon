---
name: capture
description: 未解決の懸念や計画境界を、採用せず Undecided の新規 Issue または Group として作成する。Accepted での登録、既存 Entity の変更、実装には使わない。
---

# 未判断の Axon Entity を記録する

`axon-kit:conventions` を使う。Entity を作成する前に、そのモデル、作成、mutation contract を読む。

## 入力 contract

呼び出し側が、未解決の懸念または計画境界と、意図する kind を与える。該当する場合は、意図する parent、outgoing dependency、初期 Resurface condition も与える。この capability は記述を後から読んでも通じる表現へ整えてよいが、作業の採用や開始、schedule、別 Entity を再利用するかの選択、構造的な関係の創作は行わない。与えられた初期 condition は保存できるが、後からの Resurface condition 変更は `axon-kit:triage` に渡す。

1 つの懸念または作業候補には Issue を使う。複数の Entity を含みうる明示的な計画境界にだけ Group を使う。意図する kind または関係が未解決で declaration に影響する場合は、書き込まず `input required` を返す。

## draft declaration を組み立てる

判断や要件を加えず、呼び出し側から与えられた declaration 内容を使う。将来の finding や handoff のために description を使わない。

mutation の前に、後から読む人が現在の会話を参照できないものとして、提案する declaration を読み直す。説明のないローカルな略語や曖昧な参照を解消する。

## 作成して検証する

Issue には `axon capture <title>`、Group には `axon group capture <title>` を実行する。決定済みの parent は `--parent <group-id>`、outgoing dependency はそれぞれ `--needs <entity-id>`、初期 description は `-m <description>` または `-F <snapshot>` で与える。file または stdin を使う前に、mutation contract の入力固定規則を適用する。

初期 condition が与えられた場合、`--manual`、`--at <RFC3339>`、`--after <entity-id>`、`--command <shell-string>` のいずれか 1 つだけを選ぶ。省略時は `Always` となる。ID には完全な ID または一意な suffix を指定できる。完全な draft と初期 condition は、Revision や架空の状態 transition を作らず atomic に保存される。呼び出し側が与えていない schedule や関係を創作しない。

初期 Command 文字列は作成時に実行されない。これを実行せず保存結果を検証するには `axon show <id> --skip-command-evaluation` を使う。

新しく作成した Entity について完全な最終 context を読み、次を検証する。

- 意図した kind、title、description、parent、outgoing dependency
- `Progress=NotStarted`、`Disposition=Undecided`、与えられた Resurface condition（default は `Always`）
- Declaration Revision が 0 件
- claim、Note、decision/progress transition history がないこと

明確な作成失敗では部分的な Entity は残らない。不確かな作成結果は、再試行の前に作成 contract に従って照合する。frontier に Entity がないという理由だけで代替 Entity を作らない。

作成した ID、kind、完全な declaration の要約、該当する場合は変更された storage artifact、storage 結果の分類を返す。採用や作業へ続けない。
