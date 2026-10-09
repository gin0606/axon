# Quintモデル

Axon の lifecycle、包含と dependency、Group の実効 lifecycle、候補一覧の評価、文面・Note・状態変更履歴、記録の集合の Git 統合の意味論を、実行可能な Quint のモデルとして置きます。状態・遷移・包含・候補集合・情報の契約そのものは [lifecycle](../docs/reference/lifecycle.md) と [候補と外部条件](../docs/reference/candidates.md)、記録・衝突・違反・解決の契約は [保存と統合](../docs/reference/storage.md) が定義し、ここでは各モデルが何を対象にし、何をどう検査するかを案内します。

## モデル一覧

| ファイル | 対象 | 対象外 |
| --- | --- | --- |
| [`lifecycle_rules.qnt`](lifecycle_rules.qnt) | Issue と Group が共有する状態5種類・操作8種類と、種類ごとの基本遷移の前提・遷移先 | 状態変数と遷移の実行、包含、dependency、実効 lifecycle、条件 |
| [`issue_lifecycle.qnt`](issue_lifecycle.qnt) | 単独 Issue の lifecycle（`Reopen` を含む）と、抽象化した再浮上条件による浮上の導出 | 包含、dependency、Group、登録操作、declaration による編集、履歴の保存、記録者による権限判定、条件の種類と設定操作、一覧、浮上と着手許可の接続、CLI、永続化 |
| [`group_lifecycle.qnt`](group_lifecycle.qnt) | Issue と Group の包含、所属変更、新規登録、種類の変換、dependency、Group の実効 lifecycle、`Reopen`、判断候補・着手候補・`axon tasks` の一覧の行と状況、`axon show` も使う詰まっている理由の導出 | 文面と記録、条件の種類・設定と評価失敗、CLI と表示、ID 発行、永続化、記録の集合と統合 |
| [`lifecycle_reachability.qnt`](lifecycle_reachability.qnt) | `group_lifecycle` の到達性を補う4つの探索入口 | 許可条件・状態更新・検査する性質（`group_lifecycle` のものをそのまま使う） |
| [`candidate_evaluation.qnt`](candidate_evaluation.qnt) | `axon proposals`・`axon tasks` の1回の取得と `axon show` の1回の表示における評価範囲、評価回数、結果の共有、判定失敗 | 行の状況の導出、実コマンドと shell、作業ディレクトリ、終了コードの解釈、timeout と中断、出力の上限と診断、並行実行 |
| [`lifecycle_information.qnt`](lifecycle_information.qnt) | title・description の編集範囲、Note の追記、種類の変換をまたぐ履歴と現在の状態・種類の一致（Issue として登録した1件と Group として登録した1件） | 包含・dependency・実効 lifecycle・最終確認による追加の制約、Group から Issue への変換が要求する「子がない」、日時・理由・記録者情報、表示順、公開 ID、永続化 |
| [`record_integration.qnt`](record_integration.qnt) | 2 replica（Git worktree）が記録の集合を任意の部分集合で持ち寄る統合、衝突の検出と解決、構造の違反の検出と修復 | file の bytes と配置、lock、探索、変換、再浮上条件と一覧 |
| [`record_integration_paths.qnt`](record_integration_paths.qnt) | `record_integration` の到達しにくい経路を狙う 5 つの入口 | 同上 |
| [`record_integration_test.qnt`](record_integration_test.qnt) | 固定した順序で特定の経路を再現する `run` テスト | 探索 |

Quint の `import` の向きは `lifecycle_rules` を起点にした一方向です。`issue_lifecycle`・`group_lifecycle`・`lifecycle_information` がそれぞれ `lifecycle_rules` を取り込み、`lifecycle_reachability` と `candidate_evaluation` が `group_lifecycle` を取り込みます。`issue_lifecycle` と `lifecycle_information` は互いにも `group_lifecycle` にも依存しません。`record_integration` は他のモデルを取り込まず、`record_integration_paths` と `record_integration_test` がそれを取り込みます。`record_integration` の lifecycle 操作の前提は、統合の検査に要る範囲へ簡略化した部分集合です（変換、再浮上条件、Group の `Cancel` の細部を持たない）。実装の読取側の判定（`check_operation`）は、免除を結果が違反を増やさない場合に限って与え、統合で子を持った Issue の終了に直属の子の終了を要求する点でモデルの前提より狭いが、違反を増やさない検査を合わせた操作の可否はモデルと同じです。lifecycle・包含・dependency の規則の正本は `group_lifecycle` とし、両者で異なる部分は `group_lifecycle` を優先します。

## モデル化と実装検証の分担

すべての設計判断を Quint の状態に追加することはしません。状態・関係・操作の相互作用に不確実性がある部分をモデル化し、実行環境や付随情報の取得・保存は設計判断として記述して実装側で検証します。モデルへ残す検証専用の観測値も、対象の性質を確かめるために必要な範囲に留めます。

| 対象 | 扱いと理由 |
| --- | --- |
| lifecycle、包含、dependency | 遷移の前提・循環・終了条件が相互作用するため維持する |
| Group の実効 lifecycle と一覧の状況 | 保存値から導出する値で、操作の前提や候補集合との相互作用を検査するため `group_lifecycle` で維持する。表示の形式は実装側で検証する |
| 候補一覧の評価範囲・結果共有・失敗 | 評価省略が候補の取りこぼしや失敗の見落としにつながらないかを検査するため維持する |
| 条件コマンドの不透明な識別子 | 条件の置き換えと保持を区別するため維持し、実コマンドや環境はモデル化しない |
| 文面・Note・状態変更履歴 | 編集可能状態、追記専用性、履歴と状態の一致を検査するため維持する |
| label | title・description と同じ編集規則（終了後の固定）を持ち、候補集合・状況・遷移・包含・dependency に影響しないため、モデルは拡張せず、固定集合の検査・終了後の固定・種類の変換での保持・記録と codec を実装側で検証する |
| 履歴の日時・任意の理由 | 遷移に影響せず値の保持を調べるだけなので、探索には含めず実装検証で扱う |
| 記録者情報・エージェント連携・外部コマンドの実行環境 | 操作の可否に使わない付随情報や実行詳細として、設計判断と実装検証で扱う |
| 重複着手 | モデル上は原子的な状態遷移で扱い、実際の競合は保存処理の並行テストで確認する |
| 記録の集合、衝突、構造の違反と修復 | Git の統合が持ち込む記録の部分集合と操作の相互作用を検査するため `record_integration` で維持する。file の bytes、file の配置、Git が file をどう扱うかは実装側で検証する |

同一時刻の履歴が上書きされないこと、日時・理由・記録者情報の保持、自動取得と取得不能時の継続、Note の同内容追記の区別、保存失敗時の原子性、記録 file の hash と名前の照合、破損の検出は実装側の検証対象です。独立した予約状態やエージェント別の状態は持ちません。各モデルはそれぞれの検証範囲を維持し、変更に関係のある検査を選んで実行します。検査の分担と実装側の入口は [検証方針](../docs/development/verification.md) にあります。

## モデルが表す規則

