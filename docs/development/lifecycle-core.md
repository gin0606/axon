# 単一 lifecycle の共通コア

正本は [literate spec](../../spec/lifecycle_proposal.md)。新しい [Rust library](../../crates/axon-core/src/lib.rs) の `lifecycle` module は SQL、filesystem、外部コマンド評価を呼ばない。`Snapshot` の操作と検査、`encode` / `decode` の byte 列を、両 backend が共通で使う。`cargo test -p axon-core` で独立したメモリ上の fixture を検証する。

この境界は Issue / Group の登録、基本遷移、包含・dependency の変更、文面編集、Note、分岐した記録と明示選択を扱う。候補評価は `candidates`、条件設定は `set_condition`、三者比較は `MergePlan` が扱う。file adapter と `axon merge` CLI は [file保存とGit統合](lifecycle-file.md) に接続する。[SQLite CLI](lifecycle-sqlite.md) が保存 adapter と公開入口を提供する。`archive/three-axis/src` の旧 module と `archive/three-axis/tests` は置換前の三軸 CLI に属し、新仕様の規範にしない。

## 通常操作と構造

`Current` は最大一つの親 Group と outgoing dependency 集合を保持する。`create`、`set_parent`、`add_dependency` / `remove_dependency` は候補 snapshot の全体検査後に確定し、拒否時は記録も現在値も変えない。同値の関係指定は成功した no-op とする。状態変更は `check_operation` と同じ前提を検査し、成功時だけ履歴を加える。外部条件は評価しない。

子の着手には親の `InProgress`、着手・完了には直接依存先すべての `Completed` が必要になる。Group の解放は進行中の子がいると拒否し、完了・取りやめは未終了の子がいると拒否する。`perform(..., Operation::Complete, ...)` 自体を Group 全体の最終確認済みという明示入力とする。`check_operation` や子の終了は最終確認を記録せず、親を自動変更しない。

終了した親の構成と配下の lifecycle は固定する。終了した Entity 自体の所属は、元と先の親が終了していなければ変更できる。`InProgress` の部分木は `InProgress` の親へ、または所属なしへ移動できる。`Completed` の outgoing dependency は固定し、`Cancelled` の依存編集は許す。

全体検査は包含の参照・循環と、進行中の祖先、終了した Group の子孫、`Completed` の依存先を検査する。通常完了の前提を「直属の子、自身と全祖先の依存先」へ縮約し、動的な Entity 集合に Kahn 法を適用する。`Completed` / `Cancelled` もグラフに含む。codec と明示統合もこの検査を使い、統合では選択した終了済み Group の全子孫の集合・所属・lifecycle も選択元と照合する。

## 現在値と不変な記録

`Entity` は ID、kind、作成日時、作成記録と状態先端への参照、現在の文面・lifecycle・任意の条件コマンドを持つ。ID は日時や連番から生成せず、乱数で生成する。`Snapshot` は所有する map を非公開にし、read-only な参照だけを返す。通常操作の失敗は現在値・記録を変えない。

作成は `Undecided` または `NotStarted` の初期記録を作り、架空の採用履歴を作らない。成功した lifecycle 操作は前後の状態・操作・任意の理由を保持する一件の状態記録を追加し、現在値と先端を同時に更新する。文面編集は現在値だけを変更し、採用時の全文保存や文面の編集履歴を追加しない。Note は状態と独立した追記専用の記録であり、同じ本文も別 ID にする。記録者の任意の JSON object は保存するだけで、操作の権限として使わない。

状態記録と Note は別々の因果 DAG を持つ。各参照は同じ Entity・同じ stream 内に限る。通常の状態操作は現在の状態先端を、Note 追加はその Entity の Note 先端すべてを親にする。`history` / `notes` と canonical codec は因果順で返し、並行記録の表示順だけを ID で決める。並行か先行かの判定は `precedes` を使う。日時の大小や同一時刻は先行関係を作らない。日時は UTC の瞬間と小数秒を保持し、入力表記の offset は正規化する。

## 明示統合

`Snapshot::integrate` は入力と出力を別 snapshot にし、同じ store の両入力にある不変記録を和集合にする。同じ ID の同じ記録は共有し、異なる内容なら拒否する。すべての Entity について左右どちらの現在値を採るかを呼び出し元が明示する。入力にない現在値の作成や、文面と完了の項目別合成はしない。三者比較・自動解決・保存先への適用を行う merge engine ではない。

統合記録は入力の状態先端と現在値、それらのうち採用した入力、日時・記録者・任意の理由を保持する。両側が同じ状態先端でも、履歴を作らない文面編集の違いを入力値として残せる。統合後の状態先端はこの記録となる。実際に起きた状態遷移を統合に代えて捏造せず、完了分岐と解放分岐を残したまま未完了側を選べる。続く `Start` は選択された `NotStarted` から始まる通常遷移であり、`Completed` 自体の再開経路ではない。

## 三者比較の統合 engine

