# lifecycleと構造の契約

この文書は、Issue・Group が持つ lifecycle の状態と遷移、再浮上条件、計画としての包含、dependency、候補集合、文面・Note・状態変更履歴の契約を定める。候補一覧の評価順と外部コマンドの実行は [候補と外部条件](candidates.md)、識別子・引数・表示・保存結果は [CLIと表示の契約](cli.md)、計画全体の一括編集は [Declaration](declaration.md)、保存先と統合は [保存と統合の契約](storage.md) に定める。

対応する実行可能なモデルは [`spec/lifecycle_rules.qnt`](../../spec/lifecycle_rules.qnt)、[`spec/issue_lifecycle.qnt`](../../spec/issue_lifecycle.qnt)、[`spec/group_lifecycle.qnt`](../../spec/group_lifecycle.qnt)、[`spec/lifecycle_reachability.qnt`](../../spec/lifecycle_reachability.qnt)、[`spec/candidate_evaluation.qnt`](../../spec/candidate_evaluation.qnt)、[`spec/lifecycle_information.qnt`](../../spec/lifecycle_information.qnt) にある。各モデルの対象範囲・検証する性質・再現手順は [モデルの読み方](../../spec/README.md) を参照する。

## 状態

Issue・Group は一つの lifecycle を持つ。状態の種類と基本遷移は Issue と Group で共有する。

| 状態 | 意味 |
| --- | --- |
| `Undecided` | 未判断。記録済みの提案 |
| `NotStarted` | 採用済み・未着手 |
| `InProgress` | 着手中 |
| `Completed` | 完了 |
| `Cancelled` | 取りやめ |

`Completed` と `Cancelled` を終了とする。登録直後は `Undecided` または `NotStarted` から始まる。`Undecided` は記録済みの提案であり、実施の約束を意味しない。`Accept` によって `NotStarted` へ進む。

採否を決めるためのまとまった調査は、別の Issue として採用・着手する。元の改善案は `Undecided` のままにできる。この使い分けのために状態を増やさず、複数 Entity の関係は包含と dependency で扱う。

## 遷移

| 操作 | 遷移元 | 遷移先 |
| --- | --- | --- |
| `Accept`：採用 | `Undecided` | `NotStarted` |
| `Withdraw`：採用撤回 | `NotStarted` | `Undecided` |
| `Start`：着手 | `NotStarted` | `InProgress` |
| `Release`：作業を解放 | `InProgress` | `NotStarted` |
| `Complete`：完了 | `InProgress` | `Completed` |
| `Cancel`：見送り・取りやめ | `Undecided`・`NotStarted`・`InProgress` | `Cancelled` |
| `Reconsider`：再検討 | `Cancelled` | `Undecided` |

`Completed` は戻さない。`Cancelled` は `Reconsider` できるが、再び着手するには `Accept` が必要になる。`InProgress` から採用を撤回する場合は `Release`、`Withdraw` の順に操作する。完了後の追加作業は新しい Issue で扱う。

各操作は原子的に実行され、前提を満たさない操作は無効となる。`Completed` ではすべての lifecycle 操作が無効になる。再浮上条件の成立状況はその後も変化するが、`Completed` を取り消さない。

## 再浮上条件

再浮上条件は、Entity ごとに未設定または一つの外部コマンドとして保持する。条件の種類と評価の契約は [候補と外部条件](candidates.md) に定める。

`Undecided`・`NotStarted`・`InProgress` を評価対象とし、`Cancelled`・`Completed` では条件を評価しない。評価しなかったことと不成立は区別し、評価対象外の Entity は条件の内容にかかわらず浮上しない。判定は保存状態を書き換えない。

「後で考える」は `Undecided` のまま条件で浮上を制御し、「取りやめ」は `Reconsider` するまで浮上対象外にする。`Cancelled` から `Undecided` へ戻すと、同じ条件が再び評価対象になる。条件が成立しているならその時点で浮上し、不成立なら `Undecided` のまま浮上しない。

