# Quintモデル

Axon の lifecycle、包含と dependency、Group の実効 lifecycle、候補一覧の評価、文面・Note・状態変更履歴の意味論を、実行可能な Quint のモデルとして置きます。状態・遷移・包含・候補集合・情報の契約そのものは [lifecycle](../docs/reference/lifecycle.md) と [候補と外部条件](../docs/reference/candidates.md) が定義し、ここでは各モデルが何を対象にし、何をどう検査するかを案内します。保存層（記録の集合と Git 統合）のモデルは [redesign](redesign/README.md) にあります。

## モデル一覧

| ファイル | 対象 | 対象外 |
| --- | --- | --- |
| [`lifecycle_rules.qnt`](lifecycle_rules.qnt) | Issue と Group が共有する状態5種類・操作8種類と、種類ごとの基本遷移の前提・遷移先 | 状態変数と遷移の実行、包含、dependency、実効 lifecycle、条件 |
| [`issue_lifecycle.qnt`](issue_lifecycle.qnt) | 単独 Issue の lifecycle（`Reopen` を含む）と、抽象化した再浮上条件による浮上の導出 | 包含、dependency、Group、登録操作、declaration による編集、履歴の保存、記録者による権限判定、条件の種類と設定操作、一覧、浮上と着手許可の接続、CLI、永続化 |
| [`group_lifecycle.qnt`](group_lifecycle.qnt) | Issue と Group の包含、所属変更、新規登録、種類の変換、dependency、Group の実効 lifecycle、`Reopen`、判断候補・着手候補・`axon tasks` の一覧の行と状況の導出 | 文面と記録、条件の種類・設定と評価失敗、CLI と表示、ID 発行、永続化、記録の集合と統合 |
| [`lifecycle_reachability.qnt`](lifecycle_reachability.qnt) | `group_lifecycle` の到達性を補う4つの探索入口 | 許可条件・状態更新・検査する性質（`group_lifecycle` のものをそのまま使う） |
| [`candidate_evaluation.qnt`](candidate_evaluation.qnt) | `axon proposals`・`axon tasks` の1回の取得における評価範囲、評価回数、結果の共有、判定失敗 | 行の状況の導出、実コマンドと shell、作業ディレクトリ、終了コードの解釈、timeout と中断、出力の上限と診断、並行実行 |
| [`lifecycle_information.qnt`](lifecycle_information.qnt) | title・description の編集範囲、Note の追記、状態変更履歴と状態の一致（Issue 1件と Group 1件） | 包含・dependency・実効 lifecycle・最終確認による追加の制約、日時・理由・記録者情報、表示順、公開 ID、永続化 |

Quint の `import` の向きは `lifecycle_rules` を起点にした一方向です。`issue_lifecycle`・`group_lifecycle`・`lifecycle_information` がそれぞれ `lifecycle_rules` を取り込み、`lifecycle_reachability` と `candidate_evaluation` が `group_lifecycle` を取り込みます。`issue_lifecycle` と `lifecycle_information` は互いにも `group_lifecycle` にも依存しません。

## モデル化と実装検証の分担

すべての設計判断を Quint の状態に追加することはしません。状態・関係・操作の相互作用に不確実性がある部分をモデル化し、実行環境や付随情報の取得・保存は設計判断として記述して実装側で検証します。モデルへ残す検証専用の観測値も、対象の性質を確かめるために必要な範囲に留めます。

| 対象 | 扱いと理由 |
| --- | --- |
| lifecycle、包含、dependency | 遷移の前提・循環・終了条件が相互作用するため維持する |
| Group の実効 lifecycle と一覧の状況 | 保存値から導出する値で、操作の前提や候補集合との相互作用を検査するため `group_lifecycle` で維持する。表示の形式は実装側で検証する |
| 候補一覧の評価範囲・結果共有・失敗 | 評価省略が候補の取りこぼしや失敗の見落としにつながらないかを検査するため維持する |
| 条件コマンドの不透明な識別子 | 条件の置き換えと保持を区別するため維持し、実コマンドや環境はモデル化しない |
| 文面・Note・状態変更履歴 | 編集可能状態、追記専用性、履歴と状態の一致を検査するため維持する |
| 履歴の日時・任意の理由 | 遷移に影響せず値の保持を調べるだけなので、探索には含めず実装検証で扱う |
| 記録者情報・エージェント連携・外部コマンドの実行環境 | 操作の可否に使わない付随情報や実行詳細として、設計判断と実装検証で扱う |
| 重複着手 | モデル上は原子的な状態遷移で扱い、実際の競合は保存処理の並行テストで確認する |
| 記録の集合、衝突、構造の違反と修復 | 保存層の統合として [redesign](redesign/README.md) の統合モデルが扱う |