`MergePlan::prepare(base, left, right)` は検査済みの同じ store の snapshot を保持し、Entity ごとの自動選択と未解決の左右 `Candidate` を返す。base の Entity と全不変記録が両分岐に残ること、分岐間の Entity identity と記録 ID に異内容がないことを検査する。削除や記録の書換えは統合で補完せず拒否する。

比較対象は文面、lifecycle、条件、所属、dependency を含む `Current` 全体である。片側変更と両側同値を採用し、両側で異なる値へ変わった Entity は、項目が違っても衝突として残す。状態先端が異なる同値の変更は、その両記録を保持する統合記録で結ぶ。Note だけの変更と片側だけの新規 Entity には架空の状態記録を追加しない。

`resolve(choices, reason, context)` は衝突した Entity の明示選択を要求し、候補全体へ包含・依存・終了構成の検査を適用する。構造衝突を直すため、自動選択した Entity も左右の全体値で上書き選択できる。未解決、入力にない Entity/側、循環、終了 Group の選択元と異なる子孫集合・所属・lifecycle は拒否し、入力は変更しない。異なる終了状態の子を選ぶ場合も、それを確認して終了した Group の構成と一致させる必要がある。候補確定後の通常編集は `Snapshot` の通常操作と同じ制約に従う。

再統合では、選択済みの先端が相手を包含し、同じ現在値であるか、過去の統合入力に相手の先端と現在値の完全な組が残っていれば、その先端を再利用する。逆向きの取り込みでも記録を増殖させない。文面編集は履歴を作らないため、状態先端の先行関係だけでは過去の選択済み入力と見なさない。直接の `Snapshot::integrate` は引き続き明示的な統合記録を作る低水準入口である。

この engine は条件文字列を保存値として比較するだけで、環境、shell、filesystem、SQLite、Git にアクセスしない。`axon merge prepare|check|apply` の公開 CLI、入力ファイルの保全と変更検知、正本への公開は file adapter 側で接続する。

## 検査と canonical codec

`Snapshot::validate` は因果 DAG、同一 Entity・stream の参照、単一の作成記録、通常遷移の前後と親の一致、統合入力と選択、現在の lifecycle と先端の一致を検査する。状態先端は保持する全状態記録を因果的に包含する必要があり、統合記録なしに片側の先端だけへ戻す snapshot は拒否する。`encode` / `decode` と明示統合はいずれもこの検査を通す。文面の通常編集履歴は仕様上保存しないため、過去の全編集を再生・証明する仕組みではない。

JSONL の header は `format: "axon-lifecycle/v1"` と store ID を持ち、Entity、State、Note の行を続ける。Entity は ID 順、各 stream の記録は因果順・並行時 ID 順、object のキーは決定的な順にする。decode は header を先頭に要求し、以降の行順は自由だが、未知 format / field、重複 ID、参照切れ、因果循環、状態の根拠不整合は拒否する。終端状態の統合先端では、選択した文面と現在の文面の一致も検査する。記録者 metadata の JSON 数値は任意精度の表現で保持し、整数の桁あふれや小数の丸めで内容や記録の同一性を変えない。decode 後の encode で同じ canonical bytes に収束する。これは共通の論理 snapshot 表現であり、旧 `.axon/state.jsonl` の読み替えや移行ではない。

## 検証の対応

- 基本遷移: `lifecycle_rules` / `lifecycle_proposal` の七操作、`Completed` の固定。Rust は全状態 × 全操作の行列と失敗時の原子性を検査する。
- 情報操作: `lifecycle_information` の編集制約・他 Entity 不変・Note 追記・状態と履歴の一体性。Rust は Issue / Group、全 lifecycle、同内容の独立 Note、任意の記録者情報を検査する。
- 分岐と保存: 正本の「SQLite と file backend の実装範囲」を Rust の縦断テストで検査する。日時逆転、並行履歴、明示選択後の通常操作、不正な参照・ID 衝突、canonical bytes 往復を含む。通常操作モデルの一本の履歴へ統合を押し込めない。

2026-09-11 のこの境界の検証では、lmt で正本から生成し、Quint 0.32.0 / Rust backend / 8 threads / 各 10,000 traces を実行した。基本モデルは最大 80 steps、seed `2026091002`、9 invariant に反例なし・全19 witness 到達。情報モデルは最大 60 steps、seed `2026091101`、7 invariant に反例なし・全18 witness 到達。bounded random simulation の結果であり、Rust の証明や全状態の証明ではない。再現 command は正本の各モデルの検査節を参照する。

三者比較の検証は Rust の全値比較行列と分岐 fixture で行う。独立 Entity/Note、同値の並行状態先端、本文と完了の衝突、未完了側選択後の通常操作、双方向の再統合、base 記録欠落・改変、ID 衝突、全体循環、終了 Group の子流入・子孫状態差・入れ子移動を含む。通常 lifecycle モデルの意味は変更しておらず、単線履歴モデルへの merge action の追加は行わない。
