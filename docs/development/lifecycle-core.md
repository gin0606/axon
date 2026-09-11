# 単一 lifecycle の共通コア

正本は [literate spec](../../spec/lifecycle_proposal.md)。新しい [Rust library](../../src/lib.rs) の `lifecycle` module は SQL、filesystem、外部コマンド評価を呼ばない。`Snapshot` の操作と検査、`encode` / `decode` の byte 列を、後続の両 backend が共通で使う。`cargo test --lib` で独立したメモリ上の fixture を検証する。

この境界が扱うのは、包含・dependency のない Issue / Group の基本遷移、文面編集、Note、分岐した記録と明示選択である。Group の追加 guard、包含・dependency と候補一覧、条件設定操作、保存 adapter、三者比較の自動統合、公開 CLI は後続の実装範囲。関係を持つ Entity を基本遷移だけで操作する入口はまだ提供しない。既存 `src/main.rs` の module と binary 用テストは置換前の三軸 CLI に属し、新仕様の規範にしない。

## 現在値と不変な記録

`Entity` は ID、kind、作成日時、作成記録と状態先端への参照、現在の文面・lifecycle・任意の条件コマンドを持つ。ID は日時や連番から生成せず、乱数で生成する。`Snapshot` は所有する map を非公開にし、read-only な参照だけを返す。通常操作の失敗は現在値・記録を変えない。

作成は `Undecided` または `NotStarted` の初期記録を作り、架空の採用履歴を作らない。成功した lifecycle 操作は前後の状態・操作・任意の理由を保持する一件の状態記録を追加し、現在値と先端を同時に更新する。文面編集は現在値だけを変更し、採用時の全文保存や文面の編集履歴を追加しない。Note は状態と独立した追記専用の記録であり、同じ本文も別 ID にする。記録者の任意の JSON object は保存するだけで、操作の権限として使わない。

状態記録と Note は別々の因果 DAG を持つ。各参照は同じ Entity・同じ stream 内に限る。通常の状態操作は現在の状態先端を、Note 追加はその Entity の Note 先端すべてを親にする。`history` / `notes` と canonical codec は因果順で返し、並行記録の表示順だけを ID で決める。並行か先行かの判定は `precedes` を使う。日時の大小や同一時刻は先行関係を作らない。日時は UTC の瞬間と小数秒を保持し、入力表記の offset は正規化する。

## 明示統合

`Snapshot::integrate` は入力と出力を別 snapshot にし、同じ store の両入力にある不変記録を和集合にする。同じ ID の同じ記録は共有し、異なる内容なら拒否する。すべての Entity について左右どちらの現在値を採るかを呼び出し元が明示する。入力にない現在値の作成や、文面と完了の項目別合成はしない。三者比較・自動解決・保存先への適用を行う merge engine ではない。

統合記録は入力の状態先端と現在値、それらのうち採用した入力、日時・記録者・任意の理由を保持する。両側が同じ状態先端でも、履歴を作らない文面編集の違いを入力値として残せる。統合後の状態先端はこの記録となる。実際に起きた状態遷移を統合に代えて捏造せず、完了分岐と解放分岐を残したまま未完了側を選べる。続く `Start` は選択された `NotStarted` から始まる通常遷移であり、`Completed` 自体の再開経路ではない。

## 検査と canonical codec

`Snapshot::validate` は因果 DAG、同一 Entity・stream の参照、単一の作成記録、通常遷移の前後と親の一致、統合入力と選択、現在の lifecycle と先端の一致を検査する。状態先端は保持する全状態記録を因果的に包含する必要があり、統合記録なしに片側の先端だけへ戻す snapshot は拒否する。`encode` / `decode` と明示統合はいずれもこの検査を通す。文面の通常編集履歴は仕様上保存しないため、過去の全編集を再生・証明する仕組みではない。

JSONL の header は `format: "axon-lifecycle/v1"` と store ID を持ち、Entity、State、Note の行を続ける。Entity は ID 順、各 stream の記録は因果順・並行時 ID 順、object のキーは決定的な順にする。decode は header を先頭に要求し、以降の行順は自由だが、未知 format / field、重複 ID、参照切れ、因果循環、状態の根拠不整合は拒否する。終端状態の統合先端では、選択した文面と現在の文面の一致も検査する。記録者 metadata の JSON 数値は任意精度の表現で保持し、整数の桁あふれや小数の丸めで内容や記録の同一性を変えない。decode 後の encode で同じ canonical bytes に収束する。これは共通の論理 snapshot 表現であり、旧 `.axon/state.jsonl` の読み替えや移行ではない。

## 検証の対応

- 基本遷移: `lifecycle_rules` / `lifecycle_proposal` の七操作、Completed の固定。Rust は全状態 × 全操作の行列と失敗時の原子性を検査する。
- 情報操作: `lifecycle_information` の編集制約・他 Entity 不変・Note 追記・状態と履歴の一体性。Rust は Issue / Group、全 lifecycle、同内容の独立 Note、任意の記録者情報を検査する。
- 分岐と保存: 正本の「SQLite と file backend の実装範囲」を Rust の縦断テストで検査する。日時逆転、並行履歴、明示選択後の通常操作、不正な参照・ID 衝突、canonical bytes 往復を含む。通常操作モデルの一本の履歴へ統合を押し込めない。

2026-09-11 のこの境界の検証では、lmt で正本から生成し、Quint 0.32.0 / Rust backend / 8 threads / 各 10,000 traces を実行した。基本モデルは最大 80 steps、seed `2026091002`、9 invariant に反例なし・全19 witness 到達。情報モデルは最大 60 steps、seed `2026091101`、7 invariant に反例なし・全18 witness 到達。bounded random simulation の結果であり、Rust の証明や全状態の証明ではない。再現 command は正本の各モデルの検査節を参照する。