同一時刻の履歴が上書きされないこと、日時・理由・記録者情報の保持、自動取得と取得不能時の継続、Note の同内容追記の区別、保存失敗時の原子性は実装側の検証対象です。独立した予約状態やエージェント別の状態は持ちません。各モデルはそれぞれの検証範囲を維持し、変更に関係のある検査を選んで実行します。検査の分担と実装側の入口は [検証方針](../docs/development/verification.md) にあります。

## モデルが表す規則

コアの意味論のうち、モデルが前提として置く規則です。実装と契約文書はこの規則に合わせます。

- Issue は `Accept`・`Withdraw`・`Start`・`Release`・`Complete`・`Cancel`・`Reconsider`・`Reopen` の8操作を持つ。`Reopen` は `Completed` から `NotStarted` へ戻し、`Completed` から抜ける唯一の経路になる。
- Group は `Start`・`Release` を持たず、保存される lifecycle（stored）は `InProgress` にならない。実効 lifecycle は「stored が `NotStarted` で、直属の子に実効 `InProgress` か `Completed` があれば `InProgress`」と導出し、`NotStarted` の子 Group を通じて上へ伝わる。
- Group の `Complete` は stored が `NotStarted` のとき、直属の子がすべて終了し、最終確認が通れば許す。空の Group と子がすべて `Cancelled` の Group もこの経路で完了する。Group の `Cancel` は `Undecided`・`NotStarted` からだけ行う。
- Issue の `Start` は、全祖先が採用済み（stored が `NotStarted`）で、自身と全祖先の dependency がすべて `Completed` であることを要求する。親が実効 `InProgress` であることは要求しない。
- 種類（kind）は現在値で、`Undecided`・`NotStarted` の Issue は Group に、子のない `Undecided`・`NotStarted` の Group は Issue に変換できる。変換は lifecycle・所属・dependency を変えない。`InProgress` の Issue は先に `Release` する。
- 終了した Group の構成固定は維持し、`Completed` の Group は `Reopen`、`Cancelled` の Group は `Reconsider` で戻してから構成を変える。
- `InProgress` の Entity にも未完了の dependency を追加できる（完了時の検査で止まる）。Issue・Group から Group への dependency は依存先 Group の `Complete` まで待つ。
- 再浮上条件は候補の表示だけに使い、ID を明示した操作は縛らない。
- `axon tasks` は平らな一覧で、Issue の行は未着手で自身と全祖先が浮上しているものと浮上に関係なく進行中のもの、Group の行は stored が `NotStarted` で浮上しているものと浮上に関係なく実効 `InProgress` のもの。Group の行の状況は配下から `Empty` > `Confirmable` > `Ready` > `InProgress` > `Blocked` の優先順位で導出する。直属の子がない Group は完了できても `Empty` で、次の一手は計画を書くこと（`axon complete` の可否は変えない）。詰まっている理由（未完了の dependency、`Undecided` の子、未終了の子 Group、浮上していない着手可能な子孫、`Undecided` の祖先）は、完了できず、着手候補の子孫も着手中（stored が `InProgress`）の Issue の子孫もない Group の行に示し、`Blocked` の行に限らず、完了済みの子孫はあるが着手中の子孫がない実効 `InProgress` の行と、完了できない `Empty` の行にも付く。`Confirmable` の行、`Ready` の行、完了できる `Empty` の行は次の一手が状況から分かるため理由を示さない。

### 反例と review から足した規則

