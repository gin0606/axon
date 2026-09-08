---
name: declaration
description: 任意件数の Entity に対する厳密な YAML plan declaration を review、export、canonicalize、check、apply する。declaration artifact と axon export/import に使い、通常の単一 Entity 変更や CLI 自体の実装には使わない。
---

# Axon declaration file を操作する

`axon-kit:conventions` を使う。そのモデル contract を読み、storage または artifact の mutation 前には mutation contract も読む。

この capability は declaration artifact と `axon export` / `axon import prepare|check|apply` の mechanics を所有する。declaration が編集できるのは title、description、parent、outgoing dependency だけである。Progress、Disposition、Resurface condition、claim、Declaration Revision、Note、typed history、incoming relation、external Entity value は編集できない。owned relation の変更に伴って必要な external snapshot を追加・削除できるが、edit set は拡張されない。

呼び出し側の依頼または workflow が operation mode と意図する declaration 内容を与える。artifact の編集は storage への適用を許可せず、storage への適用は Control state の変更を許可しない。

file への export、canonicalize、apply、conflict または不確かな結果からの復旧を行う前に、[declaration workflow](references/workflow.md)を最後まで読む。read-only の `axon import check <file>` には、この entrypoint の境界だけが必要である。Export、check、apply は、観測 snapshot または導出影響に必要な Command condition を評価でき、skip option はない。最初に保存済み Command 文字列を調査し、適切な実行 environment を使う。

固定済み declaration をまず `Undecided` に戻す必要がある場合は `axon-kit:triage` を使う。与えられた内容と、declaration format では表現できない初期 condition を持つ単一 Entity の作成には `axon-kit:plan` または `axon-kit:capture` を使う。これらの capability は artifact mechanics から分離したままとする。

operation mode、storage 結果の分類、変更した ID と kind、key-to-ID mapping、関係する構造的・導出的影響、warning、変更した storage artifact、artifact status、保持すべき recovery file を返す。
