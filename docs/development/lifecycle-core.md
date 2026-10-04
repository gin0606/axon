# 共通コア

[axon-core](../../crates/axon-core/src/lib.rs) は、保存先や CLI から独立して記録の集合を検査し、現在値の導出と変更する記録の生成を担う。filesystem、Git、外部コマンドは呼ばない。crate 間の依存規則は [層構造](architecture.md#依存方向の規則) に定める。

## 通常操作と構造

操作の前提と状態遷移は [lifecycle](../reference/lifecycle.md)、衝突中の操作と違反の修復は [保存と統合](../reference/storage.md#衝突と通常操作)、同値指定と reason の扱いは [CLI 契約](../reference/cli.md#mutationの結果) を正本とする。

[通常操作の実装](../../crates/axon-core/src/lifecycle/record/ops.rs) は、個々の操作の前提と、操作後の構造の検査を担う。読取側の `check_operation` は操作後の全体検査を代替しない。読取側で許可されても、変更によって違反が増える場合などは書込側が拒否する。adapter は読取時の判定だけで記録を作らず、共通コアの通常操作を使う。

Group の `Complete` の呼出し自体を、Group 全体の成果の最終確認済みという明示入力として扱う。子の終了や前提の検査だけでは、この確認を記録しない。

## 記録の集合と導出

[記録の集合](../../crates/axon-core/src/lifecycle/record/store.rs) と [view](../../crates/axon-core/src/lifecycle/record/view.rs) が、記録から現在値・衝突・gap・構造の違反を導出する。これらは保存項目にせず、[現在値の導出](../reference/storage.md#現在値の導出) に従う。履歴と Note の順序は [CLI 契約](../reference/cli.md#noteと履歴) に定める。

親記録と照合する検査では、Entity の現在の種類ではなく、その記録の時点の種類を使う。種類の変換前に Issue として行った `Start`・`Release` を、現在 Group であることを理由に破損としないためである。親記録が欠けている場合は、その親との照合を省き、gap として保持する。

親記録があるときは、記録の種類が変更を許す項目以外が変わっていないことも検査する。

| 記録の種類 | 変更できる現在値の項目 |
| --- | --- |
| `transition` | lifecycle、owner |
| `edit` | title、description |
| `label` | label |
| `parent` | parent |
| `dependency` | needs |
| `condition` | condition |
| `convert` | kind |
| `import` | title、description、label、parent、needs |
| `resolve` | 選択した head と同じ現在値 |

## 検査と codec

[codec](../../crates/axon-core/src/lifecycle/record/codec.rs) は記録 1 件の bytes と値の相互変換を担う。decode に成功した bytes は encode の結果と一致し、記録 ID はその bytes から計算する。項目・canonical 形式・拒否条件は [記録 file と codec](lifecycle-file.md#記録-file-と-codec) に定める。

コアは記録単体と記録間の整合性を検査し、保存 adapter は file の列挙と名前・hash の照合を担う。記録の親の循環は集合の検査で拒否するが、親記録の欠けや同じ Entity の複数の作成記録は拒否せず、gap や衝突として導出する。

検証の入口は [各層の検証入口](architecture.md#各層の検証入口)、モデルと Rust の分担は [検証方針](verification.md#モデルと運用検証) を参照する。