書き始めた時点の案では足りず、`group_lifecycle` の反例と独立 review から足した規則です。いずれも invariant を弱めずに規則側を直しました。

- **守る性質は「実効 `InProgress` の Group の全祖先が採用済み」。** `InProgress` の Issue についての同じ性質を Group の実効 lifecycle へ広げたもの。この性質を保つために、Issue・Group の `Complete` と `Reopen`、着手・完了した子孫を持つ Group の `Accept` は全祖先が採用済みであることを要求し、実効が `InProgress` か `Completed` の Entity の移動先は、移動先の Group 自身を含む全祖先が採用済みであることを要求する。Group の `Withdraw` は実効が `NotStarted` のときだけ許す。実効は `NotStarted` の子 Group を通じて上へ伝播するので、孫が着手中なら祖父も `Withdraw` できない（直属の子の保存だけを見ると、子 Group の保存は `InProgress` にならないため、この穴が反例として出た）。
- **`Undecided` の Group の下に完了済みの子孫があることは許す。** 完了済みの子を持つ Group を `Cancel` してから `Reconsider` すると、この状態になる。取りやめた子 Group の下に完了済みの子孫があるときの親の `Withdraw` も同じ理由で禁じない。その Group を再び `Accept` するには全祖先が採用済みであることが要る。
- **`Blocked` は残余として定義する。** Group の行の状況は、`Empty`（直属の子がない）、`Confirmable`、`Ready`、`InProgress`、`Blocked`（子があり、上のどれでもない）の順で決める。当初の「未着手の Issue の子孫はあるが着手候補がない」という定義では、採用直後で子がすべて `Undecided` の Group や、子が空の子 Group しか持たない Group がどの状況にも入らなかった。詰まっている理由は、未完了の dependency、`Undecided` の子、未終了の子 Group、浮上していない着手可能な子孫、`Undecided` の祖先の 5 種類で尽きる。
- **空の Group は `Confirmable` より先に `Empty` にし、詰まっている理由は `Blocked` の行に限らない。** 取り込み後の独立 review で、採用済みで dependency のない空の Group が `Confirmable` になり、完了済みの子孫はあるが着手中の子孫がない Group が理由のない `InProgress` の行になることが分かった。どちらも定義どおりで反例ではないが、空の Group の次の一手は完了ではなく計画を書くことなので `Empty` を先に判定し、`InProgress` の行は「配下の仕事が始まっている」という実効 lifecycle と同じ概念のまま維持して（`Blocked` へ落とす案と複合表示は採らない）、理由の導出を「完了できず、着手候補の子孫も着手中の Issue の子孫もない Group の行」全般へ広げた。`Confirmable` の行と完了できる `Empty` の行は次の一手が状況から分かるため、理由を要求しない。着手中の Issue に後から足した未完了の dependency はその Issue の行の問題として扱い、Group を詰まっているとは見なさない。

規則を直す前の実行で出た反例は、五つの状況で尽くせない Group の行、`Undecided` の祖先の下の実効 `InProgress`（空の Group の完了、完了済み Entity の移動、取りやめ・見送り・再検討・採用の組合せ）、理由のない `Blocked`、近傍が狭すぎた行き止まり検査の偽陽性の 4 件。独立 review からは、空虚な invariant と名前とずれた witness を取り込みました。

### モデリングで見えた edge case

- **再開は依存元と祖先の側から順に行う。** `Completed` の依存元がある Entity は `Reopen` できず、依存元を `Reopen` するにはその祖先が採用済みである必要がある。数段の `Reopen` を要する場合がある。行き止まりの検査はこのため、完了済みの依存元とその祖先を近傍に加えている。
- **`Undecided` の祖先の下の採用済み Group は、子があれば `Blocked`、なければ `Empty` として一覧に出て、理由に `Undecided` の祖先が付く。** 祖先そのものは `Undecided` なので一覧に出ない。同じ理由で、`Undecided` の祖先の下の未着手 Issue も浮上していれば `Blocked` の行として出る。
- **実効 `InProgress` の Group は浮上していなくても一覧に出る。** 着手中の仕事を見失わないため、Issue の `InProgress` と同じ扱いにしている。その Group に着手候補の子孫があれば状況は `Ready` になる。

