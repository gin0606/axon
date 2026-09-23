# 保存層の再設計の前提 (草案)

この文書は、file 保存と Git 統合の設計をやり直すにあたって確定した前提と判断を残す。2026-09-23 時点の合意であり、Quint モデルで検証してから Axon の計画として登録する。現在の契約は [保存と統合の契約](../reference/storage.md) と [lifecycle](../reference/lifecycle.md) が定義しており、この文書はそれらを置き換える設計の根拠になる。モデルで前提が崩れた場合はこの文書を直す。

## 維持するもの

- コアの意味論。Issue と Group、`Undecided`・`NotStarted`・`InProgress`・`Completed`・`Cancelled` の状態、親子と dependency の制約、Note の追記専用、乱数 ID、導出値の定義固定。統合に必要な規則はコアに足す。
- ignore 運用。linked worktree に `.axon` が無いときに main worktree の保存先を読む探索規則。これはデータ形式に影響しない。
- 記録者、declaration、CLI の公開面。保存層と直交する。

## 追跡する運用に求めること

1. 作業 tree の tracked file だけで Axon の状態が完全に決まる。`git checkout` や `git reset --hard` の後に `axon` を実行すれば、その commit 時点の状態が読める。tree の外に意味を持つ状態を置かない。
2. 別 worktree で同じ Group の子 Issue を進め、両側が親 Group に Note を付ける、という正常系は、merge でも rebase でも人の判断なしに統合される。
3. 同じ Issue を両側で `Start` した場合や、統合結果がコアの意味論に反する場合は、黙って成功させない。次の `axon` コマンドと CI の `axon storage check` が拒否し、解決するまで操作できない。

Git は Axon の意味を理解できないため、統合の判定は Axon が行う。ただし判定の時点は Git の merge 中ではなく、次の読取に置く。

## 保存層の判断

- **保存先は不変な記録の集合。** 現在値は保存せず、記録から導出する。本文・親・dependency・条件・kind の編集も記録にする。
- **書込は 1 行追記。** snapshot 全体の書き直し、temporary、atomic replace、元 bytes の再照合は持たない。途中で切れた行は次の読取で検出する。順序の並べ直しは記録を減らさない限り安全である。古い branch が同じ行を再度足しても、集合として同じになる。
- **Git 統合は union 属性に任せ、driver は持たない。** `axon init` が `.axon/.gitattributes` に `merge=union` を書く。repository 内の file なので clone ごとの設定は要らない。root の file には触れない。並行して追記された行は Git が両方残し、意味の衝突は読取で検出する。
- **Entity ごとの記録 DAG を Git と同型に扱う。** 追記が commit、並行した記録が branch、複数の head を持つ Entity が衝突中、複数の head を親に持つ解決記録が merge。`axon resolve` が解決記録を書く。`axon merge prepare|check|apply`、workspace、manifest、preimage、merge driver は廃止する。三者比較と base は不要になる。
- **Note は因果を持たない集合。** 表示順は日時と ID で決める。
- **欠けた親記録は適用して報告する。** 状態記録は遷移の前後の状態を自身が持つため、head の記録だけから現在値を導ける。cherry-pick や squash で親が欠けた記録は、現在値には反映し、履歴の隙間として報告する。これは作業仮説であり、replica モデルで収束が崩れないことを確かめてから確定する。

## コアに足す規則

- **Issue の `Start` は排他。** 同じ Issue の並行した `Start` は衝突として複数 head になる。
- **記録の種類ごとの統合規則。** 同じ効果になる並行記録は自動で一つに束ね、異なる並行記録は衝突とする。Note は常に追記。
- **解決記録の前提条件は通常操作と同じ。** 解決で `Completed` の head ではなく `NotStarted` の head を選ぶ場合も、下の `Completed` を戻す操作と同じ条件を検査する。
- **衝突中の Entity と、全体検査に反する状態は、解決するまで操作を拒否する。** 別 Entity の変更が組み合わさって生じる循環や、終了した Group への子の追加も、この拒否で捕まえる。

## コアの変更

### Group の `Start`・`Release` を廃止する

子の `Start` に親の `InProgress` を要求する規則は儀式になっており、skill が親を自動で `Start` している。並行 worktree で両側が親を `Start` する統合の場合も、この儀式から生じる。