コアの意味論のうち、モデルが前提として置く規則です。実装と契約文書はこの規則に合わせます。契約としての定義は [lifecycle](../docs/reference/lifecycle.md)（遷移、実効 lifecycle、前提、種類の変換、候補集合）、[候補と外部条件](../docs/reference/candidates.md#一覧の行と状況)（一覧の行と状況、詰まっている理由）、[CLIと表示の契約](../docs/reference/cli.md)（表記と `axon reopen`・`axon convert` の入出力）、[Declaration](../docs/reference/declaration.md)（保存値と種類の扱い）にあり、規則がその形になった理由は [設計判断](../docs/design/decisions.md#group-の着手を子から導出する理由) にあります。

- Issue は `Accept`・`Withdraw`・`Start`・`Release`・`Complete`・`Cancel`・`Reconsider`・`Reopen` の8操作を持つ。`Reopen` は `Completed` から `NotStarted` へ戻し、`Completed` から抜ける唯一の経路になる。
- Group は `Start`・`Release` を持たず、保存される lifecycle（stored）は `InProgress` にならない。実効 lifecycle は「stored が `NotStarted` で、直属の子に実効 `InProgress` か `Completed` があれば `InProgress`」と導出し、`NotStarted` の子 Group を通じて上へ伝わる。
- Group の `Complete` は stored が `NotStarted` のとき、直属の子がすべて終了し、最終確認が通れば許す。空の Group と子がすべて `Cancelled` の Group もこの経路で完了する。Group の `Cancel` は `Undecided`・`NotStarted` からだけ行う。
- Issue の `Start` は、全祖先が採用済み（stored が `NotStarted`）で、自身と全祖先の dependency がすべて `Completed` であることを要求する。親が実効 `InProgress` であることは要求しない。
- 種類（kind）は現在値で、`Undecided`・`NotStarted` の Issue は Group に、子のない `Undecided`・`NotStarted` の Group は Issue に変換できる。変換は lifecycle・所属・dependency を変えない。`InProgress` の Issue は先に `Release` する。
- 変換は lifecycle 遷移ではないが記録として残り、変換前後の種類が読める。lifecycle 遷移の記録の妥当性は、その遷移の時点の種類（直前の記録の現在値。登録時の種類を、その遷移より前の変換の記録で進めたもの）の規則で判定し、Entity の現在の種類では判定しない。Issue として `Start`・`Release` してから Group に変換した履歴と、Group として `NotStarted` から `Complete` し `Reopen` してから Issue に変換した履歴は、この判定で妥当になり、現在の種類で判定すると不正になる。`axon log` は変換を lifecycle 遷移と区別し、変換前後の種類とともに示す。
- 終了した Group の構成固定は維持し、`Completed` の Group は `Reopen`、`Cancelled` の Group は `Reconsider` で戻してから構成を変える。
- `InProgress` の Entity にも未完了の dependency を追加できる（その Entity 自身の `Complete` の検査で止まる）。Issue・Group の `Complete` は自身の依存先だけを検査し、祖先の依存先を検査しないため、Group に置いた依存先は Group 自身の `Complete` の前提になるが、配下の `Complete` の前提にはならない（[親子のlifecycle](../docs/reference/lifecycle.md#親子のlifecycle)、[Group の dependency を配下の完了の前提にしない理由](../docs/design/decisions.md#group-の-dependency-を配下の完了の前提にしない理由)）。Issue・Group から Group への dependency は依存先 Group の `Complete` まで待つ。
- 再浮上条件は候補の表示と `axon show` の状況・理由の表示だけに使い、ID を明示した操作は縛らない。
- `axon tasks` は平らな一覧で、Issue の行は未着手で自身と全祖先が浮上しているものと浮上に関係なく進行中のもの、Group の行は stored が `NotStarted` で浮上しているものと浮上に関係なく着手中の Issue を子孫に持つもの。Group の行の状況は配下から `Empty` > `Confirmable` > `Ready` > `InProgress` > `Blocked` の優先順位で導出する。直属の子がない Group は完了できても `Empty` で、次の一手は計画を書くこと（`axon complete` の可否は変えない）。詰まっている理由（未完了の dependency、`Undecided` の子、未終了の子 Group、浮上していない着手可能な子孫、`Undecided` の祖先、浮上していない祖先、自身の再浮上条件が未成立）は、完了できず、着手候補の子孫も着手中（stored が `InProgress`）の Issue の子孫もない Group の行に示し、`Blocked` の行に限らず、完了済みの子孫はあるが着手中の子孫がない実効 `InProgress` の行と、完了できない `Empty` の行にも付く。`Confirmable` の行、`Ready` の行、完了できる `Empty` の行は次の一手が状況から分かるため理由を示さない。
- `axon show` は対象の状況と詰まっている理由を `axon tasks` の行と同じ導出で示し、そのために対象の行が必要とする範囲（祖先、対象自身、Group なら着手可能な子孫の Issue とその間の Group）の再浮上条件を評価する。詰まっている理由は `axon tasks` の行に限らず stored が `NotStarted` のすべての Group に導出でき、詰まっている Group には少なくとも一つ付く。浮上していない祖先の理由と自身の再浮上条件が未成立という理由は、`axon tasks` の行になる Group では着手中の Issue を子孫に持つ行にだけ当たり（浮上して行になる Group は自身と全祖先が浮上している）、その行は詰まっていないため、詰まっている行には付かない。これらの理由は行に出ない詰まっている Group に `axon show` が示す。自身の条件は全祖先が浮上しているときだけ評価されるため、浮上していない祖先がある Group には自身の条件が未成立という理由は付かない。

### 反例と review から足した規則

書き始めた時点の案では足りず、`group_lifecycle` の反例と独立 review から足した規則です。いずれも invariant を弱めずに規則側を直しました。

- **守る性質は「実効 `InProgress` の Group の全祖先が採用済み」。** `InProgress` の Issue についての同じ性質を Group の実効 lifecycle へ広げたもの。この性質を保つために、Issue・Group の `Complete` と `Reopen`、着手・完了した子孫を持つ Group の `Accept` は全祖先が採用済みであることを要求し、実効が `InProgress` か `Completed` の Entity の移動先は、移動先の Group 自身を含む全祖先が採用済みであることを要求する。Group の `Withdraw` は実効が `NotStarted` のときだけ許す。実効は `NotStarted` の子 Group を通じて上へ伝播するので、孫が着手中なら祖父も `Withdraw` できない（直属の子の保存だけを見ると、子 Group の保存は `InProgress` にならないため、この穴が反例として出た）。
- **`Undecided` の Group の下に完了済みの子孫があることは許す。** 完了済みの子を持つ Group を `Cancel` してから `Reconsider` すると、この状態になる。取りやめた子 Group の下に完了済みの子孫があるときの親の `Withdraw` も同じ理由で禁じない。その Group を再び `Accept` するには全祖先が採用済みであることが要る。
- **`Blocked` は残余として定義する。** Group の行の状況は、`Empty`（直属の子がない）、`Confirmable`、`Ready`、`InProgress`、`Blocked`（子があり、上のどれでもない）の順で決める。当初の「未着手の Issue の子孫はあるが着手候補がない」という定義では、採用直後で子がすべて `Undecided` の Group や、子が空の子 Group しか持たない Group がどの状況にも入らなかった。詰まっている理由は、未完了の dependency、`Undecided` の子、未終了の子 Group、浮上していない着手可能な子孫、`Undecided` の祖先の 5 種類で尽きる。`axon show` が条件を評価するようになった際に、行に出ない Group を対象にするために浮上していない祖先を 6 つ目として足し、その後、自身の条件で隠れている Group を `axon show` だけで読めるように自身の再浮上条件が未成立を 7 つ目として足した。条件に由来するこの 2 つは条件入力が不成立ならほぼ常に付くため、それらを除いても理由が残ることを別の invariant で検査し、5 種類で尽きる性質を保っている。
- **空の Group は `Confirmable` より先に `Empty` にし、詰まっている理由は `Blocked` の行に限らない。** 取り込み後の独立 review で、採用済みで dependency のない空の Group が `Confirmable` になり、完了済みの子孫はあるが着手中の子孫がない Group が理由のない `InProgress` の行になることが分かった。どちらも定義どおりで反例ではないが、空の Group の次の一手は完了ではなく計画を書くことなので `Empty` を先に判定し、`InProgress` の行は「配下の仕事が始まっている」という実効 lifecycle と同じ概念のまま維持して（`Blocked` へ落とす案と複合表示は採らない）、理由の導出を「完了できず、着手候補の子孫も着手中の Issue の子孫もない Group の行」全般へ広げた。`Confirmable` の行と完了できる `Empty` の行は次の一手が状況から分かるため、理由を要求しない。着手中の Issue に後から足した未完了の dependency はその Issue の行の問題として扱い、Group を詰まっているとは見なさない。

規則を直す前の実行で出た反例は、五つの状況で尽くせない Group の行、`Undecided` の祖先の下の実効 `InProgress`（空の Group の完了、完了済み Entity の移動、取りやめ・見送り・再検討・採用の組合せ）、理由のない `Blocked`、近傍が狭すぎた行き止まり検査の偽陽性の 4 件。独立 review からは、空虚な invariant と名前とずれた witness を取り込みました。

### モデリングで見えた edge case

- **再開は依存元と祖先の側から順に行う。** `Completed` の依存元がある Entity は `Reopen` できず、依存元を `Reopen` するにはその祖先が採用済みである必要がある。数段の `Reopen` を要する場合がある。行き止まりの検査はこのため、完了済みの依存元とその祖先を近傍に加えている。
- **`Undecided` の祖先の下の採用済み Group は、子があれば `Blocked`、なければ `Empty` として一覧に出て、理由に `Undecided` の祖先が付く。** 祖先そのものは `Undecided` なので一覧に出ない。同じ理由で、`Undecided` の祖先の下の未着手 Issue も浮上していれば `Blocked` の行として出る。
- **着手中の Issue を子孫に持つ Group は浮上していなくても一覧に出る。** 着手中の仕事を見失わないため、Issue の `InProgress` と同じ扱いにしている。その Group に着手候補の子孫があれば状況は `Ready` になる。完了済みの子孫だけで実効 `InProgress` の Group は、浮上していなければ一覧に出ない。

## 統合モデルが表す規則

保存層のうち、`record_integration` が前提として置く規則です。契約としての定義は [保存と統合](../docs/reference/storage.md) にあり、規則がその形になった理由は [設計判断](../docs/design/decisions.md#保存層を記録の集合にしgit-統合を読取時の検出に委ねる理由) にあります。モデルは記録の集合と Entity ごとの因果 DAG だけを扱い、記録 1 件 1 file という保存形式、file の bytes、Git が file をどう扱うかを扱いません。

- 保存先は不変な記録の集合。現在値は記録から導出し、本文・関係・条件の編集も記録にする。
- Entity ごとの記録は因果 DAG を成す。head が複数ある Entity を「衝突」とし、全 head を親にする解決記録で一つの値を選ぶ。解決記録以外の記録は親を一つしか持たない。Note は因果を持たない集合。
- Git の merge・rebase・cherry-pick はいずれも「相手の記録の部分集合を取り込む」操作で、Axon は Git の統合時に呼ばれない。衝突と構造の不整合は次の読取と CI が検出する。
- 記録 ID は内容から決まる hash で、同じ ID なら同じ内容。同じ Entity ID の作成記録が二つあれば作成記録が二つとも head になるので、通常の衝突として見える。
- cherry-pick や revert で記録の一部だけが branch に入ると、親の欠けた記録が古い記録と並んで head になり、偽の衝突として見える。これを受け入れ、CLI が「片方は親記録が欠けていて新しい可能性が高い」と示す。祖先集合を記録に持たせて偽の衝突をなくす案は、困ったときに後から足す。

### 統合側で反例と review から足した規則

書き始めた時点の案では足りず、反例と独立 review から足した規則です。いずれも invariant を弱めずに規則側を直しました。コア側の規則のうち守る性質と `Undecided` の Group の下の完了済み子孫は「モデルが表す規則」の「反例と review から足した規則」を正とし、統合モデルにも同じ規則を入れています（一覧の状況は統合モデルの対象外）。

- **複数 head はすべて衝突にする。** 当初は値の等しい並行 head を次の記録が畳む規則を置いていたが、それが要った場面（両側の worktree が親 Group を `axon start` する）は Group の `Start` を導出に変えて消えた。残るのは同じ actor が二つの worktree で同じ Issue を `axon start` した、両側で同じ提案を `axon accept` した、といった稀な場合で、衝突として見えた方が二重作業に気づける。畳む規則をやめると、現在値の導出が「head が一つならその値、複数なら衝突」の二分岐になり、二重登録の特別扱いも消える。
- **Issue の `Start` の排他性は現在値で表す。** `InProgress` の現在値に着手した actor を含め、着手の記録が誰のものかを現在値で読める。特別な規則を足さずに、通常の衝突判定で済む。
- **衝突があれば通常操作を止める。** 衝突中の Entity が一つでもある replica では、解決と Note 以外の操作を拒否する。
- **構造の不整合は「違反」の集合として導出し、通常操作は違反を増やさない。** 違反は Entity ごとに種類を持つ（包含の循環、親の不在、終了した親の下の未終了、`InProgress` の Issue と実効 `InProgress` の Group の未採用の祖先、`Completed` の未完了の依存先、依存先の不在、通常完了経路の循環）。lifecycle 操作を含む全通常操作は、各操作の前提に加えて「操作後の違反が操作前の違反の部分集合」であることを要求する。当初は所属変更・dependency の追加・登録だけに課していたが、免除された前提の下で `Reopen`・`Reconsider` が違反を作れた（invariant `invLocalStep` は全ローカル操作に部分集合を要求しており、`lifecycleOp` と、違反を増やせない文面編集・dependency の削除が検査を省いていた）。モデルでは、所属変更・dependency の追加・登録の検査を常に課し、lifecycle 操作・文面編集・dependency の削除の検査は違反のある store でだけ課す。有効な store では各操作の前提だけで違反を防ぐことを `invLocalStep` が検査し続けるためで、実装は lifecycle 操作を常に検査し、違反を増やせない文面編集・dependency の削除は前提だけで確定する。無関係な違反が残っていても操作は止まらない。
- **循環の違反の構成員は循環上の Entity だけ。** 通常完了経路の循環（包含の循環も同じ）の違反は、前提をたどって自身に戻れる Entity にだけ付き、循環を待つだけの Entity には付かず、免除も与えない。待つ側まで構成員にすると、待つ側どうしに新しい循環を足しても違反の集合が増えず、部分集合の検査を通り抜けて通常操作が循環を作れた（`cycleAmongWaitersIsRejectedScenario` で再現してから定義を変えた）。待つ側への循環にならない dependency の追加と登録も、待つ側が違反に加わるとして拒否されていた。すでに循環上にある Entity どうしの間の辺は構成員を変えないので、この検査では拒否されない（そのうち循環を閉じる辺は次項の検査が拒否する）。
- **所属変更・dependency の追加・登録は、新しい辺が循環上に載るなら違反のある store でも拒否する。** 変更で新しく持った関係（dependency、所属）が誘導する通常完了経路の前提の辺（依存先は自身と子孫にその依存先を待たせ、所属先は新しい子を待ち、新しく祖先の連なりに加わった Group は自身と子孫にその依存先を待たせる。移動の前後で共通の祖先の依存先は新しい関係ではない）のどれかについて、辺の先から前提をたどって辺の元へ戻れるなら拒否する（`relationOnCycle`）。辺は関係ごとに数え、別の関係がすでに同じ組を誘導していても検査する（`relationRepeatingPairIsRejectedScenario`）。共通の祖先の下の兄弟 Group への移動は通る（`siblingMoveUnderSharedAncestorScenario`）。モデルは有効な store ではこの検査を省く。部分集合の検査だけでは、同じ循環への chord や別の循環の構成員どうしで閉じる循環が構成員を増やさずに通り、元の循環を直した後に通常操作で足した循環が残った（`chordOnCycleIsRejectedScenario`・`edgeBetweenCyclesIsRejectedScenario` で、部分集合の検査は通り辺の検査が拒否することを確かめている）。有効な store では循環上に載る辺は必ず違反を増やすので、この検査は部分集合の検査に含まれる。修復は辺の除去で進むため、修復可能性には影響しない。
- **「全祖先が採用済み」は親の連なりの各段が settled で `NotStarted`。** 直属の親が settled で settled な祖先がすべて `NotStarted` という当初の定義では、連なりの途中に記録の欠けた Entity があっても `Start` が通った。契約と実装の読み方に揃え、途中に衝突中または記録の欠けた Entity があれば採用済みとみなさない。
- **違反に含まれる Entity には修復のための免除を与える。** 終了した親の配下は変更しないという固定と、`Completed` の dependency の固定、`Completed` の依存元による `Reopen` の阻止を、その Entity が違反に含まれるときだけ免除する。免除は操作の前提を緩めるだけで、違反を増やす操作は免除の下でも通らない。終了した Group どうしの包含の循環（両側の移動と完了の取り込みで起きる）は取り外しで、完了済みの相互依存（解決の選択で起きる）は dependency の削除で直せる。免除は有効な store では働かない。違反があり衝突がない store には、修復を確実に進める通常操作が一つはあることを invariant にしている。修復の進み具合は「違反の集合が縮む」か「集合が同じでも違反に含まれる Entity の dependency と所属の辺が減る」で測る。
- **解決記録は違反の検査を免除する。** 解決の選択で違反ができることがあり、それを解決時に拒否すると、どの選択も拒否されて行き止まりになる場合がある。解決で head を一つに戻してから、通常操作で直す。
- **同じ ID の二重登録は、片方の系列を選んで解決する。** 作成記録が二つとも head なので通常の衝突として見え、解決記録が両系列の head を親にして片方の値を採る。捨てた側の内容は登録し直す。

規則や検査を直す前の実行で出た反例は、孫が着手中の祖父 Group の `Withdraw`、親より先に届いた解決記録を親の値と照合していた検査、二重に絡んだ循環で一手では違反が減らない修復可能性の検査の 3 件。独立 review からは、実効 `InProgress` の Group の祖先を統合モデルの全体検査が見ていなかったこと、Issue の `Complete` で不整合を正常化できたこと、構造の不整合が無関係な操作を止めていたこと、終了した Group どうしの循環と完了済みの相互依存が直せなかったこと、二重登録で replica が凍ること、空虚な invariant と名前とずれた witness を取り込みました。実装の独立 review からは、免除の下で `Reopen`・`Reconsider` が違反を増やせたこと、途中で切れた祖先の連なりを採用済みと扱っていたこと、循環を待つ Entity どうしの新しい循環を検査が見逃していたこと、すでに循環上にある Entity どうしで閉じる新しい循環を検査が見逃していたことを取り込みました。

### 統合側で見えた edge case

- **同じ衝突を両側で別々に解決すると解決記録が並行になる。** 同じ値を選んでいても再び衝突として見え、もう一段の解決で収束する。増殖はしない。
- **cherry-pick や revert で親記録が欠けた記録が入ると、偽の衝突になる。** 受け手が古い祖先の記録を持っている普通の場合、その祖先と新しい記録が並んで head になる。記録は親しか知らないので、実際には祖先どうしだと判定できない。新しい方を選んで解決すれば正しい値になり、後で残りの記録が届いても値は変わらない。gap を作るのは、Axon の状態を触った commit を後から部分的に取り消す（revert、rebase での drop）か拾う（cherry-pick）操作と、未 commit の記録 file を残したまま別の commit へ移る操作で、merge、squash merge、rebase merge は記録を足すだけで自身では作らない（片側の revert による削除を merge が他側へ伝えることはある）。code と Axon の状態を同じ commit に入れる運用は無関係。
- **解決記録だけが先に届いた replica でも現在値は決まる。** 選ばれた側の親が無くても解決記録が値を持つので、gap として報告しつつ読める。
- **Git は構造の違反を止めない。** 片側が Group を `Complete`、他側がその Group に子を登録した場合、両側の移動で循環ができた場合、片側が完了しつつ他側でその依存先を `Reopen` した場合、片側で Group を未採用の下へ移し他側でその配下を完了した場合は、取り込みだけで違反になる。いずれも次の読取で検出でき、`Reopen`・取り外し・dependency の削除・再採用で直せる。
- **dependency の追加と完了は黙って混ざらない。** dependency は依存元の Entity の記録なので、片側で dependency を足し他側でその Entity を完了すると、同じ Entity の値の異なる head になり衝突として見える。完了済みの Entity に未完了の dependency が黙って入るのは、依存先を他側で `Reopen` した場合に限る。
- **解決の選択が違反を作ることがある。** 両側が互いを依存先にして完了していると、両方の dependency 側を選べば完了済みの相互依存になる。免除された dependency の削除で直す。
- **二重に絡んだ dependency の循環は一手では違反が減らない。** 両側で足した dependency が取り込みで絡み、直接の dependency と祖先の dependency の二経路で互いを待つ形になると、どの一手も違反の集合を縮めない。dependency を一本ずつ外せば直る。修復可能性の検査はこのため辺の数も進み具合に数える。
- **終了した Group どうしの循環は取り外しで直す。** 循環の中では互いの下への移動が拒否され、親が終了しているので通常は取り外しもできないが、違反に含まれる Entity には免除が働く。
- **完了と解放の衝突で未完了側を選べる。** 解決記録の後に通常の `Start` が続く。`Completed` からの再開経路ではない。
- **免除の下でも `Reopen`・`Reconsider` が違反を作るなら拒否される。** 互いを依存先にして `Completed` になった Group どうしでは、依存元による阻止が免除されていても、一方の `Reopen` は他方に未完了の依存先を残すので通らない。修復は dependency の削除で行う。片側の完了と他側の依存先の `Reopen` が混ざった状態は、依存元の `Reopen` が違反を減らすので通る。

### 統合モデルの対象外と限界

- `record_integration` は file の bytes、記録 file の配置と hash の照合、途中で切れた file、lock、探索を扱わない。これらは Rust の fixture で検査する。Git が記録 file をどう統合するかは実験で確かめた（[設計判断](../docs/design/decisions.md#記録-1-件-1-file-にした理由)）。
- `record_integration` は Entity 5 件・replica 2 つ・最大 40 step の範囲で、3 replica 以上や長い分岐は探索していない。未登録の枠は 1 つで、登録は 1 trace に 1 回。取り込みは記録を増やすだけで、revert による記録の削除とその merge での伝播は扱わない。削除の結果できる状態は部分取り込みの状態と同じ形なので、gap と偽の衝突の扱いはそこで検査している。
- `record_integration` の修復可能性の invariant は「修復を進める操作が一つはある」ことだけを見る。有効な状態まで戻れることは、進み具合が有限で単調に減ることから従うが、テストの経路以外で直接は確かめていない。
- 変換操作と再浮上条件・一覧の状況は `group_lifecycle`・`lifecycle_information` だけが扱い、統合との組合せは探索していない。

## 各モデルの探索と検証する性質

### `lifecycle_rules`

状態変数を持たない純粋な定義だけの module で、単独では実行しません。`canPerform` は Issue の基本遷移の前提、`canPerformAs` は種類ごとの前提（Group は `Start`・`Release` を持たず、`NotStarted` から完了し、`Undecided`・`NotStarted` からだけ取りやめる）、`applyOperation` は遷移先で、他のモデルがこれらを共有します。

### `issue_lifecycle`

提案として登録した直後の `Undecided` から始め、外部入力は不成立から開始します。`step` は8操作と外部入力の変化を探索候補にし、前提を満たすものだけが実行されます。外部入力は両方向へ変化でき、`Cancelled`・`Completed` の間も変化しますが、条件の評価対象には戻りません。

invariant は、`NotStarted`・`InProgress`・`Completed` には直近の見直しより後の採用が必要なこと、`InProgress` の入口が `NotStarted` からの `Start` に限ること、`Completed` の入口が `InProgress` からの `Complete` に限ること、`Completed` から抜ける経路が `Reopen` だけで `NotStarted` へ戻ること、明示操作の行き止まりがないこと（規則の定義から直接従う構成の確認）、外部入力と明示操作が互いの値を変えないこと、`Cancelled`・`Completed` が評価されず浮上しないこと、評価対象の浮上が入力の成立と一致することを検査します。witness は8操作それぞれの到達に加え、未判断からの見送り・未着手からの取りやめ・進行中からの打ち切りを区別し、`Reopen` 後の再着手、外部入力の両方向の変化、各状態での浮上・非浮上、再検討後と `Reopen` 後の浮上・非浮上を観測します。初期状態だけではどの witness も成立しません。

条件定義を固定しているため、取りやめ時の定義の保存や読み戻しは検証しません。`NotEvaluated` は意味上の評価除外を表すもので、実際に外部コマンドが呼ばれないこと、読み取りコスト、実行失敗と副作用は実装側で検証します。

### `group_lifecycle`

Entity は固定 ID 5件。0 は Group、1 と 2 は 0 の子 Issue、3 は所属なしの Group、4 は未登録の枠で、全員 `Undecided`、条件入力はすべて不成立から始めます。未登録の枠は探索領域を有限にする仕組みであって、新しい lifecycle ではありません。これは探索の開始点であり、製品の再浮上条件の初期値ではありません。`step` は Group の操作、Issue の操作、それ以外（移動、登録、変換、dependency の編集、条件入力の変化）の三枝から選びます。ID は固定集合から選び、未登録 ID の再利用と削除は扱いません。

invariant は、Group の保存が `InProgress` にならないこと、実効 lifecycle が導出の不動点であること、実効 `InProgress` の Group が着手・完了した子孫を持ちその全祖先が採用済みであること、着手・完了の前提、Group の `Completed` への到達が最終確認を伴う `Complete` だけであること、`Completed` から抜けるのが `Reopen` だけで他を変えないこと、包含（所属の妥当性、Issue が子を持たないこと、非循環、終了した Group の子孫がすべて終了し構成が変わらないこと）、各操作の影響範囲、変換が種類だけを変えること、登録が採否を反映し自動で着手しないこと、dependency（参照の妥当性、通常完了経路の非循環、`Completed` の dependency がすべて `Completed` で固定されること、自己依存と祖先・子孫間の依存の不在）、候補（判断候補と着手候補の定義、Group が着手候補にならないこと、判断候補と着手候補・着手中の非重複、条件入力が着手可否を変えないこと）、一覧の状況の定義と優先順位、詰まっている Group（完了できず、着手候補の子孫も着手中の Issue の子孫もない）に、行に出るかに関係なく理由があり、条件に由来する 2 つ（浮上していない祖先、自身の再浮上条件が未成立）を除いても理由があること、浮上していない祖先の理由と自身の再浮上条件が未成立という理由が `axon tasks` の行では着手中の Issue を子孫に持つ行にしか現れないこと、詰まっている行は自身と全祖先が浮上していること、`Blocked` の行、着手中の子孫がない `InProgress` の行、完了できない `Empty` の行が詰まっていること、未終了の各 Entity について自身・祖先・子孫・依存先とその完了済みの依存元の閉包のどこかで操作できることを検査します。前提と更新の定義から直接従う性質は「構成の確認」として file 内で区別しています。

witness は各操作の到達に加えて、`Reopen` 後の再着手、完了済みの子を持つ Group の `Reopen`、`Completed` の依存元による `Reopen` の拒否、空の Group と全子 `Cancelled` の Group の完了、祖先の dependency による着手の阻害と解禁、浮上していない Issue への ID を明示した着手、三階層での実効 `InProgress` の伝播、孫が着手中の祖父の `Withdraw` の拒否、変換と変換後の登録、実効 `InProgress` の Entity の移動、直接・間接の循環の拒否、着手中に足した未完了の dependency による完了の阻止、五つの状況と優先順位、完了できる空の Group の `Empty`、子のある詰まっている行での 5 つの理由、完了済みの子孫だけで実効 `InProgress` の Group が浮上せず行に出ないこと、行に出ない Group での浮上していない祖先の理由、行に出ない Group での自身の再浮上条件が未成立という理由（着手可能な子孫を隠している場合と、完了済みの子孫だけで実効 `InProgress` の場合を含む）、理由が付く `InProgress`・`Blocked`・`Empty` の各行を観測します。理由のない詰まっている Group（`wStalledWithoutListedReason`）と、実効 `InProgress` が `Undecided` の祖先の下にできる状態とそこへ至る経路（`wEffectiveWorkingUnderUnadopted`・`wUnadoptedViaComplete`・`wUnadoptedViaMove`・`wUnadoptedViaAcceptOrReopen`）の 5 つは、規則で塞いだことを 0 trace で確かめる対象として残しています。

行き止まりがないという性質は整理・再判断・`Reopen` の操作も含み、当初の計画どおり必ず完了できることを意味しません。同じ所属の再指定はモデルでは無効な操作として探索から外しますが、CLI では同値の指定は成功した no-op です。前提の循環検査と包含の検査はこの固定範囲を対象とし、四階層以上の木や、より多い Entity を含む具体的なグラフは探索していません。追加できる Entity が1件という探索上の上限があり、新規登録が無効になることは製品が追加件数を制限するという意味ではありません。最終確認が通るかは抽象入力で、確認工程の実装や不合格の理由は扱いません。

### `lifecycle_reachability`

通常探索では、取りやめ・見送り・変換が先行して、`Reopen` を経る経路、祖先の dependency の完了による着手の解禁、一覧で `Ready` と実効 `InProgress` が重なる Group、規則で塞いだ経路の再確認に到達しにくくなります。探索を厚くするより、選ぶ操作を絞った入口から狙うほうが、同じ試行数で桁違いに濃く検査できます。

`group_lifecycle` と同じ `init` と操作を使い、1 step で選ぶ操作だけを絞る4つの入口を持ちます。`workAndReopen` は採用・着手・完了・`Reopen` と移動・登録・dependency の追加を選び、`Reopen` 後の再着手、完了済みの子を持つ Group の `Reopen`、三階層での孫 Issue の着手、`Completed` の依存元による `Reopen` の拒否を調べます。`dependAndStart` は dependency の追加と採用・着手・完了・移動を選び、祖先や自身の依存先の完了で子孫の Issue が着手可能になる瞬間を調べます。`listAndWork` は条件の成立と採用・着手・移動・登録を選び、`Ready` と実効 `InProgress` が重なる Group と、実効 `InProgress` の Group の移動を調べます。`readoptAfterClose` は8つの lifecycle 操作と2つの固定の移動だけを選び、完了済みの子を持つ Group の祖先を取りやめ・見送り・再検討で外したあとの採用・`Reopen` を集中して試し、規則で塞いだ経路が再び開いていないことを確かめます。許可条件・状態更新・到達目標は変えないため、invariant は `group_lifecycle` のものをそのまま指定します。これらの選び方は到達性の確認用であり、製品の workflow を定めるものでも、通常探索の分布を表すものでもありません。

### `candidate_evaluation`

`group_lifecycle` の状態と許可条件を再利用し、一覧の取得と `axon show` の表示を開始・Entity ごとの評価・公開という複数の観測ステップへ具体化します。`queryInit` は `group_lifecycle` の `init` に加えて全 Entity の条件を `Unset` から始め、`queryStep` は取得の各段と、取得外の条件編集・計画変更を混ぜて探索します。取得の種類は `axon proposals`、`axon tasks` と、全 ID から選んだ対象の `axon show` です。`group_lifecycle` の bool 入力と一覧の行の集合は失敗のない場合の意味を定める基準として残り、評価回数・省略・失敗をこのモデルで扱います。`group_lifecycle` の `false` 初期入力は未成立の外部入力から探索を始める指定で、条件未設定の初期値ではありません。このモデルはすべて `Unset` から始め、未設定が成立として扱われることを検査します。未設定の成立判定も観測ステップとして数えますが、外部コマンドを呼ぶという意味ではありません。

評価の対象は、`axon proposals` では stored が `Undecided` の Entity、`axon tasks` では stored が `NotStarted` の Issue と Group で、どちらも終了した祖先の配下を外します。祖先は上から評価し、未成立ならその配下を評価しません。実効 `InProgress` の Group も stored は `NotStarted` なので、自身の浮上判定として評価されます。`InProgress` の Issue はどちらの対象にも入らず、子を持たないので祖先としても評価されず、`axon tasks` には評価なしで載ります。`axon show` では、対象が `axon tasks` の行になる場合（stored が `NotStarted`）に限り、対象自身と、Group なら着手可能な子孫の Issue を対象にし、祖先は他の取得と同じく上から評価します。`Undecided`・進行中・終了した対象は何も評価しません。行の状況（`Ready`・`Blocked` など）は `group_lifecycle` が導出し、このモデルは一覧では行の集合だけを、`axon show` では対象の状況と詰まっている理由が評価した範囲で決まることを見ます。

成功時には、未評価の入力をすべて成立と仮定しても、すべて未成立と仮定しても、`group_lifecycle` 側の判断候補または一覧の行の集合と一致することを検査します。これにより、評価を省いた入力が候補の欠落を隠していないかを調べます。失敗は成功の集合とは異なる variant で表し、失敗した Entity を保持します。途中で観測した候補は成功結果として公開しません。合わせて invariant で、評価済みの各 Entity が必要な対象かその祖先であること、全祖先の成立後にだけ評価すること、評価回数の上限、同一呼び出し内の結果共有と次回の初期化、条件編集と明示操作が評価処理を呼ばないこと、取得と外部の結果が保存した計画・条件を変えないこと、`axon tasks` が進行中の Issue とそれを子孫に持つ Group をすべて含むこと、進行中の Issue を評価しないこと、`axon show` の評価範囲が同じ計画の `axon tasks` の範囲に含まれること、`axon show` の対象の状況と詰まっている理由（Issue では浮上の有無）が未評価の入力の仮定によらず決まることを検査します。witness は、成功・空・失敗の各結果、未設定と外部コマンドの評価、条件の設定・訂正・解除と失敗からの回復、成立の持続と次回の未成立化、祖先の未成立による評価の省略と同じ祖先の結果の共有、終了した Entity の評価省略、依存待ちや `Undecided` の祖先を持つ未着手が `axon tasks` に出ること、進行中の Group の評価と非浮上のままの表示、未着手の Group の行、`Reopen` した Issue の再表示、評価に失敗した当の Entity への明示的な着手、`axon show` が Group の浮上していない着手可能な子孫を評価して見つけること、Issue の対象が祖先の未成立で評価されないまま浮上しないと決まること、`axon tasks` の行にならない対象で何も評価しないこと、`axon tasks` より狭い範囲で終わること、`axon show` の判定失敗を観測します。

通常探索では `Reopen` した Issue の再表示と、評価に失敗した Entity への着手が、計画変更の順序が長く噛み合ったときにしか成立しません。`workAndList` は Group 0 とその子 Issue 1 への採用・着手・完了・`Reopen` と条件の設定、`axon tasks` の取得だけを選ぶ補助の入口で、これらと進行中の Group に関する witness を狙います。invariant は通常探索と同じものを指定します。

一覧取得中の lifecycle・包含・dependency・条件設定は固定し、明示操作は取得の間に行います。これは単一呼び出しを調べるための前提であり、並行更新や snapshot の実装契約を決めるものではありません。外部の結果は Entity ごとの評価時に選ぶため、異なる Entity の条件を同一瞬間に観測する保証も置きません。独立した枝の評価順は非決定的で、最初の失敗で取得を終了します。エラー時にどの枝まで観測済みかは保証しません。実行契約は自然言語で定め、Quint では実行結果を成立・未成立・判定失敗へ抽象化します。shell・作業ディレクトリ・環境変数、終了コードの解釈、timeout と process group の終了、出力の上限と診断、実際に外部コマンドを呼ばないことと呼び出し回数は実装側で検証します。

### `lifecycle_information`

固定2 Entity（0 は Issue、1 は Group として登録）、文面の値3種類で、未判断の登録済み Entity から始めます。文面と Note の内容は不透明な整数で更新と保持を区別し、履歴は lifecycle 遷移の変更前後の状態と種類の変換（変換前後の種類）の追記列へ抽象化します。日時・任意の理由・記録者情報は設計上の保存項目として残しますが、操作の可否や状態遷移に影響しないため探索変数にしません。登録操作と初期状態の記録は扱わず、登録時の種類は ID から引きます。状態遷移の前提は `lifecycle_rules` の種類ごとの規則で、Group は `Start`・`Release` を持たず `NotStarted` から完了します。種類の変換は保存値が `Undecided`・`NotStarted` のときだけ行い、Group から Issue への変換が要求する「子がない」は包含を持たないこのモデルの外です。

invariant は、操作が対象以外の Entity を変えないこと、文面の編集が編集可能な状態に限ること、終了後の文面が固定されること、Note が追記専用であること、状態変更と履歴追加が一体で確定すること、履歴を各時点の種類の規則で再生すると現在の状態と種類になること、`Completed` から抜けるのが `Reopen` だけで `NotStarted` へ戻ること、種類は変換だけで変わり変換は種類と履歴への変換の追記以外を変えないこと、Group が `InProgress` を保存しないことを検査します。履歴の再生は、lifecycle 遷移の記録をその時点の種類の規則で判定し、変換の記録で種類を進めます。witness は8遷移それぞれの到達（`Start`・`Release` は Issue だけ）、Group の `NotStarted` からの完了、採用後・着手中・再検討後・`Reopen` 後の文面編集、各状態での Note 追加、同じ内容の追記が別の記録になること、両方向の変換、変換後の lifecycle 遷移、Issue として `Start` し続けて `Release` してから Group へ変換した履歴、Group として `NotStarted` から `Complete` し `Reopen` してから Issue へ変換した履歴、各時点の種類では妥当な履歴が現在の種類で全記録を判定すると不正になる状態を観測します。基本遷移だけを持つため、このモデルが許す遷移がそのまま実際の Group や依存を持つ Issue で許可されるわけではありません。実時刻の取得、精度、時計補正、公開 ID、actor、記録の表示順、永続化と保存失敗はこのモデルの外です。

### `record_integration`

replica 2 つ、actor 2 つ、Entity は固定 ID 5 件。0 は Group、1 は 0 の子 Issue、2 は所属なしの Group、3 は所属なしの Issue、4 は未登録の枠で、両 replica は同じ base から始める。`step` は一方の replica での通常操作（lifecycle 8 操作、文面編集、移動、dependency の追加と削除、登録、Note）、解決、取り込みから選ぶ。取り込みは相手にあって自分にない記録を、全部（merge、squash merge、rebase merge）、連番の接頭辞まで（rebase の途中）、一件だけ（cherry-pick）のいずれかで取る。

検証専用に、各記録は祖先の記録 ID の集合を持ち、各 replica の view（head、衝突、settled、現在値、gap、違反）は状態変数として状態ごとに一度だけ更新する。どちらも記録と store から導出できる値の写しで、実装の保存項目ではない。head の判定には祖先集合を使わない（実装は親だけを知る）。

invariant は、通常操作が衝突のない replica でしか起きず違反を増やさず gap を変えず他の Entity の記録を増やさないこと（所属変更・dependency の追加・登録は action がこれを検査するが、lifecycle 操作・文面編集・dependency の削除は有効な store では検査を課していないため、各操作の前提だけで違反を防ぐことをこの invariant が検査する）、有効な store では修復の免除が働かないこと（免除の定義から直接従う構成の確認）、違反があり衝突がない store には修復を確実に進める通常操作があること、全部取り込みの後に settled な Entity の現在値が取り込み前の自分か相手の現在値であること（値を捏造しない）、両側が settled で値が異なりどちらの head も他方に先んじていなければ取り込み後は衝突になること（黙って片方を選ばない）、解決の直後に settled になることを検査します。記録が消えないこと、記録 ID の一意性、解決記録以外の記録が親を一つしか持たないこと、解決記録が親のどれかの値を採ること、view の写しの一致、owner と lifecycle の対応、Group が `InProgress` を保存しないことは構成の確認として残しています。

witness は、別 actor の並行 `Start` の衝突、値の等しい並行記録が衝突として見えその解決で片方を選ぶこと、解決、解決記録どうしの並行（同じ値でも違う値でも再び衝突になり、解決の解決で収束）、部分取り込みによる gap とその充足、gap による偽の衝突、収束、違反の種類ごとの観測（終了した Group への子の流入、包含の循環、通常完了経路の循環、未採用の祖先、完了済みの未完了依存先）、それらの修復（`Reopen` による修復、免除を使った修復）、完了と解放の衝突で未完了側を選んで再着手すること、取りやめと着手の衝突、文面の衝突、両側の Note、同じ ID の二重登録とその解決、各操作の到達、完了済みの子を持つ Group の `Reopen` を観測します。

### `record_integration_paths`

`divergeAndResolve` は両 replica が Issue 1 だけを操作して全部取り込みと解決を繰り返す（別 actor と同 actor の並行 `Start`、並行 `Accept`、完了と解放、解決記録どうしの並行）。`breakAndRepair` は着手・完了・再開・取りやめ、移動、dependency の追加、Group への登録と全部取り込みを選ぶ。`crossMoves` は Group 0 と 2 を互いの下へ移す操作と全部取り込みだけを選ぶ。`reopenUnderDependent` は Group 2 を完了して Issue 1 の dependency にし、片側の完了と他側の `Reopen` を組み合わせる。`reopenAfterInflow` は Issue 1 を終了して Group 0 を完了し、他側で Group 0 に Issue 4 を登録して取り込み、Group 0 の `Reopen` で直す。許可条件・状態更新・検査する性質は変えないため、invariant は `record_integration` のものをそのまま指定します。

### `record_integration_test`

固定した順序の `run` が、孫が着手中の祖父 Group の `Withdraw` の拒否、解決記録だけが先に届いた replica の gap と現在値、終了した Group どうしの循環の取り外しによる修復、解決の選択で作った完了済みの相互依存の dependency 削除による修復、二重登録の解決、cherry-pick による偽の衝突とその解決を確認します。後の 4 本は独立 review が見つけた経路です。実装の独立 review が見つけた経路として、免除の下の `Reconsider`・`Reopen` が違反を増やすときに拒否されること（前提と検査の判定と、action が失敗すること）、依存元からの `Reopen` が違反を減らして通ること、依存元がすでに未完了の依存先を持つ違反にあるときだけ `Completed` の依存元による阻止の免除が働くこと、途中で切れた祖先の連なりの下で `Start` と着手済みの Issue の移動が拒否され（判定と action の失敗）記録が届けばどちらも通ること、循環を待つ Entity どうしの新しい循環が拒否され（判定と action の失敗）待つ側への dependency の追加が通ること、既存の循環への chord と別の循環の構成員どうしで循環を閉じる dependency が部分集合の検査を通っても拒否され（判定と action の失敗）、循環上にない Entity からの追加と辺の除去による修復が通ること、所属変更でも親が新しい子を待つ辺と移動する Group の子孫が継ぐ依存先の辺が、dependency の追加でも子孫が継ぐ辺が循環を閉じれば拒否され（判定と action の失敗）循環上にない Entity の移動は通ること、既存の組と重なる関係でも循環上に載れば拒否されること（判定と action の失敗）、親より先に子孫の記録が届いた Group の登録で子孫が継ぐ辺が循環を閉じれば拒否されること（判定と action の失敗）、共通の祖先の下の兄弟 Group への移動が通ること、循環を待つ Group への登録が通ることを加えています。

## 再現手順

以下は repository root から実行します。`quint run` は bounded random simulation であり、検査の成功は exit status だけでなく、列挙した全 invariant に反例がなく、0 が期待の witness を除く全 witness が出力上1 trace 以上で観測されたことを確認します。通常探索で観測率の低い witness は補助探索が担保するため、`group_lifecycle` と `lifecycle_reachability`、`candidate_evaluation` の `queryStep` と `workAndList`、`record_integration` と `record_integration_paths` は、それぞれ合わせて1つの検査として扱い、全 witness がいずれかの探索で観測されたことを確認します。

seed は固定しません。`quint run` は seed を渡すと再現のために単一スレッドで実行し、渡さないときだけ CPU 数に応じて並列化します。同じ seed を使い続けても、モデルが変わらない限り同じ経路をなぞるだけで新しい情報は得られないため、実行ごとに異なる経路を探索させます。観測される trace 数は実行ごとに変わります。反例が出た場合は `Use --seed=0x… to reproduce.` が出力されるので、その seed を指定すれば同じ反例を再現できます。`--n-threads` は既定で CPU 数になるため指定しません。

invariant と witness はモデルから定義名を読み取って全件を指定します。module 直下の定義だけを読むため、関数の中の局所的な `val` は含めません。

```sh
for f in spec/*.qnt; do quint typecheck "$f"; done
quint test spec/record_integration_test.qnt --match Scenario
```

```python
from pathlib import Path
import re
import subprocess

def names(path, prefix):
    return re.findall(rf"^  val ({prefix}\w+)\s*=", Path(path).read_text(), re.M)

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

ri = "spec/record_integration.qnt"
invariants, witnesses = names(ri, "inv"), names(ri, "w")
main_witnesses = ["wResolved", "wGap", "wGapFilled", "wSpuriousGapConflict", "wPrefixSync", "wSingleSync",
                  "wConverged", "wCompleteVsRelease", "wCancelVsStart", "wTextConflict", "wNotesFromBoth",
                  "wDuplicateCreation", "wDuplicateResolved", "wTerminalGroupGainsChild", "wUnadoptedAncestor",
                  "wRepairedAfterSync", "wWaivedRepair", "wAccept", "wStart", "wComplete", "wReopen", "wCancel",
                  "wReconsider", "wWithdraw", "wRelease", "wEdit", "wMove", "wAddDep", "wRemoveDep", "wCreate",
                  "wGroupCompleted"]
run(ri, 1000, 40, invariants, main_witnesses)
for step, samples, steps in [("divergeAndResolve", 400, 40), ("breakAndRepair", 200, 40),
                             ("crossMoves", 200, 12), ("reopenUnderDependent", 300, 30),
                             ("reopenAfterInflow", 300, 20)]:
    run("spec/record_integration_paths.qnt", samples, steps, invariants, witnesses, "--step", step)
```

`record_integration` の本実行は 1 trace あたりの計算が重く、1,000 traces で数分かかります（実測は「検証結果」）。補助探索が担う witness は本実行の一覧から外し、状態ごとの評価を減らしています。

`group_lifecycle` の witness のうち `wStalledWithoutListedReason`・`wEffectiveWorkingUnderUnadopted`・`wUnadoptedViaComplete`・`wUnadoptedViaMove`・`wUnadoptedViaAcceptOrReopen` の 5 つは 0 trace が期待で、通常探索と補助探索のいずれでも観測されないことを確認します。

## 検証結果

いずれも bounded random simulation の結果であり、全状態の証明でも、必ず完了することの保証でもありません。どのモデルも、完了への到達を強制する公平性は仮定しません。seed を固定しないため、観測される trace 数は実行ごとに変わります。ここに残すのは実行条件と判定で、trace 数は witness の到達しやすさの目安として添えます。

以下は Quint 0.32.0、Rust backend、並列実行、seed 未固定での結果です。`group_lifecycle`・`lifecycle_reachability`・`candidate_evaluation` は 2026-10-09 に、浮上していなくても `axon tasks` に出す Group を着手中の Issue を子孫に持つものに絞った後に再実行し、`issue_lifecycle` は 2026-09-25 に一覧の Group の行の規則（`Empty` の優先、詰まっている理由の導出範囲）を直した後に実行した結果を残しています。`lifecycle_information` は 2026-09-26 に種類の変換と各時点の種類での履歴の再生を足した後に実行しました。`record_integration` と `record_integration_paths` は 2026-09-26 に、所属変更・dependency の追加・登録で新しく持った関係が誘導する辺が循環上に載る変更を違反のある store でも拒否する前提を足した後に実行しました。9モデルの型検査と `record_integration_test` の `--match Scenario` の `run` は 2026-09-26 にいずれも成功し、9モデルの型検査は 2026-10-09 にも成功しました。

- `issue_lifecycle`: 100,000 traces、最大80 steps（約7秒）。9 invariant に反例はなく、23 witness はすべて観測されました。最も少ない完了後の非浮上で37.8%です。
- `group_lifecycle`: 5,000 traces、最大150 steps（約4.5分）。44 invariant に反例はなく、97 witness のうち規則で塞いだ経路と条件に依らない理由のない詰まっている Group を観測する5つが期待どおり0で、残り92をすべて観測しました。最も少ないのは祖先の dependency の完了による着手の解禁で7 traces、次いで `Completed` の依存元による `Reopen` の拒否で10、`Reopen` 後の再着手と孫の着手による祖父の実効 `InProgress` で各12、完了済みの子を持つ Group の `Reopen` で14 traces です。完了済みの子孫だけで実効 `InProgress` の Group が浮上せず行に出ない状態は291、着手中の Issue を子孫に持ち浮上していない Group が行に出る状態は242 traces で観測しました。行に出ない Group での浮上していない祖先の理由は2,396、浮上していない着手可能な子孫の理由は1,020 traces で観測しました。自身の再浮上条件が未成立という理由は、行に出ない Group で4,596、そのうち着手可能な子孫を隠している状態で1,462、完了済みの子孫だけで実効 `InProgress` の Group で208 traces で観測しました。完了できる空の Group の `Empty` は3,412、着手中の子孫がない実効 `InProgress` の行に理由が付く状態は206、完了できない `Empty` の行に理由が付く状態は4,303 traces で観測しました。
- `lifecycle_reachability`: 同じ44 invariant を指定して反例なし。`workAndReopen`・`dependAndStart`・`listAndWork` は各300 traces、最大100 steps、`readoptAfterClose` は5,000 traces、最大60 steps。`workAndReopen` は `Reopen` 後の再着手を295、完了済みの子を持つ Group の `Reopen` を59、`Completed` の依存元による `Reopen` の拒否を298 traces で観測しました。`dependAndStart` は祖先の dependency の完了による着手の解禁を36、自身の依存先の完了による解禁を243 traces で観測しました。`listAndWork` は `Ready` と実効 `InProgress` が重なる Group を276、実効 `InProgress` の Group の移動を300、完了できる空の Group の `Empty` を300 traces で観測しました。`readoptAfterClose` では規則で塞いだ経路の witness は0のままで、完了済みの子孫だけで実効 `InProgress` の Group が浮上せず行に出ない状態を1,358、その Group に自身の再浮上条件が未成立という理由が付く状態を1,162 traces で観測しました。この入口は条件入力を変えず不成立のままなので、着手中の子孫がない実効 `InProgress` の行に理由が付く状態は観測せず、通常探索が担います。通常探索と補助探索を合わせ、0が期待の5つを除く92 witness がいずれかの探索で1 trace 以上に到達しました。
- `candidate_evaluation`: `queryStep` は30,000 traces、最大60 steps（約2.7分）。17 invariant に反例はなく、34 witness のうち `Reopen` した Issue の再表示を除く33を観測しました（未観測の1つは補助入口が担保します）。通常探索で最も少ないのは評価失敗後の着手で5 traces、次いで未成立の結果を持つ進行中の Group の表示で9 traces です。`axon show` の witness は、Group の浮上していない着手可能な子孫の評価を60、祖先の未成立で評価されない Issue を1,817、`axon tasks` の行にならない対象での非評価を29,728、`axon tasks` より狭い範囲を15,356、判定失敗を6,655 traces で観測しました。`workAndList` は3,000 traces、最大60 steps で反例なし、`Reopen` した Issue の再表示を25、評価失敗後の着手を47、未成立の結果を持つ進行中の Group の表示を242、進行中の Group の評価を621 traces で観測しました。両方を合わせ、34 witness はすべて1 trace 以上で観測されました。
- `lifecycle_information`: 100,000 traces、最大60 steps（約33秒）。9 invariant に反例はなく、27 witness はすべて観測されました。最も少ないのは Issue として `Start` し続けて `Release` してから Group へ変換した履歴で2.2%、次いで `Release` で3.0%、`Reopen` 後の文面編集で3.2%、Group として完了し `Reopen` してから Issue へ変換した履歴で4.6%です。各時点の種類では妥当な履歴が現在の種類で判定すると不正になる状態は8.2%で観測しました。
- `record_integration`: 1,000 traces、最大40 steps（約10.5分）。14 invariant に反例はなく、本実行に指定した31 witness のうち30を観測しました。未観測の1つ（完了と解放の衝突）は補助探索で観測しています。最も少ないのは未採用の祖先と免除を使った修復で各2 traces、次いで `Reopen` と取りやめと着手の衝突で各7、終了 Group への子の流入で11 traces です。修復の witness は取り込み後の修復で14 traces でした。gap は193、gap による偽の衝突は79、二重登録の衝突は409、その解決は35、文面の衝突は278 traces で観測しました。
- `record_integration_paths`: 同じ14 invariant を指定して反例なし。`divergeAndResolve` は400 traces、最大40 steps（約1分）で、別 actor の並行 `Start` の衝突5、値の等しい並行記録の衝突338とその解決335、完了と解放の衝突4、未完了側の選択15、解決後の再着手79、解決記録どうしの並行が同値58・異値39、解決の解決87。`crossMoves` は200 traces、最大12 steps で包含の循環76。`reopenUnderDependent` は300 traces、最大30 steps で完了済みへの未完了 dependency の流入44、`Reopen` による修復12。`reopenAfterInflow` は300 traces、最大20 steps で終了 Group への子の流入4と `Reopen` による修復4、完了済みの子を持つ Group の `Reopen` 9。`breakAndRepair` は200 traces、最大40 steps（約40分）で、通常完了経路の循環125、免除を使った修復34、取り込み後の修復31、包含の循環6、未採用の祖先2、完了済みへの未完了 dependency の流入1、`Reopen` による修復1。本実行と補助探索を合わせ、44 witness はすべて1 trace 以上で観測しました。

各モデルの traces 数は、補助探索が担保しない witness が偶然に左右されずに観測される水準を下限とし、そのうえで invariant を叩く厚みを加えて決めています。観測率の低い witness を通常探索の traces 数で拾おうとするより、補助入口を足すほうが確実です。規則を直す前の反例と review の経緯は、コア側は「反例と review から足した規則」、統合側は「統合側で反例と review から足した規則」にまとめています。

## 更新するとき

状態・遷移・初期状態・モデルの対象範囲を変えるときは、該当するモデルを更新して再検証し、確定した意味を [lifecycle](../docs/reference/lifecycle.md)、[候補と外部条件](../docs/reference/candidates.md)、[保存と統合](../docs/reference/storage.md) などの契約文書へ反映してから実装へ進みます。実行条件とその結果はこのファイルの「検証結果」に、時点と条件を明記して残します。手順の全体は [検証方針](../docs/development/verification.md) にあります。