## 各モデルの探索と検証する性質

### `lifecycle_rules`

状態変数を持たない純粋な定義だけの module で、単独では実行しません。`canPerform` は Issue の基本遷移の前提、`canPerformAs` は種類ごとの前提（Group は `Start`・`Release` を持たず、`NotStarted` から完了し、`Undecided`・`NotStarted` からだけ取りやめる）、`applyOperation` は遷移先で、他のモデルがこれらを共有します。

### `issue_lifecycle`

提案として登録した直後の `Undecided` から始め、外部入力は不成立から開始します。`step` は8操作と外部入力の変化を探索候補にし、前提を満たすものだけが実行されます。外部入力は両方向へ変化でき、`Cancelled`・`Completed` の間も変化しますが、条件の評価対象には戻りません。

invariant は、`NotStarted`・`InProgress`・`Completed` には直近の見直しより後の採用が必要なこと、`InProgress` の入口が `NotStarted` からの `Start` に限ること、`Completed` の入口が `InProgress` からの `Complete` に限ること、`Completed` から抜ける経路が `Reopen` だけで `NotStarted` へ戻ること、明示操作の行き止まりがないこと（規則の定義から直接従う構成の確認）、外部入力と明示操作が互いの値を変えないこと、`Cancelled`・`Completed` が評価されず浮上しないこと、評価対象の浮上が入力の成立と一致することを検査します。witness は8操作それぞれの到達に加え、未判断からの見送り・未着手からの取りやめ・進行中からの打ち切りを区別し、`Reopen` 後の再着手、外部入力の両方向の変化、各状態での浮上・非浮上、再検討後と `Reopen` 後の浮上・非浮上を観測します。初期状態だけではどの witness も成立しません。

条件定義を固定しているため、取りやめ時の定義の保存や読み戻しは検証しません。`NotEvaluated` は意味上の評価除外を表すもので、実際に外部コマンドが呼ばれないこと、読み取りコスト、実行失敗と副作用は実装側で検証します。

### `group_lifecycle`

Entity は固定 ID 5件。0 は Group、1 と 2 は 0 の子 Issue、3 は所属なしの Group、4 は未登録の枠で、全員 `Undecided`、条件入力はすべて不成立から始めます。未登録の枠は探索領域を有限にする仕組みであって、新しい lifecycle ではありません。これは探索の開始点であり、製品の再浮上条件の初期値ではありません。`step` は Group の操作、Issue の操作、それ以外（移動、登録、変換、dependency の編集、条件入力の変化）の三枝から選びます。ID は固定集合から選び、未登録 ID の再利用と削除は扱いません。

invariant は、Group の保存が `InProgress` にならないこと、実効 lifecycle が導出の不動点であること、実効 `InProgress` の Group が着手・完了した子孫を持ちその全祖先が採用済みであること、着手・完了の前提、Group の `Completed` への到達が最終確認を伴う `Complete` だけであること、`Completed` から抜けるのが `Reopen` だけで他を変えないこと、包含（所属の妥当性、Issue が子を持たないこと、非循環、終了した Group の子孫がすべて終了し構成が変わらないこと）、各操作の影響範囲、変換が種類だけを変えること、登録が採否を反映し自動で着手しないこと、dependency（参照の妥当性、通常完了経路の非循環、`Completed` の dependency がすべて `Completed` で固定されること、自己依存と祖先・子孫間の依存の不在）、候補（判断候補と着手候補の定義、Group が着手候補にならないこと、判断候補と着手候補・着手中の非重複、条件入力が着手可否を変えないこと）、一覧の状況の定義と優先順位、詰まっている Group（完了できず、着手候補の子孫も着手中の Issue の子孫もない）の行に理由があること、`Blocked` の行、着手中の子孫がない `InProgress` の行、完了できない `Empty` の行が詰まっていること、未終了の各 Entity について自身・祖先・子孫・依存先とその完了済みの依存元の閉包のどこかで操作できることを検査します。前提と更新の定義から直接従う性質は「構成の確認」として file 内で区別しています。