- Group の保存する lifecycle は `Undecided`・`NotStarted`・`Completed`・`Cancelled` の四つ。`InProgress` は「直属の子に `InProgress` か `Completed` がある」で導出する。`Cancelled` の子だけでは `InProgress` にしない。
- Group の `Complete` は `NotStarted` からも許す。空の Group と、子がすべて `Cancelled` の Group を終えるためである。最終確認の明示入力は残す。
- 子の `Start` は、全祖先が採用済みで未終了であることと、全祖先の dependency が `Completed` であることを要求する。Group の `Start` が担っていた dependency の検査を、子の `Start` へ移す。
- `InProgress` の Entity の移動先を `InProgress` の Group に限る規則は、導出によって意味を失うので消す。

### `axon tasks` の Group の行

平らな一覧を維持する。Issue の行は今までどおり自身の lifecycle と dependency から `Ready`・`Blocked`・`InProgress` を出す。Group の行は配下から導いた状況を出し、上から順に最初に当てはまるものを一つ表示する。

| 状況 | 条件 | 次にできること |
| --- | --- | --- |
| `Confirmable` | 直属の子がすべて終了し、自身の dependency が `Completed` | `axon complete` で最終確認 |
| `Ready` | 着手可能な子孫が一つ以上ある | 子の `axon start` |
| `InProgress` | `InProgress` の子孫がある | 進行を見守る |
| `Blocked` | 未着手の子孫はあるが、どれも着手可能でない | dependency の完了待ち |
| `Empty` | 子がない | 計画を書く、または `axon complete` |

`Ready`・`Blocked`・`InProgress` の語は Issue と共有するが、条件は分ける。Group の `Ready` は Group 自身への操作ではなく、配下に着手できる子があることを意味する。`Confirmable` と `Empty` は Group だけの語で、`Empty` の Group も一覧に出す。子の内訳の件数は `axon show` に置く。

### `Completed` を戻す操作を足す

直接の dependent に `Completed` がなく、親が終了していない `Completed` の Entity を、明示操作で `NotStarted` へ戻す。状態は五つのまま操作が一つ増える。不変条件は「`Completed` は永久」から「`Completed` の dependency はすべて `Completed`」へ言い換える。統合で `Completed` 側を捨てて未完了側を選ぶ例外は、この操作と同じ条件の解決に置き換わる。

### 終了した Entity の文面を編集できるようにする

終了で固定するのは、不変条件が依存する関係だけとする。`Completed` の dependency と、終了した Group の子の構成は固定し、title・description・条件は編集できる。編集は記録に残り、終了時点の文面は終了の記録の時点から復元できる。終了後に編集された Entity であることを `axon show` で示す。

### kind を現在値にする

kind を Entity の同一性ではなく、記録から導く現在値として持つ。未終了の Issue を Group へ変換する操作と、子のない Group を Issue へ戻す操作は、この形式の上に後から一つの記録種別として足せる。`InProgress` の Issue を Group へ変換した場合、その Entity の状態は子からの導出に切り替わり、変換の記録が lifecycle を `NotStarted` へ動かす。変換操作を今回の計画に含めるかは、モデルの書き直しの量を見て決める。

## 採らなかったもの

- Git の merge driver で意味を見る案。設定済みの clone では衝突の時点で Git が止まるが、clone ごとの `git config` が要り、設定漏れの clone では正常系でも conflict marker で止まる。データ設計は driver の有無で変わらないため、早く止めたくなった時に `.axon/.gitattributes` を書き換えるだけで repository 単位に足せる。
- Entity ごとの file に分ける案。`git log --stat` は読みやすいが、`git reset --hard` は untracked file を消さないため、未 commit の新規 Entity が残る。
- 記録 ID を内容だけから決め、同じ操作を同じ行にする案。Issue と Group の `Start` の規則を hash の中に隠すことになる。
- Group を見出しにした木の一覧。今回の変更と独立した表示の改善なので切り離す。

## 次の手順

1. `spec/group_lifecycle.qnt` と `spec/candidate_evaluation.qnt` を、Group の導出 `InProgress`、子の `Start` の祖先条件、`NotStarted` からの Group `Complete`、`Completed` を戻す操作、`axon tasks` の五つの状況に合わせて改訂する。Issue と Group を一つの Entity 空間に kind で持つ形にし、既存の不変条件が保たれること、状況が排他であること、`Confirmable` なら `Complete` が許されることを検査する。
2. 二つの replica が記録の集合を持ち、和集合で統合し、複数 head を解決記録で束ねるモデルを新しく書く。統合の順序と回数によらず同じ状態に収束すること、head が一つの Entity の導出状態が単線モデルで到達可能な状態と一致すること、同じ Issue の並行した `Start` が必ず複数 head になること、解決後の状態が全体検査を通ること、記録の部分集合だけを持ち込んでも収束が崩れないことを検査する。
3. モデルで確かめた前提をもとに、保存層とコアの二つの計画を Axon に登録する。
