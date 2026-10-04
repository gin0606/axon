# lifecycleと構造の契約

この文書は、Issue・Group が持つ lifecycle の状態と遷移、Group の実効 lifecycle、再浮上条件、Groupによる包含、種類の変換、dependency、候補集合、label、文面・Note・状態変更履歴の契約を定める。一覧の行と状況、候補一覧の評価順と外部コマンドの実行は [候補と外部条件](candidates.md)、識別子・引数・表示・保存結果は [CLIと表示の契約](cli.md)、計画全体の一括編集は [Declaration](declaration.md)、記録の保存と Git 統合は [保存と統合の契約](storage.md) に定める。

対応する実行可能なモデルは [`spec/lifecycle_rules.qnt`](../../spec/lifecycle_rules.qnt)、[`spec/issue_lifecycle.qnt`](../../spec/issue_lifecycle.qnt)、[`spec/group_lifecycle.qnt`](../../spec/group_lifecycle.qnt)、[`spec/lifecycle_reachability.qnt`](../../spec/lifecycle_reachability.qnt)、[`spec/candidate_evaluation.qnt`](../../spec/candidate_evaluation.qnt)、[`spec/lifecycle_information.qnt`](../../spec/lifecycle_information.qnt) にある。モデルが前提とする規則の一覧、各モデルの対象範囲・検証する性質・再現手順は [モデルの読み方](../../spec/README.md#モデルが表す規則) を参照する。

## 状態

Issue・Group は一つの lifecycle を持つ。状態の種類は Issue と Group で共有する。

| 状態 | 意味 |
| --- | --- |
| `Undecided` | 未判断。記録済みの提案 |
| `NotStarted` | 採用済み・未着手 |
| `InProgress` | 着手中 |
| `Completed` | 完了 |
| `Cancelled` | 取りやめ |

`Completed` と `Cancelled` を終了とする。登録直後は `Undecided` または `NotStarted` から始まる。`Undecided` は記録済みの提案であり、実施の約束を意味しない。`Accept` によって `NotStarted` へ進む。

保存される lifecycle（保存値）と、表示や前提の判定に使う lifecycle（実効値）を区別する。Issue の実効値は保存値と同じである。Group は保存値として `InProgress` を持たず、実効値を子から導出する（[Group の実効 lifecycle](#group-の実効-lifecycle)）。この文書で「採用済み」は保存値が `NotStarted` であることを指し、`Undecided` と終了した Entity を含まない。

採否を決めるためのまとまった調査は、別の Issue として採用・着手する。元の改善案は `Undecided` のままにできる。この使い分けのために状態を増やさず、複数 Entity の関係は包含と dependency で扱う。

## 遷移

| 操作 | 遷移元（保存値） | 遷移先 | 対象 |
| --- | --- | --- | --- |
| `Accept`：採用 | `Undecided` | `NotStarted` | Issue・Group |
| `Withdraw`：採用撤回 | `NotStarted` | `Undecided` | Issue・Group |
| `Start`：着手 | `NotStarted` | `InProgress` | Issue だけ |
| `Release`：作業を解放 | `InProgress` | `NotStarted` | Issue だけ |
| `Complete`：完了 | Issue は `InProgress`、Group は `NotStarted` | `Completed` | Issue・Group |
| `Cancel`：見送り・取りやめ | Issue は `Undecided`・`NotStarted`・`InProgress`、Group は `Undecided`・`NotStarted` | `Cancelled` | Issue・Group |
| `Reconsider`：再検討 | `Cancelled` | `Undecided` | Issue・Group |
| `Reopen`：再開 | `Completed` | `NotStarted` | Issue・Group |

Group は `Start`・`Release` を持たない。Group の着手は配下の Issue の `Start` から導出し、Group 自身へ着手や解放を記録しない。Group の `Complete`・`Cancel` は保存値が `NotStarted`（Group の `Cancel` は `Undecided` も）のときに行い、実効値が `InProgress` でもよい。

`Completed` から抜ける経路は `Reopen` だけで、`NotStarted` へ戻る。`Cancelled` は `Reconsider` できるが、再び着手するには `Accept` が必要になる。`InProgress` の Issue の採用を撤回する場合は `Release`、`Withdraw` の順に操作する。

各操作は原子的に実行され、前提を満たさない操作は無効となる。遷移表の遷移元に加え、包含と dependency からの前提を [親子のlifecycle](#親子のlifecycle) に定める。`Completed` で受け付ける lifecycle 操作は `Reopen` だけである。再浮上条件の成立状況は終了後も変化するが、`Completed` を取り消さない。

## Group の実効 lifecycle

Group の実効値は、保存値が `NotStarted` で、直属の子のうち実効値が `InProgress` か `Completed` のものがあれば `InProgress`、それ以外は保存値と同じとする。子 Group の実効値も同じ規則で決まるため、孫の Issue の `Start` は `NotStarted` の子 Group を通じて祖父の Group まで `InProgress` にする。`Cancelled` の子だけでは `InProgress` にならない。

実効値は保存せず、子の変化に応じて導出が変わるだけである。配下の Issue を `Start` しても、Group の保存値と履歴は変わらない。一覧と詳細の状況欄と `axon list --lifecycle` の絞り込みは Group の実効値に基づき、保存値は詳細で読める。表記は [CLIと表示の契約](cli.md) に定める。

## 再浮上条件

再浮上条件は、Entity ごとに未設定または一つの外部コマンドとして保持する。条件の種類と評価の契約は [候補と外部条件](candidates.md) に定める。

保存値が `Undecided`・`NotStarted`・`InProgress` の Entity を評価対象とし、`Cancelled`・`Completed` では条件を評価しない。実効値が `InProgress` の Group も保存値は `NotStarted` なので評価対象になる。評価しなかったことと不成立は区別し、評価対象外の Entity は条件の内容にかかわらず浮上しない。判定は保存状態を書き換えない。

「後で考える」は `Undecided` のまま条件で浮上を制御し、「取りやめ」は `Reconsider` するまで浮上対象外にする。`Cancelled` から `Undecided` へ戻すと、同じ条件が再び評価対象になる。条件が成立しているならその時点で浮上し、不成立なら `Undecided` のまま浮上しない。`Reopen` で `Completed` から `NotStarted` へ戻した Entity も、同じ条件が再び評価対象になる。

## 計画と包含

Issue・Group とも所属は最大一つで、所属なしも許す。Group も最大一つの親 Group を持ち、Issue と子 Group を同じ階層に置ける。所属先は Group に限り、Issue は子を持たない。Group の自己包含と、子孫の下への移動による循環を禁止する。所属変更は既存の親子の制約を壊さない限り許可し、lifecycle を変えない。

終了した Group への追加と、そこからの取り外しは不可とする。Group の最終確認は完了の時点で固定した子の集合について行うため、終了後に構成が変わると、確認したGroupの構成と現在の構成が食い違う。構成を変えるには、`Completed` の Group は `Reopen`、`Cancelled` の Group は `Reconsider` で戻してから操作する。どちらの戻しも、その Group の親が終了していれば行えない。

終了した Group の配下の lifecycle も固定する。終了した Group の配下にある Entity の lifecycle 操作は、`Reopen`・`Reconsider` を含めてすべて無効になる。`Completed` の Group 配下にある `Cancelled` の Issue は、Group を `Reopen` するまで `Reconsider` も取り外しもできない。理由は [終了した Group の構成と配下の状態を固定する理由](../design/decisions.md#終了した-group-の構成と配下の状態を固定する理由) に記す。

### 親子のlifecycle

Issue の `Start` には、Issue 自身が `NotStarted` であることに加え、全祖先が採用済みであることと、自身と全祖先の依存先がすべて `Completed` であることを要求する。親 Group の実効値が `InProgress` であることは要求しない。子への `Start` で親の保存値を変えない。Group の dependency は Group 自身の着手ではなく配下の Issue の `Start` で検査されるため、Group に置いた前提は配下の Issue の着手を待たせる。Issue・子 Group の `Complete` は自身の依存先だけを検査し、祖先の依存先を検査しないため、Group の dependency は Group 自身の `Complete` の前提になるが、配下の `Complete` の前提にはならない。理由は [Group の dependency を配下の完了の前提にしない理由](../design/decisions.md#group-の-dependency-を配下の完了の前提にしない理由) に記す。

Group の `Complete`・`Cancel` には直属の Issue・子 Group がすべて終了している必要があり、`Undecided` の子もその妨げになる。子の終了は `Completed`・`Cancelled` のどちらでも満たす。直属の子がない Group と、子がすべて `Cancelled` の Group も `Complete` できる。

Group の `Complete` にはさらに、そのGroup全体の成果の最終確認が通ったという入力を要求する。画面単位の子 Group にも、機能全体の親 Group にも、それぞれ独立した最終確認がある。Group に対する `axon complete ID` の実行自体を最終確認済みの明示入力とし、別のレビュー済み状態や必須フラグを設けない。確認手順は呼び出し側の skill・運用で扱う。空の Group でもこの確認を省略しない。子の終了だけで親を自動的に終了しない。

包含と dependency から各操作が要求する前提は次のとおりとする。すべての lifecycle 操作は、これに加えて親が終了していないことを要求する。

| 操作 | 包含からの前提 | dependency からの前提 |
| --- | --- | --- |
| `Start`（Issue） | 全祖先が採用済み | 自身と全祖先の依存先がすべて `Completed` |
| `Complete` | 全祖先が採用済み。Group と、統合で子を持った Issue（[構造の違反と修復](storage.md#構造の違反と修復)）なら、直属の子が全員終了している | 自身の依存先がすべて `Completed` |
| `Cancel` | Group と、統合で子を持った Issue なら、直属の子が全員終了している | 要求しない |
| `Withdraw` | Group なら、実効値が `NotStarted` | 要求しない |
| `Accept` | Group で、着手・完了した子孫（保存値が `InProgress` か `Completed`）があるなら、全祖先が採用済み | 要求しない |
| `Reopen` | 全祖先が採用済み | 自身を依存先に持つ `Completed` の Entity がない |

`Release`・`Reconsider` と、子孫に着手・完了した Entity のない Group や Issue の `Accept` は、親が終了していないこと以外に包含と dependency からの前提を持たない。未判断の Group の下で子を先に採用でき、登録した子も採用済みになれる。ただし全祖先が採用済みになるまで、その子は着手できない。

`Reopen` は対象の lifecycle だけを `NotStarted` へ戻し、子や依存元の lifecycle を変えない。`Completed` の依存元がある Entity は `Reopen` できないため、依存元から順に `Reopen` する。依存元を `Reopen` するにはその祖先が採用済みである必要があり、数段の `Reopen` を要する場合がある。完了済みの子を持つ Group を `Reopen` すると、その Group の実効値は `InProgress` になる。

守る性質は「実効値が `InProgress` の Group と `InProgress` の Issue の全祖先が採用済み」である。上表で `Complete`・`Reopen` と着手・完了した子孫を持つ Group の `Accept` が全祖先の採用を要求し、Group の `Withdraw` が実効値の `NotStarted` を要求するのはこの性質を保つためである。実効値は `NotStarted` の子 Group を通じて上へ伝わるので、孫が着手中なら祖父も `Withdraw` できない。一方、`Undecided` の Group の下に完了済みの子孫があることは許す。完了済みの子を持つ Group を `Cancel` してから `Reconsider` するとこの状態になり、取りやめた子 Group の下に完了済みの子孫があるときの親の `Withdraw` も同じ理由で禁じない。その Group を再び `Accept` するには全祖先が採用済みであることが要る。

循環の検査には「全員が通常どおり完了する経路」を使う。各 Entity の完了に先行するものを次の集合とする。

```text
完了の前提 = 直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先
```

Issue の着手は全祖先の依存先を待ち、子の終了は通常完了経路では子の完了に対応する。`Cancel` による回避はこの経路に加えない。この前提のグラフから、前提の残っていない節点を順に除去し、全節点を除去できるかを検査する。これは構造の循環判定用の導出であり、実際の操作可否には上表の現在状態を使う。保存する関係は包含と明示的な dependency だけである。

### 所属変更と新規登録

所属変更では、移動元・移動先に Group がある場合、どちらも終了していないことを検査する。実効値が `InProgress` か `Completed` の Entity は、所属なしにするか、移動先の Group 自身を含む全祖先が採用済みである Group の下へ移せる。それ以外の Entity は、終了していない Group の下へ移せる。移動のためだけに `Release` や `Reopen` を挟む必要はなく、移動によって採否や進行の状態は変わらない。移動は移動先とその祖先の依存先を検査しないため、着手済みの Issue を移し入れた場合、その Issue は移動先の Group の依存先を待たずに着手済みのまま残る。変更後の通常完了経路が循環する所属変更も拒否する。例えば A が B の完了に依存している場合、B を A の配下へ移すことはできない。

Group の移動は、その Group の親だけを付け替える。配下の所属・lifecycle・dependency は保持され、部分木全体が移る。終了した Group でも、自身の親が終了していなければ、その内部構成を変えずに移せる。これは終了した Issue の移動と同じ制約である。

新しい Entity は Group 内または Group 外へ登録する。所属先は終了していない Group で、採用済みである必要はない。提案の記録は `Undecided`、採用済みの仕事の登録は `NotStarted` とする。登録操作は与えられた採否を反映し、採用判断そのものは代行しない。

どちらの経路も、登録だけでは `InProgress` にならない。`NotStarted` で登録した後も、`Start` には祖先と dependency の前提を要求する。最終確認待ちの Group に不足していた Issue を追加した場合、どちらの経路でも未終了の子が増えるため、その Group は再び `Complete` できなくなる。

## 種類の変換

種類（Issue・Group）は Entity の現在値であり、変換で変わる。保存値が `Undecided`・`NotStarted` の Issue は Group に、保存値が `Undecided`・`NotStarted` で子のない Group は Issue に変換できる。`InProgress` の Issue は先に `Release` する。終了した Entity と子を持つ Group は変換できない。

変換は lifecycle・所属・dependency・文面・label・再浮上条件・Note・ID を変えない。変換した Entity を参照する所属と dependency はそのまま残る。Issue から変換した Group は子を持たないため、実効値は保存値と同じになる。

変換は lifecycle 遷移ではないが記録として残り、変換前後の種類が読める。lifecycle 遷移の記録の妥当性は、その遷移の時点の種類（直前の記録の現在値の種類。登録時の種類を、その遷移より前の変換の記録で進めたもの）の規則で判定し、Entity の現在の種類では判定しない。遷移は種類を変えないため、遷移の記録が持つ操作後の種類がその時点の種類でもある。Issue として `Start`・`Release` してから Group に変換した履歴と、Group として `NotStarted` から `Complete` し `Reopen` してから Issue に変換した履歴は、この判定で妥当になる。現在の種類で全記録を判定すると、Group が `Start` を持たず、Issue が `NotStarted` から完了しないため不正になる。`axon log` は変換を lifecycle 遷移と区別し、変換前後の種類とともに示す（[CLIと表示の契約](cli.md#noteと履歴)）。

## dependency

依存先がすべて `Completed` になるまで、依存元は `Complete` できず、Issue は `Start` できない。依存先の `Cancelled` は前提を満たさない。依存先が `Cancelled` になっても依存元の `Cancel` を強制せず、依存関係の見直しや再検討は明示操作に残す。

`InProgress` の Entity にも未完了の依存先を追加でき、追加は lifecycle を変えない。着手の前提は着手時に検査し、着手中に再検査しない。追加した依存先はその Entity 自身の `Complete` の前提として完了時に検査されるため、その Entity が未完了の依存先を残したまま完了することはない。Group に追加した依存先が配下の `Complete` の前提にならないことは [親子のlifecycle](#親子のlifecycle) に定める。着手中に見つかった前提を記録するために `Release` を挟ませない。

`Completed` の Entity 自身の dependency は固定する。`Reopen` で `NotStarted` へ戻せば編集できる。`Completed` の Entity を、別の Entity が前提として参照することは許す。未完了の Entity の前提は、判断に応じて追加・削除する。所属変更と変換は dependency を保持し、Group をまたぐ依存と、所属なしの Entity への依存を許す。

Group から Issue への依存を許す。Issue・Group から別の Group への依存も許し、依存先の Group 自身が最終確認を経て `Completed` になるまで待つ。配下がすべて終了しただけでは、依存先の完了の前提を満たさない。依存先のGroup全体の成果の最終確認を待つことが、Group への依存の意味だからである。

自己依存と、包含を合わせた通常完了経路の循環を、追加時に拒否する。親・祖先と子孫の間の明示的な dependency はどちら向きも禁止される。兄弟や別の Group に属する Entity どうしでも、他の包含・依存を経由して循環する場合は拒否する。終了した Entity も構造のグラフから除外しない。拒否された操作は無効となり、状態・所属・dependency を自動調整しない。

## 候補集合

候補集合は、保存した状態・包含・dependency と、その時点の再浮上条件の成立状況から導出する。評価結果は保存しない。

| 集合 | 条件 |
| --- | --- |
| 判断候補 | 保存値が `Undecided` の Issue・Group で、自身とすべての祖先が浮上している。祖先の採用と依存先の完了は要求しない |
| 浮上した未着手 | 保存値が `NotStarted` の Issue・Group で、自身とすべての祖先が浮上している。祖先の採用と依存先の完了は要求しない |
| 着手可能 | `Start` の前提を満たす Issue |
| 着手候補 | 着手可能で、自身とすべての祖先が浮上している Issue |
| 着手中 | `InProgress` の Issue と、実効値が `InProgress` の Group。自身・祖先の浮上状態や、追加された未完了の依存に関係なく含む |

`axon proposals` は判断候補を返す。`axon tasks` は着手候補に限定せず、浮上した未着手と着手中を合わせた平らな一覧を返すため、依存先の完了待ちや祖先の採用待ちも含む。実効値が `InProgress` の Group は浮上した未着手と着手中の両方に当たりうるが、一覧には一行だけ出る。一覧の行の状況と詰まっている理由は [候補と外部条件](candidates.md#一覧の行と状況)、その表記は [CLIと表示の契約](cli.md) に定める。

Group は `Start` を持たないため、着手可能・着手候補にならない。Group に置いた dependency と祖先の採否は、配下の Issue の着手可能性を通じて効く。

判断候補は採否を検討するための集合であり、候補への出入りで採否や進行状態を変えない。dependency は着手・完了の前提であり、採否を先に決めることを妨げない。

再浮上は候補の表示と、`axon show` が示す状況・詰まっている理由だけを制御し、明示的な採否判断、`Start` や `Complete`、所属・依存の変更の可否には加えない。ID を明示した操作は、浮上していない Entity にも候補と同じ前提で行える。条件が壊れていても、条件の修復や明示操作ができなくなることはない。

終了した Entity は条件を評価せず、浮上しない。祖先のいずれかが非浮上なら、その子孫を判断候補・着手候補から外すが、子の状態や条件は変えない。再浮上条件の成立状況が変化しても、保存する lifecycle・所属・dependency は変わらない。終了している間にも外界は変化するが、その Entity の条件を評価するという意味ではない。

## label

label は、Entity が表す仕事の種類を固定集合の値で分類する属性である。Issue・Group とも必ず一つの label を持ち、未設定の状態はない。集合は次の 7 値で、これ以外の値は拒否する。集合を設定で変える経路はない。

| label | 意味 |
| --- | --- |
| `bug` | 期待と異なる振る舞いを直す |
| `feat` | 新しい振る舞いや機能を加える |
| `chore` | 依存の更新、CI、設定など、振る舞いを変えない保守 |
| `docs` | 文書・help・skill を整備する |
| `test` | テストを追加・整理する |
| `refactor` | 振る舞いを変えずに構造を整理する |
| `spike` | 決めるために調べる（要検討の事項、仕様・計画・方針の検討） |

Group の label は、そのGroup全体の仕事の主な種類を表す。配下の label から導出せず、配下と一致することも要求しない。

label は登録時に必ず与え、後から別の値へ変更できる。解除はない。編集できる状態は title・description と同じで、`Undecided`・`NotStarted`・`InProgress` では状態を変えずに変更でき、終了後は固定する（[情報の役割と編集範囲](#情報の役割と編集範囲)）。変更は記録として残る。

label は分類であり、優先度ではない。lifecycle 遷移の前提、包含と dependency の制約、候補集合、一覧の状況、再浮上条件の評価のいずれにも影響しない。種類の変換は label を変えない。一覧での表示と絞り込みは [CLIと表示の契約](cli.md#一覧) に定める。理由は [label を固定集合の必須属性にした理由](../design/decisions.md#label-を固定集合の必須属性にした理由) に記す。

## 文面・Note・状態変更履歴

### 情報の役割と編集範囲

Issue・Group とも、title・description・label は現在の内容を保持する。`Undecided`・`NotStarted`・`InProgress` は状態を変えずに編集でき、終了後は固定する。`Cancelled` の Entity は `Reconsider` で `Undecided` へ、`Completed` の Entity は `Reopen` で `NotStarted` へ戻せば編集できる。完了した結果を保ったままの訂正・補足は Note に追記する。この規則は title・description・label の規則であり、所属・dependency は上に定めた変更条件に従う。

文面の編集は編集後の現在値を持つ記録として残り、`axon log` で編集があったことが読める。採用した時点の文面を別に保存することはせず、Axon は編集の差分を示さない。着手中の計画の具体化・修正のために、作業の解放や採用撤回を要求しない。完了条件をエージェントが都合よく緩和・削除することへの対処は、skill による運用とセッションログでの確認に任せる。操作に対応する判断理由は任意の reason として操作と一体で残し、必要な詳しい補足は Note に書く。本文は現在の作業定義の正本なので、reason や Note に書いた有効な制約や判断も本文へ反映する。入力範囲・検証と表示は [CLI契約](cli.md) に従う。

Note は、調査結果・作業結果・申し送り・訂正など、状態変更と独立した情報を残す。どの状態でも追加でき、追加しても状態は変えない。追記専用とし、既存の Note は編集・削除せず、訂正は新しい Note とする。同じ内容の追記も、それぞれ別の記録として残す。Note は互いに因果を持たない集合で、表示順は日時、同じ日時なら記録 ID で決める。

状態変更履歴は Note と別の役割を持ち、成功した lifecycle 遷移ごとに、操作・変更後の状態・日時・任意の理由を持つ記録を自動で残す。変更前の状態は直前の記録の現在値として読める。状態変更と記録の追加は一体で確定し、片方だけを保存しない。拒否された遷移は記録を作らない。記録は不変で、後からの理由の補足・訂正は Note に追記する。文面編集・label の設定・関係の変更・条件の設定・種類の変換・衝突の解決も同じ記録の列に残るが、lifecycle 遷移とは区別して表示する。記録が持つ項目と、記録の集合からの現在値の導出は [保存と統合の契約](storage.md#記録) に定める。

Note と Note 以外の記録は別々の入口で表示する。保存上の区別は、実際に起きた操作を自動記録することと、自由な補足を追加することの区別である。

### 記録者情報とエージェント連携

Note・状態変更履歴には、任意の記録者情報 `{ actor: string, data: object }` を添えられる。`actor` は `codex`・`claude`・人間など記録元を示す文字列で、固定の列挙型にはしない。`data` は連携側が用途に応じた情報を入れるオブジェクトとし、例えば `{"actor":"codex","data":{"session_id":"..."}}` のように元の作業をたどる手掛かりを保持する。通常表示は actor を中心とし、詳細確認時には data も取得できる。

記録者情報は、連携側が環境変数などから取得できる範囲を自動で埋める。利用者が操作のたびに session ID 付きでコマンドを呼ぶことは要求しない。actor が分かり session ID が取れない場合は取得できた範囲を残し、記録元も分からない場合は記録者情報を省略できる。情報が取得できないことだけを理由に、Note 追加や状態変更を失敗させない。保存した記録者情報（`data` を含む）は記録とともに保持し、Git で追跡すれば他の worktree とも共有される。

エージェント固有の検出・メタデータ構築は、状態モデルや記録の保存処理から分離し、Axon core に依存しない Rust の crate が担う。読み取る環境変数と検出の優先順位は [記録者連携](../development/lifecycle-recorder.md) に定める。Axon 本体は受け取った情報を保持・表示し、エージェントごとの必須項目や session の探索・生存確認・ログ解析は持たない。メタデータは記録時点の手掛かりであり、ログの存続やセッションの再開を保証しない。

### 予約状態と作業所有者を独立して持たない

複数のエージェント・セッションによる別々の Issue の並行作業は想定するが、独立した予約状態や作業所有者は持たない。重複着手は、現在の状態と既存の前提を確認して `NotStarted` から `InProgress` へ原子的に更新することで防ぐ。先に着手された同じ Entity への二度目の `Start` は拒否する。

記録者情報は履歴をたどるために使い、`Complete`・`Release` などの操作権限や排他制御には使わない。`InProgress` の Issue の現在値は着手した actor を含むが、これは Git で分岐した保存先を統合したときに並行した `Start` を衝突として見せるためのもので（[現在値の導出](storage.md#現在値の導出)）、操作の権限には使わない。現在の作業の経緯を調べる場合も履歴を参照し、履歴と別に現在の作業者の情報を持たない。自動的な解放や、作業者がいなくなったことを理由とする自動的な状態変更も行わない。