witness は各操作の到達に加えて、`Reopen` 後の再着手、完了済みの子を持つ Group の `Reopen`、`Completed` の依存元による `Reopen` の拒否、空の Group と全子 `Cancelled` の Group の完了、祖先の dependency による着手の阻害と解禁、浮上していない Issue への ID を明示した着手、三階層での実効 `InProgress` の伝播、孫が着手中の祖父の `Withdraw` の拒否、変換と変換後の登録、実効 `InProgress` の Entity の移動、直接・間接の循環の拒否、着手中に足した未完了の dependency による完了の阻止、五つの状況と優先順位、完了できる空の Group の `Empty`、子のある詰まっている行での 5 つの理由、理由が付く `InProgress`・`Blocked`・`Empty` の各行を観測します。理由のない詰まっている Group（`wStalledWithoutListedReason`）と、実効 `InProgress` が `Undecided` の祖先の下にできる状態とそこへ至る経路（`wEffectiveWorkingUnderUnadopted`・`wUnadoptedViaComplete`・`wUnadoptedViaMove`・`wUnadoptedViaAcceptOrReopen`）の 5 つは、規則で塞いだことを 0 trace で確かめる対象として残しています。

行き止まりがないという性質は整理・再判断・`Reopen` の操作も含み、当初の計画どおり必ず完了できることを意味しません。同じ所属の再指定はモデルでは無効な操作として探索から外しますが、CLI では同値の指定は成功した no-op です。前提の循環検査と包含の検査はこの固定範囲を対象とし、四階層以上の木や、より多い Entity を含む具体的なグラフは探索していません。追加できる Entity が1件という探索上の上限があり、新規登録が無効になることは製品が追加件数を制限するという意味ではありません。最終確認が通るかは抽象入力で、確認工程の実装や不合格の理由は扱いません。

### `lifecycle_reachability`

通常探索では、取りやめ・見送り・変換が先行して、`Reopen` を経る経路、祖先の dependency の完了による着手の解禁、一覧で `Ready` と実効 `InProgress` が重なる Group、規則で塞いだ経路の再確認に到達しにくくなります。探索を厚くするより、選ぶ操作を絞った入口から狙うほうが、同じ試行数で桁違いに濃く検査できます。

`group_lifecycle` と同じ `init` と操作を使い、1 step で選ぶ操作だけを絞る4つの入口を持ちます。`workAndReopen` は採用・着手・完了・`Reopen` と移動・登録・dependency の追加を選び、`Reopen` 後の再着手、完了済みの子を持つ Group の `Reopen`、三階層での孫 Issue の着手、`Completed` の依存元による `Reopen` の拒否を調べます。`dependAndStart` は dependency の追加と採用・着手・完了・移動を選び、祖先や自身の依存先の完了で子孫の Issue が着手可能になる瞬間を調べます。`listAndWork` は条件の成立と採用・着手・移動・登録を選び、`Ready` と実効 `InProgress` が重なる Group と、実効 `InProgress` の Group の移動を調べます。`readoptAfterClose` は8つの lifecycle 操作と2つの固定の移動だけを選び、完了済みの子を持つ Group の祖先を取りやめ・見送り・再検討で外したあとの採用・`Reopen` を集中して試し、規則で塞いだ経路が再び開いていないことを確かめます。許可条件・状態更新・到達目標は変えないため、invariant は `group_lifecycle` のものをそのまま指定します。これらの選び方は到達性の確認用であり、製品の workflow を定めるものでも、通常探索の分布を表すものでもありません。

### `candidate_evaluation`

`group_lifecycle` の状態と許可条件を再利用し、一覧の取得を開始・Entity ごとの評価・公開という複数の観測ステップへ具体化します。`queryInit` は `group_lifecycle` の `init` に加えて全 Entity の条件を `Unset` から始め、`queryStep` は取得の各段と、取得外の条件編集・計画変更を混ぜて探索します。`group_lifecycle` の bool 入力と一覧の行の集合は失敗のない場合の意味を定める基準として残り、評価回数・省略・失敗をこのモデルで扱います。`group_lifecycle` の `false` 初期入力は未成立の外部入力から探索を始める指定で、条件未設定の初期値ではありません。このモデルはすべて `Unset` から始め、未設定が成立として扱われることを検査します。未設定の成立判定も観測ステップとして数えますが、外部コマンドを呼ぶという意味ではありません。