## 計画と包含

Issue・Group とも所属は最大一つで、所属なしも許す。Group も最大一つの親 Group を持ち、Issue と子 Group を同じ階層に置ける。Group の自己包含と、子孫の下への移動による循環を禁止する。所属変更は既存の親子の制約を壊さない限り許可し、lifecycle を変えない。

終了した Group への追加と、そこからの取り外しは不可とする。`Cancelled` の Group は `Reconsider` で `Undecided` へ戻せば構成を変更できる。この終了時の構成固定と Group の再検討は暫定の判断とし、運用上の負担が分かれば見直す。

終了した Group の配下の lifecycle も固定する。`Completed` の Group 配下にある `Cancelled` の Issue は `Reconsider` も取り外しもできない。完了した計画から独立して見直す仕事は、新しい Issue として扱う。理由は [終了した Group の構成と配下の状態を固定する理由](../design/decisions.md#終了した-group-の構成と配下の状態を固定する理由) に記す。

### 親子のlifecycle

子の `Start` には、子自身が `NotStarted` であることと親の `InProgress` を要求する。`InProgress` の子があれば親を `Release` できない。Group の `Complete`・`Cancel` には直属の Issue・子 Group がすべて終了している必要があり、`Undecided` の子もその妨げになる。子 Group の `Start` にも親の `InProgress` を要求するため、`InProgress` の Entity の祖先はすべて `InProgress` になる。各階層で明示的に着手し、子への `Start` で親を自動変更しない。

Group の `Complete` にはさらに、その計画全体の最終確認が通ったという入力を要求する。画面単位の子 Group にも、機能全体の親 Group にも、それぞれ独立した最終確認がある。Group に対する `axon complete ID` の実行自体を最終確認済みの明示入力とし、別のレビュー済み状態や必須フラグを設けない。確認手順は呼び出し側の skill・運用で扱う。空の Group でもこの確認を省略しない。子の終了だけで親を自動的に終了しない。

着手・完了・取りやめが要求する前提は次のとおりとする。

| 操作 | 包含からの前提 | dependency からの前提 |
| --- | --- | --- |
| `Start` | 親があれば、現在 `InProgress` である | 全依存先が `Completed` |
| `Complete` | Group なら、直属の子が全員終了している | 全依存先が `Completed` |
| `Cancel` | Group なら、直属の子が全員終了している | 要求しない |

子の終了は `Completed`・`Cancelled` のどちらでも満たす。Group の最終確認と、終了した親の配下を変更しない制約は、この前提に加えて適用する。親の前提は過去の着手履歴ではなく現在の `InProgress` を要求する。

循環の検査には「全員が通常どおり完了する経路」を使う。各 Entity の着手から完了への辺、親の着手から子の着手への辺、子の完了から親の完了への辺、依存先の完了から依存元の着手・完了への辺を導出する。`Cancel` による回避はこの経路に加えない。着手の節点を縮約すると、各 Entity の完了に先行するものは次の集合になる。

```text
完了の前提 = 直属の子 ∪ 自身の依存先 ∪ 全祖先の依存先
```

着手の前提を親へさかのぼると祖先の依存先へ到達し、子の終了は通常完了経路では子の完了に対応する。この縮約で得たグラフから、前提の残っていない節点を順に除去し、全節点を除去できるかを検査する。これは構造の循環判定用の導出であり、実際の操作可否には上表の現在状態を使う。保存する関係は包含と明示的な dependency だけである。

### 所属変更と新規登録

`InProgress` の Issue・Group は、所属なしにするか、`InProgress` の別 Group へ移せる。移動元・移動先に Group がある場合は、どちらも終了していないことを検査する。移動のためだけに `Release` と `Start` を挟む必要はなく、移動によって採否や進行の状態は変わらない。変更後の通常完了経路が循環する所属変更も拒否する。例えば A が B の完了に依存している場合、B を A の配下へ移すことはできない。

Group の移動は、その Group の親だけを付け替える。配下の所属・lifecycle・dependency は保持され、部分木全体が移る。終了した Group でも、自身の親が終了していなければ、その内部構成を変えずに移せる。これは終了した Issue の移動と同じ制約である。

新しい Entity は Group 内または Group 外へ登録する。提案の記録は `Undecided`、採用済みの仕事の登録は `NotStarted` とする。登録操作は与えられた採否を反映し、採用判断そのものは代行しない。

どちらの経路も、登録だけでは `InProgress` にならない。`NotStarted` で登録した後も、`Start` には親 Group と dependency の前提を要求する。最終確認待ちの Group に不足していた Issue を追加した場合、どちらの経路でも未終了の子が増えるため、その Group は再び `Complete` できなくなる。

## dependency

依存先がすべて `Completed` になるまで、依存元は `Start`・`Complete` できない。依存先の `Cancelled` は前提を満たさない。依存先が `Cancelled` になっても依存元の `Cancel` を強制せず、依存関係の見直しや再検討は明示操作に残す。`InProgress` でも未完了の依存先を追加でき、追加は lifecycle を変えない。この着手後の追加は暫定の判断とする。

`Completed` の Entity 自身の dependency は固定する。`Completed` の Entity を、別の Entity が前提として参照することは許す。未完了の Entity の前提は、判断に応じて追加・削除する。所属変更は dependency を保持し、Group をまたぐ依存と、所属なしの Entity への依存を許す。

Group から Issue への依存を許す。Issue・Group から別の Group への依存も暫定の判断として許し、依存先の Group 自身が最終確認を経て `Completed` になるまで待つ。配下がすべて終了しただけでは、依存先の完了の前提を満たさない。

自己依存と、包含を合わせた通常完了経路の循環を、追加時に拒否する。親・祖先と子孫の間の明示的な dependency はどちら向きも禁止される。兄弟や別の Group に属する Entity どうしでも、他の包含・依存を経由して循環する場合は拒否する。終了した Entity も構造のグラフから除外しない。拒否された操作は無効となり、状態・所属・dependency を自動調整しない。

## 候補集合

候補集合は、保存した状態・包含・dependency と、その時点の再浮上条件の成立状況から導出する。評価結果は保存しない。

| 集合 | 条件 |
| --- | --- |
| 判断候補 | `Undecided` で、自身とすべての祖先が浮上している。親の `InProgress` と依存先の完了は要求しない |
| 浮上した未着手 | `NotStarted` で、自身とすべての祖先が浮上している。親の `InProgress` と依存先の完了は要求しない |
| 着手可能 | `NotStarted`、親があればその親が `InProgress`、全依存先が `Completed` |
| 着手候補 | 着手可能で、自身とすべての祖先が浮上している |
| 着手中 | `InProgress`。自身・祖先の浮上状態や、追加された未完了の依存に関係なく含む |

`axon proposals` は判断候補を返す。`axon tasks` は着手候補に限定せず、浮上した未着手と着手中を合わせて返すため、依存先の完了待ちや親の着手待ちも含む。着手可能かどうかは一覧の状況として表示し、その表記は [CLIと表示の契約](cli.md) に定める。

判断候補は採否を検討するための集合であり、候補への出入りで採否や進行状態を変えない。dependency は着手・完了の前提であり、採否を先に決めることを妨げない。Group 自身も上表と同じ条件で着手可能・着手候補になり、`InProgress` の Group は非浮上でも着手中に含む。

再浮上は候補の表示だけを制御し、明示的な採否判断、`Start` や `Complete`、所属・依存の変更の可否には加えない。この表示と明示操作の分離は暫定の判断とする。

終了した Entity は条件を評価せず、浮上しない。祖先のいずれかが非浮上なら、その子孫を判断候補・着手候補から外すが、子の状態や条件は変えない。再浮上条件の成立状況が変化しても、保存する lifecycle・所属・dependency は変わらない。終了している間にも外界は変化するが、その Entity の条件を評価するという意味ではない。

## 文面・Note・状態変更履歴

### 情報の役割と編集範囲

Issue・Group とも、title・description は現在の内容を保持する。`Undecided`・`NotStarted`・`InProgress` は状態を変えずに編集でき、終了後は固定する。`Cancelled` の Entity は `Reconsider` で `Undecided` へ戻せば編集できる。`Completed` には再開経路がないため、後からの訂正・補足は Note に追記する。この規則は title・description の規則であり、所属・dependency は上に定めた変更条件に従う。

文面の編集履歴は持たず、採用した時点の文面も保存しない。着手中の計画の具体化・修正のために、作業の解放や採用撤回を要求しない。完了条件をエージェントが都合よく緩和・削除することへの対処は、skill による運用とセッションログでの確認に任せる。Axon 単独では、完了前の文面の書き換えや、採用時からの差分を検証できない。残したい変更理由は Note で補足する。

Note は、調査結果・作業結果・申し送り・訂正など、状態変更と独立した情報を残す。どの状態でも追加でき、追加しても状態は変えない。追記専用とし、既存の Note は編集・削除せず、訂正は新しい Note とする。同じ内容の追記も、それぞれ別の記録として残す。

状態変更履歴は Note と別の役割を持ち、成功した lifecycle 遷移ごとに、変更前の状態・変更後の状態・日時・任意の理由を自動記録する。状態変更と履歴追加は一体で確定し、片方だけを保存しない。拒否された遷移は成功履歴を作らない。履歴も既存の記録を保持し、後からの理由の補足・訂正は Note に追記する。文面編集や Note 追加を lifecycle 遷移として履歴へ混ぜず、状態変更時に文面の全文も保存しない。

Note と状態変更履歴は別々の入口で表示する。保存上の区別は、実際に起きた遷移を自動記録することと、自由な補足を追加することの区別である。

### 記録者情報とエージェント連携

Note・状態変更履歴には、任意の記録者情報 `{ actor: string, data: object }` を添えられる。`actor` は `codex`・`claude`・人間など記録元を示す文字列で、固定の列挙型にはしない。`data` は連携側が用途に応じた情報を入れるオブジェクトとし、例えば `{"actor":"codex","data":{"session_id":"..."}}` のように元の作業をたどる手掛かりを保持する。通常表示は actor を中心とし、詳細確認時には data も取得できる。

記録者情報は、連携側が環境変数などから取得できる範囲を自動で埋める。利用者が操作のたびに session ID 付きでコマンドを呼ぶことは要求しない。actor が分かり session ID が取れない場合は取得できた範囲を残し、記録元も分からない場合は記録者情報を省略できる。情報が取得できないことだけを理由に、Note 追加や状態変更を失敗させない。

エージェント固有の検出・メタデータ構築は、状態モデルや記録の保存処理から分離し、Axon core に依存しない Rust の crate が担う。読み取る環境変数と検出の優先順位は [記録者連携](../development/lifecycle-recorder.md) に定める。Axon 本体は受け取った情報を保持・表示し、エージェントごとの必須項目や session の探索・生存確認・ログ解析は持たない。メタデータは記録時点の手掛かりであり、ログの存続やセッションの再開を保証しない。

### 予約状態と作業所有者を独立して持たない

複数のエージェント・セッションによる別々の Issue の並行作業は想定するが、独立した予約状態や作業所有者は持たない。重複着手は、現在の状態と既存の前提を確認して `NotStarted` から `InProgress` へ原子的に更新することで防ぐ。先に着手された同じ Entity への二度目の `Start` は拒否する。

記録者情報は履歴をたどるために使い、`Complete`・`Release` などの操作権限や排他制御には使わない。現在の作業の経緯を調べる場合も履歴を参照し、履歴と別に現在の作業者の情報を持たない。自動的な解放や、作業者がいなくなったことを理由とする自動的な状態変更も行わない。
