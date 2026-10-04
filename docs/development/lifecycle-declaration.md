# Declaration と保存の境界

形式・競合検知・再試行の契約は [計画全体の取得と一括編集](../reference/declaration.md) に定める。実装は、記録の集合を扱うコアと、入力 file・保存先を扱う adapter に分ける。

## コアの責務

[declaration](../../crates/axon-core/src/declaration.rs) は YAML の解析・出力、編集集合と外部参照の選択、fingerprint を担う。file 内で検査できる形式・参照構造と、保存された記録の集合との照合を分け、filesystem や外部コマンドを呼ばない。

[適用候補の構築](../../crates/axon-core/src/declaration/import.rs) は、競合と Entity ごとの適用済み判定を行い、共通コアの通常操作を使って候補を検査する。lifecycle・包含・dependency の制約を declaration 専用に複製しない。有効な最終状態を中間状態の循環や前提不足で拒否しないよう、関係の解除を追加より先に扱う。入力の並びで適用結果を変えない。

候補の検査は保存を伴わない。保存する記録は Entity ごとの最終値を持ち、検査で用いた通常操作の列をそのまま履歴にしない。

## adapter の責務

[declaration_file](../../src/declaration_file.rs) は、保存先の lock を取った後に入力 file と記録の集合を読み、コアで再検証してから記録を保存する。事前の `axon import check` の成功だけで書き込まない。

記録の保存と declaration の書戻しは別の保存境界である。書戻しには、この反映で保存した記録の集合を使い、lock 解放後に他の writer が追加した変更を base に取り込まない。入力 bytes の再照合と原子的な置換には、file 保存の共通処理を使う。

複数の記録の公開途中で止まった場合と、保存成功後に書戻しが失敗した場合を区別する。再試行では保存済みの Entity を重複作成せず、残りの反映と書戻しを完了できるようにする。詳細は [書込の保証](lifecycle-file.md#書込の保証) と [再試行](../reference/declaration.md#再試行) を参照する。

形式と I/O 境界の検証は [Declaration の独立fixture](verification.md#declaration-の独立fixture) にまとめる。