評価の対象は、`axon proposals` では stored が `Undecided` の Entity、`axon tasks` では stored が `NotStarted` の Issue と Group で、どちらも終了した祖先の配下を外します。祖先は上から評価し、未成立ならその配下を評価しません。実効 `InProgress` の Group も stored は `NotStarted` なので、自身の浮上判定として評価されます。`InProgress` の Issue はどちらの対象にも入らず、子を持たないので祖先としても評価されず、tasks には評価なしで載ります。行の状況（`Ready`・`Blocked` など）は `group_lifecycle` が導出し、このモデルは行の集合だけを見ます。

成功時には、未評価の入力をすべて成立と仮定しても、すべて未成立と仮定しても、`group_lifecycle` 側の判断候補または一覧の行の集合と一致することを検査します。これにより、評価を省いた入力が候補の欠落を隠していないかを調べます。失敗は成功の集合とは異なる variant で表し、失敗した Entity を保持します。途中で観測した候補は成功結果として公開しません。合わせて invariant で、評価済みの各 Entity が必要な対象かその祖先であること、全祖先の成立後にだけ評価すること、評価回数の上限、同一呼び出し内の結果共有と次回の初期化、条件編集と明示操作が評価処理を呼ばないこと、取得と外部の結果が保存した計画・条件を変えないこと、tasks が進行中の Issue と実効 `InProgress` の Group をすべて含むこと、進行中の Issue を評価しないことを検査します。witness は、成功・空・失敗の各結果、未設定と外部コマンドの評価、条件の設定・訂正・解除と失敗からの回復、成立の持続と次回の未成立化、祖先の未成立による評価の省略と同じ祖先の結果の共有、終了した Entity の評価省略、依存待ちや `Undecided` の祖先を持つ未着手が tasks に出ること、進行中の Group の評価と非浮上のままの表示、未着手の Group の行、`Reopen` した Issue の再表示、評価に失敗した当の Entity への明示的な着手を観測します。

通常探索では `Reopen` した Issue の再表示と、評価に失敗した Entity への着手が、計画変更の順序が長く噛み合ったときにしか成立しません。`workAndList` は Group 0 とその子 Issue 1 への採用・着手・完了・`Reopen` と条件の設定、tasks の取得だけを選ぶ補助の入口で、これらと進行中の Group に関する witness を狙います。invariant は通常探索と同じものを指定します。

一覧取得中の lifecycle・包含・dependency・条件設定は固定し、明示操作は取得の間に行います。これは単一呼び出しを調べるための前提であり、並行更新や snapshot の実装契約を決めるものではありません。外部の結果は Entity ごとの評価時に選ぶため、異なる Entity の条件を同一瞬間に観測する保証も置きません。独立した枝の評価順は非決定的で、最初の失敗で取得を終了します。エラー時にどの枝まで観測済みかは保証しません。実行契約は自然言語で定め、Quint では実行結果を成立・未成立・判定失敗へ抽象化します。shell・作業ディレクトリ・環境変数、終了コードの解釈、timeout と process group の終了、出力の上限と診断、実際に外部コマンドを呼ばないことと呼び出し回数は実装側で検証します。

### `lifecycle_information`

固定2 Entity（0 は Issue、1 は Group）、文面の値3種類で、未判断の登録済み Entity から始めます。文面と Note の内容は不透明な整数で更新と保持を区別し、履歴は変更前後の状態だけを持つ追記列へ抽象化します。日時・任意の理由・記録者情報は設計上の保存項目として残しますが、操作の可否や状態遷移に影響しないため探索変数にしません。登録操作と初期状態の記録は扱いません。状態遷移の前提は `lifecycle_rules` の種類ごとの規則で、Group は `Start`・`Release` を持たず `NotStarted` から完了します。

