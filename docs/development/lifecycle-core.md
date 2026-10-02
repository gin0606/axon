# 共通コア

この境界が実装する契約は [lifecycle](../reference/lifecycle.md) と [保存と統合](../reference/storage.md)、対応するモデルは [モデル](../../spec/README.md)。[Rust library](../../crates/axon-core/src/lib.rs) の `lifecycle` module は filesystem、外部コマンド評価、Git を呼ばない。記録の集合からの導出、通常操作の前提検査と記録の生成、記録 1 件の `encode` / `decode` を、保存 adapter と CLI が共通で使う。`cargo test -p axon-core` で独立したメモリ上の fixture を検証する。

この境界は Issue / Group の登録、基本遷移、包含・dependency の変更、文面編集、label の設定、種類の変換、Note、衝突の解決記録、構造の違反の導出を扱う。候補評価は `list_candidates` と `Surfacing`、条件設定は `set_condition` が扱う。保存 adapter は [file 保存と Git 統合](lifecycle-file.md) に接続する。[CLI と保存の接続](lifecycle-cli.md) が公開入口を示す。

## 通常操作と構造

現在値は最大一つの親 Group と outgoing dependency 集合を保持する。`create`、`set_parent`、`add_dependency`、`convert`、`perform`（lifecycle 操作）は操作後の view の全体検査で「違反が操作前の部分集合である」ことを確認して確定し（`create`、`set_parent`、`add_dependency` は加えて、操作前に違反があれば、新しく持った dependency、所属、新しく祖先の連なりに加わった Group の依存先が誘導する前提の辺が操作後の循環上に載らないことを確認する。循環上の Entity どうしの辺は違反の集合を変えないため。辺は関係ごとに数え、既存の組と重なっても検査する）、違反を増やせない `remove_dependency`（自身の前提は `Completed` でないか、違反に含まれること）、`write`、`set_label`、`set_condition` は自身の前提で確定する。拒否時は記録を作らない。同値の関係指定は成功した no-op とする。`write` と `set_label` は終了した Entity を、指定した値が現在の値と同じでも同値の判定より先に拒否する（[CLI と表示の契約](../reference/cli.md#mutationの結果)）。状態変更は `check_operation` の前提に加えて全体検査を行い、成功時だけ記録を加える。`check_operation` は免除を、結果が違反を増やさない場合にだけ与える（終了した親の下では、未終了のまま留まる操作と終了させる操作。親の記録が欠けているだけならそれ以上は縛らない。`Completed` の依存元による阻止では、その依存元がすでに未完了の依存先を持つ違反にある場合）ので、免除の場面では読取側の判定と書込の結果が一致する。規則は書込の全体検査で、読取側が先取りしない場面では書込だけが拒否する。外部条件は評価しない。衝突中の Entity が一つでもある store では、解決と Note 以外の操作を拒否する。

操作の前提は `Operation::apply_as` の種類別の基本遷移と `check_operation` が検査する。Group は `Start`・`Release` を受け付けず、保存値が `InProgress` になることはない。実効 lifecycle は `working_groups` / `effective_lifecycle` が子から導出する。Issue の `Start` は全祖先の保存値が `NotStarted` で、自身と全祖先の直接依存先がすべて `Completed` であること、`Complete` は自身の依存先の `Completed` と全祖先の採用と直属の子がすべて終了していること（統合で子を持った Issue も同じ。`Cancel` も直属の子の終了を要求する）、`Reopen` は全祖先の採用と `Completed` の依存元がないこと、Group の `Withdraw` は実効値が `NotStarted` であること、着手・完了した子孫を持つ Group の `Accept` は全祖先の採用を要求する。`perform(..., Operation::Complete, ...)` 自体を Group 全体の最終確認済みという明示入力とする。`check_operation` や子の終了は最終確認を記録せず、親を自動変更しない。衝突中の Entity は現在値を持たないため、祖先や依存先が衝突中ならこれらの前提を満たさない。「全祖先が採用済み」（`ancestors_adopted`）は親の連なりの各段が settled で `NotStarted` であることを要求し、連なりの途中に衝突中または記録の欠けた Entity があれば、その下の settled な祖先がすべて `NotStarted` でも満たさない。

終了した親の構成と配下の lifecycle は固定する。終了した Entity 自体の所属は、元と先の親が終了していなければ変更できる。実効値が `InProgress` か `Completed` の Entity は、移動先自身を含む全祖先の保存値が `NotStarted` の Group の下へ、または所属なしへ移動できる。`Completed` の outgoing dependency は固定し、`Cancelled` の依存編集は許す。違反に含まれる Entity には、終了した親の配下の固定、`Completed` の dependency の固定、`Completed` の依存元による `Reopen` の阻止を免除する。免除は前提を緩めるだけで、全体検査は免除しない（[構造の違反と修復](../reference/storage.md#構造の違反と修復)）。

全体検査は settled な Entity の現在値から違反の集合を導出する。種類は包含の循環、親の不在、終了した親の下の未終了、`InProgress` の Issue と実効 `InProgress` の Group の未採用の祖先、`Completed` の未完了の依存先、依存先の不在、通常完了経路の循環である。通常完了の前提を「直属の子、自身と全祖先の依存先」へ縮約し、動的な Entity 集合に Kahn 法を適用して前提の残っていない Entity を除き、残った Entity のうち前提をたどって自身に戻れるもの（循環上の Entity。祖先が自身の子孫に依存する場合は自身への辺）だけを通常完了経路の循環の違反とする。循環を待つだけの Entity は違反に含めない。包含の循環も同じく、自身が自身の祖先に現れる Entity だけに付く。`Completed` / `Cancelled` もグラフに含む。違反は操作を止めず、通常操作が違反を増やさないことの判定と、免除の判定に使う。

## 記録の集合と導出

記録は Entity ごとの不変な事実で、ID は内容の hash、Entity、種類、親記録の ID の集合、日時、記録者、任意の理由、操作後の現在値（種類、lifecycle、`InProgress` の owner、title、description、label、条件、親、dependency）を持つ。label は固定集合の `Label` で、綴りの表は `Label::name` にあり、CLI と declaration もこれを使う。Note は親を持たない記録で本文を持つ。項目と codec は [記録 file と codec](lifecycle-file.md#記録-file-と-codec) に定める。

記録の集合から view を導出する。Entity ごとに、Note 以外の記録のうちどの記録の親にもなっていない記録を head とし、head が一つなら settled でその現在値、複数なら衝突中で現在値なし。親が集合にない記録を持つ Entity は gap を持つ。settled な Entity の現在値から実効 lifecycle と違反の集合を導出する。導出は入力の順序に依存せず、同順位の並びは記録 ID で固定する。view は記録から導出できる値であり、保存項目ではない。

作成は `Undecided` または `NotStarted` の初期記録を作り、架空の採用履歴を作らない。成功した lifecycle 操作は操作名と操作後の現在値を持つ一件の記録を追加し、変更前の状態は親記録の現在値として読める。文面編集、所属変更、dependency の追加と削除、条件の設定と解除、種類の変換もそれぞれ一件の記録で、操作後の現在値を持つ。declaration の一括反映は Entity ごとに最終値を持つ一件の記録を作る。Note は状態と独立した追記専用の記録であり、乱数の nonce を内容に含めて同じ本文・日時・記録者の Note も別 ID にする。記録者の任意の JSON object は保存するだけで、操作の権限として使わない。

`history` は Entity の Note 以外の記録を因果順で返し、並行する分岐を枝ごとにまとめる。保存先にある親をすべて返し終えた記録を返せる記録とし、次に返すのは、直前の記録の返せる子の最小 ID、無ければ返せる子が残っている記録のうち最後に返したもの（直近の分岐点）の返せる子の最小 ID（深さ優先）、それも無ければ親を持たない作成の記録、親がすべて保存先にない記録（gap の根）の順で最小 ID で、この選び方は表示順だけを決める。並行か先行かの判定は親をたどる `precedes` を使う。日時の大小や同一時刻は先行関係を作らない。`notes` は日時順、同時刻は ID 順で返す。日時は UTC の瞬間と小数秒を保持し、writer が入力表記の offset を UTC に正規化してから記録にする。

lifecycle 遷移の記録の妥当性は、その遷移の時点の種類（直前の記録の現在値の種類）の規則で判定する。遷移は種類を変えないので、その記録自身の現在値の種類がその時点の種類である。遷移元は親記録の現在値から取り、親記録が欠けている記録ではこの検査を行わない。親記録がある記録は、その種類が変えてよい項目（遷移は lifecycle と owner、文面編集は title と description、label の設定は label、所属変更は parent、dependency の増減は needs、条件は condition、変換は kind、declaration の適用は title・description・label・parent・needs、解決は採った head と同じ値）以外を親記録の現在値から変えていないことも検査し、変えていれば破損とする。Issue として `Start`・`Release` してから Group に変換した履歴は妥当で、現在の種類で判定すると不正になる。変換の記録は操作後の種類を持ち、変換前の種類はその反対なので、記録だけから両方が読める。

## 衝突と解決

Issue の `Start` は現在値に着手した actor を owner として持つ。並行した `Start` は別の head として衝突に見え、値の等しい並行記録や同じ ID の二重登録も衝突とし、黙って畳まない。

`resolve` は衝突中の Entity の head から一つを指定させ、全 head を親にし、指定した head の現在値と記録 ID を持つ解決記録を一つ作る。値の捏造も項目別の合成もしない。解決記録は違反の検査を免除し、解決の直後にその Entity は settled になる。解決の後に残る違反は通常操作で直す。祖先関係にある親を持つ解決記録（gap による偽の衝突の解決）も通常の解決記録として受け入れる。解決記録の後に続く `Start` は選択された `NotStarted` から始まる通常遷移であり、`Completed` 自体の再開経路ではない。`Completed` を戻す経路は通常操作の `Reopen` だけである。

## 検査と codec

`decode` は記録 1 件の bytes を検査し、未知 field、欠けた field、種類と合わない field、規則外の値、canonical でない bytes を拒否する。`encode` は canonical bytes を返し、decode に成功した bytes は encode の結果そのものである。記録 ID は bytes から計算し、記録の内容に含まない。file の列挙、hash と file 名の照合、途中で切れた file と空の file の検出は保存 adapter が行い、コアは記録の集合を受け取る。

view の導出は参照切れ（親記録の欠け）を gap として保持し、拒否しない。同じ Entity の複数の作成記録は二重登録の衝突として head に残し、拒否しない。親の循環は記録 ID が内容の hash であることから起きないが、集合の検査で拒否する。

## 検証の対応

- 基本遷移: [`spec/lifecycle_rules.qnt`](../../spec/lifecycle_rules.qnt) と [`spec/issue_lifecycle.qnt`](../../spec/issue_lifecycle.qnt) の八操作と種類別の前提。Rust は種類ごとの全状態 × 全操作の行列と失敗時の原子性を検査する。
- 包含と実効 lifecycle: [`spec/group_lifecycle.qnt`](../../spec/group_lifecycle.qnt) の witness に対応する Rust テストが、`Reopen` 後の再着手、完了済みの子を持つ Group の `Reopen`、`Completed` の依存元による `Reopen` の拒否、空の Group と全子 `Cancelled` の Group の完了、祖先の dependency による着手の阻害と解禁、三階層の実効 `InProgress`、孫が着手中の祖父の `Withdraw` の拒否、一覧の五つの状況と優先順位、詰まっている理由を検査する。
- 情報操作: [`spec/lifecycle_information.qnt`](../../spec/lifecycle_information.qnt) の編集制約・他 Entity 不変・Note 追記・状態と履歴の一体性・各時点の種類での履歴の再生。Rust は Issue / Group、全 lifecycle、同内容の独立 Note、任意の記録者情報、変換をまたぐ記録列の判定を検査する。
- 記録の集合と統合: [`spec/record_integration_test.qnt`](../../spec/record_integration_test.qnt) の `run` テストと [`spec/record_integration.qnt`](../../spec/record_integration.qnt) の主要 witness（別 actor の並行 `Start` の衝突、値の等しい並行記録の衝突と解決、解決記録どうしの並行、gap と偽の衝突、終了 Group への子の流入と `Reopen` による修復、両側の移動による循環と取り外し、完了済みへの未完了 dependency の流入、二重登録の解決）に対応する Rust テストが、通常操作が衝突のある store で拒否されること、違反を増やす移動・dependency 追加・登録が拒否されること、免除が働くこと、免除の下でも `Reconsider`・`Reopen` が違反を増やす場合は拒否され違反を減らす修復は通ること、循環を待つ Entity どうしの新しい循環が拒否され待つ側への追加と登録は通ること、既存の循環への chord、別の循環の構成員どうしで循環を閉じる dependency、所属変更と登録で親や子孫に誘導される辺、既存の組と重なる関係が循環上に載れば拒否され、循環上にない Entity の追加と移動、共通の祖先の下の兄弟 Group への移動、辺の除去による修復は通ること、codec の拒否、入力順によらない導出を検査する。
- label: モデルを持たず（[モデル化と実装検証の分担](../../spec/README.md#モデル化と実装検証の分担)）、Rust テストで検証する。
  - 固定集合: [`record/tests/codec.rs`](../../crates/axon-core/src/lifecycle/record/tests/codec.rs) の `decode_rejects_truncated_empty_unknown_missing_and_non_canonical_input` が `Label::ALL` の全綴りの往復と、記録の label の欠落・集合外の値の拒否を検査する。
  - 終了後の固定: [`record/tests/operations.rs`](../../crates/axon-core/src/lifecycle/record/tests/operations.rs) の `a_label_changes_like_text_and_leaves_everything_else_alone` が、`Completed` / `Cancelled` の Issue と Group への `set_label` を同値でも拒否すること、`Store::import` が `Completed` の Issue の label を変えられず同値なら no-op であることを検査する。
  - 種類の変換での保持: [`record/tests/lifecycle_pbt.rs`](../../crates/axon-core/src/lifecycle/record/tests/lifecycle_pbt.rs) の `generated_conversion_changes_only_kind_and_same_kind_is_noop` が、生成した label を持つ Issue を Group へ、または Group を Issue へ変換し、変換の記録が kind 以外を変えないことを検査する。
  - 記録と codec: [`record/tests/conversion.rs`](../../crates/axon-core/src/lifecycle/record/tests/conversion.rs) の `records_that_change_fields_outside_their_kind_are_corruption` が、label の設定と declaration の適用の記録による label の変更を受け入れ、文面編集・所属変更・dependency・条件・遷移・変換の記録による変更を破損とすることを検査する。[`record/tests/codec.rs`](../../crates/axon-core/src/lifecycle/record/tests/codec.rs) の `parent_field_matrix_rejects_forbidden_changes` は、遷移・文面編集・所属変更・dependency・条件・変換・解決の記録で label を変えた記録が、decode は通り、親の記録とともに導出すると破損になることを検査する。同じ file の `every_record_kind_round_trips_through_canonical_bytes_with_its_hash_as_id` と `records_written_earlier_decode_and_re_encode_to_the_same_bytes` が、label の設定の記録を含む各種類の記録と label の項目の canonical な往復を検査する。
  - 共通コアの外では、[`tests/lifecycle/contracts.rs`](../../tests/lifecycle/contracts.rs) の label のテストが CLI の登録時の省略・集合外の値の拒否、設定・絞り込み・種類の変換での保持、`Completed` の Issue での拒否を検査する。declaration の label は [`declaration/tests.rs`](../../crates/axon-core/src/declaration/tests.rs) と [`tests/lifecycle/declaration.rs`](../../tests/lifecycle/declaration.rs) の label のテストが検査する（[Declaration と canonicalな`axon export`](lifecycle-declaration.md)）。

各モデルの探索結果と再現手順は [モデル](../../spec/README.md) の「検証結果」と「再現手順」にある。