invariant は、操作が対象以外の Entity を変えないこと、文面の編集が編集可能な状態に限ること、終了後の文面が固定されること、Note が追記専用であること、状態変更と履歴追加が一体で確定すること、履歴を種類ごとの規則で再生すると現在の状態になること、`Completed` から抜けるのが `Reopen` だけで `NotStarted` へ戻ること、種類が変わらないこと、Group が `InProgress` を保存しないことを検査します。witness は8遷移それぞれの到達（`Start`・`Release` は Issue だけ）、Group の `NotStarted` からの完了、採用後・着手中・再検討後・`Reopen` 後の文面編集、各状態での Note 追加、同じ内容の追記が別の記録になることを観測します。基本遷移だけを持つため、このモデルが許す遷移がそのまま実際の Group や依存を持つ Issue で許可されるわけではありません。実時刻の取得、精度、時計補正、公開 ID、actor、記録の表示順、永続化と保存失敗はこのモデルの外です。

## 再現手順

以下は repository root から実行します。`quint run` は bounded random simulation であり、検査の成功は exit status だけでなく、列挙した全 invariant に反例がなく、0 が期待の witness を除く全 witness が出力上1 trace 以上で観測されたことを確認します。通常探索で観測率の低い witness は補助探索が担保するため、`group_lifecycle` と `lifecycle_reachability`、`candidate_evaluation` の `queryStep` と `workAndList` は、それぞれ合わせて1つの検査として扱い、全 witness がいずれかの探索で観測されたことを確認します。

seed は固定しません。`quint run` は seed を渡すと再現のために単一スレッドで実行し、渡さないときだけ CPU 数に応じて並列化します。同じ seed を使い続けても、モデルが変わらない限り同じ経路をなぞるだけで新しい情報は得られないため、実行ごとに異なる経路を探索させます。観測される trace 数は実行ごとに変わります。反例が出た場合は `Use --seed=0x… to reproduce.` が出力されるので、その seed を指定すれば同じ反例を再現できます。`--n-threads` は既定で CPU 数になるため指定しません。

invariant と witness はモデルから定義名を読み取って全件を指定します。

```sh
for f in spec/*.qnt; do quint typecheck "$f"; done
```

```python
from pathlib import Path
import re
import subprocess

def names(path, prefix):
    return re.findall(rf"val ({prefix}\w+)\s*=", Path(path).read_text())

def run(model, samples, steps, invariants, witnesses, *extra):
    subprocess.run(["quint", "run", model, "--invariants", *invariants, "--witnesses", *witnesses,
                    "--max-samples", str(samples), "--max-steps", str(steps),
                    "--backend", "rust", "--verbosity", "1", *extra], check=True)

for model, samples, steps in [("issue_lifecycle", 100000, 80), ("lifecycle_information", 100000, 60)]:
    path = f"spec/{model}.qnt"
    run(path, samples, steps, names(path, "inv"), names(path, "w"))

group = "spec/group_lifecycle.qnt"
invariants, witnesses = names(group, "inv"), names(group, "w")
run(group, 5000, 150, invariants, witnesses)
for step, samples, steps in [("workAndReopen", 300, 100), ("dependAndStart", 300, 100),
                             ("listAndWork", 300, 100), ("readoptAfterClose", 5000, 60)]:
    run("spec/lifecycle_reachability.qnt", samples, steps, invariants, witnesses, "--step", step)

candidate = "spec/candidate_evaluation.qnt"
invariants, witnesses = names(candidate, "inv"), names(candidate, "w")
run(candidate, 30000, 60, invariants, witnesses, "--init", "queryInit", "--step", "queryStep")
run(candidate, 3000, 60, invariants, witnesses, "--init", "queryInit", "--step", "workAndList")
```

`group_lifecycle` の witness のうち `wStalledWithoutListedReason`・`wEffectiveWorkingUnderUnadopted`・`wUnadoptedViaComplete`・`wUnadoptedViaMove`・`wUnadoptedViaAcceptOrReopen` の 5 つは 0 trace が期待で、通常探索と補助探索のいずれでも観測されないことを確認します。

## 検証結果

いずれも bounded random simulation の結果であり、全状態の証明でも、必ず完了することの保証でもありません。どのモデルも、完了への到達を強制する公平性は仮定しません。seed を固定しないため、観測される trace 数は実行ごとに変わります。ここに残すのは実行条件と判定で、trace 数は witness の到達しやすさの目安として添えます。

以下は 2026-09-25、Quint 0.32.0、Rust backend、並列実行、seed 未固定での、一覧の Group の行の規則（`Empty` の優先、詰まっている理由の導出範囲）を直した後の結果です。6モデルの型検査はいずれも成功しました。

- `issue_lifecycle`: 100,000 traces、最大80 steps（約7秒）。9 invariant に反例はなく、23 witness はすべて観測されました。最も少ない完了後の非浮上で37.8%です。
- `group_lifecycle`: 5,000 traces、最大150 steps（約3.5分）。40 invariant に反例はなく、92 witness のうち規則で塞いだ経路と理由のない詰まっている Group を観測する5つが期待どおり0で、残り87をすべて観測しました。最も少ないのは三階層での孫の着手による祖父の実効 `InProgress` で8 traces、次いで祖先の dependency の完了による着手の解禁で9 traces、`Reopen` 後の再着手で13 traces です。完了できる空の Group の `Empty` は3,453、着手中の子孫がない実効 `InProgress` の行に理由が付く状態は254、完了できない `Empty` の行に理由が付く状態は4,320 traces で観測しました。
- `lifecycle_reachability`: 同じ40 invariant を指定して反例なし。`workAndReopen`・`dependAndStart`・`listAndWork` は各300 traces、最大100 steps、`readoptAfterClose` は5,000 traces、最大60 steps。`workAndReopen` は `Reopen` 後の再着手を293、完了済みの子を持つ Group の `Reopen` を62、`Completed` の依存元による `Reopen` の拒否を296 traces で観測しました。`dependAndStart` は祖先の dependency の完了による着手の解禁を46、自身の依存先の完了による解禁を245 traces で観測しました。`listAndWork` は `Ready` と実効 `InProgress` が重なる Group を257、実効 `InProgress` の Group の移動を300、完了できる空の Group の `Empty` を300 traces で観測しました。`readoptAfterClose` では規則で塞いだ経路の witness は0のままで、着手中の子孫がない実効 `InProgress` の行に理由が付く状態を1,193 traces で観測しました。通常探索と補助探索を合わせ、0が期待の5つを除く87 witness がいずれかの探索で1 trace 以上に到達しました。
- `candidate_evaluation`: `queryStep` は30,000 traces、最大60 steps（約1.5分）。15 invariant に反例はなく、29 witness 中28を観測しました。`Reopen` した Issue の再表示は通常探索では未到達で、補助入口が担保します。通常探索で最も少ないのは評価失敗後の着手で4 traces、次いで未成立の結果を持つ進行中の Group の表示で15 traces です。`workAndList` は3,000 traces、最大60 steps で反例なし、`Reopen` した Issue の再表示を35、評価失敗後の着手を38、未成立の結果を持つ進行中の Group の表示を245、進行中の Group の評価を581 traces で観測しました。両方を合わせ、29 witness はすべて1 trace 以上で観測されました。
- `lifecycle_information`: 100,000 traces、最大60 steps（約17秒）。9 invariant に反例はなく、21 witness はすべて観測されました。最も少ない `Release` で4.1%、次いで `Reopen` 後の文面編集で7.6%です。

各モデルの traces 数は、補助探索が担保しない witness が偶然に左右されずに観測される水準を下限とし、そのうえで invariant を叩く厚みを加えて決めています。観測率の低い witness を通常探索の traces 数で拾おうとするより、補助入口を足すほうが確実です。取り込み前の再設計モデルの反例と review の経緯は「反例と review から足した規則」にまとめています。

## 更新するとき

状態・遷移・初期状態・モデルの対象範囲を変えるときは、該当するモデルを更新して再検証し、確定した意味を [lifecycle](../docs/reference/lifecycle.md) や [候補と外部条件](../docs/reference/candidates.md) などの契約文書へ反映してから実装へ進みます。実行条件とその結果はこのファイルの「検証結果」に、時点と条件を明記して残します。手順の全体は [検証方針](../docs/development/verification.md) にあります。
