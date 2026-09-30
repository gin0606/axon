# CLIと表示の契約

この文書は、公開コマンドが受け取る入力と、何をどう表示し、保存結果をどう伝えるかの契約を定める。状態・遷移・包含・種類の変換・dependency・候補集合の意味は [lifecycleと構造の契約](lifecycle.md)、一覧の行と状況の導出と候補一覧の条件評価は [候補と外部条件](candidates.md)、declarationの形式は [計画全体の取得と一括編集](declaration.md)、記録・衝突・違反の意味は [保存と統合の契約](storage.md) に従う。

## 情報を見る目的

採否判断、着手する仕事の選択、着手から完遂、最終確認の四つを、情報を見る目的として扱う。Entityの保存状態とは別の分類であり、四種類の状態や専用画面を追加しない。再開・引継ぎ・計画修正は着手から完遂に含め、採否の再判断が必要なら採否判断へ戻る。

利用者はIDを使って操作するため、表示の先頭はID・種別・状況・label・タイトルとする。内部fieldを並べて利用者に解読させず、その場の判断に必要な内容を短く示す。通常表示へ操作例やコマンド案内を毎回付けず、使い方はhelpへ置く。本文を機械的に要約・分類して判断材料を生成する機能は設けない。

## 識別子と入力

Issue/Groupは共通の `<prefix>-<ランダム8文字>` namespaceを使う。乱数部分は小文字Crockford Base32で紛らわしいi/l/o/uを除く。連番やkind、優先順位の意味を持たせず、同じ保存先で衝突したら再生成する。IDは不透明な文字列として扱い、乱数部分の長さで検証しない。乱数部分が6文字の既存のIDもそのまま有効である。prefixはASCII小文字の `a-z`、数字、ハイフンだけを許し、空、先頭のハイフン、末尾のハイフンは拒否する。Entity ID全体も同じ文字種に限り、保存先の読取とdeclarationの入力で、それ以外の文字を含むIDを拒否する。この文字種に限ることで、完全IDをshellでquoteせずに渡せる。`axon init PREFIX` の明示値は変換せず検証する。省略時は管理rootのdirectory名のASCII大文字を小文字化した結果を使い、規則に合わなければ保存先を作らずに失敗し、`axon init PREFIX` での明示を求める。小文字化以外の自動補正はしない。

全Entity入力は完全IDまたは一意なsuffixを受け付ける。入力が保存済みの完全IDと一致すれば、それが別のIDの末尾であっても、そのEntityに解決する。対象だけでなく`parent`/`needs`も同じ規則。曖昧なときは候補IDを示して拒否し、保存を変更しない。mutationではlock取得後に読んだ記録の集合で解決する。`axon dep rm` の `--needs` は、対象Entityの依存先の中の完全ID、保存済みの完全ID、対象Entityの依存先の中の一意なsuffix、通常の解決の順に解決するため、保存先に存在しない依存先（[依存先の不在の違反](storage.md#構造の違反と修復)）も外せる。依存先の中で曖昧なら拒否する。記録ID（Note IDを含む）は記録の内容のhashで、小文字16進64文字の完全なIDだけを受け付け、suffixでは解決しない（[記録](storage.md#記録)）。

option値の先頭hyphenは `--description='--text'` のように渡す。構文は `axon help <COMMAND PATH>` で確認できる。

## 一覧

一覧の入口は三つとし、候補集合の定義は [lifecycleと構造の契約](lifecycle.md#候補集合) に従う。

| コマンド | 表示対象・役割 |
| --- | --- |
| `axon proposals` | 判断候補のIssue・Group |
| `axon tasks` | 浮上した未着手と着手中のIssue・Groupを並べた平らな一覧 |
| `axon list` | 非浮上・完了・取りやめも含む保存済みEntityを、必要な条件で絞り込む汎用一覧 |

`axon tasks` は依存先の完了待ちや祖先の採用待ちの未着手も含み、着手できるものだけには限定しない。行の集合と状況の導出は [候補と外部条件](candidates.md#一覧の行と状況) に従う。着手中だけを見るには `axon list --lifecycle in-progress` を使う。

`axon list`は保存済み全件を作成日時の昇順、同時刻はID順で表示する。`--kind issue|group`、`--label bug|feat|chore|docs|test|refactor|spike`、`--lifecycle undecided|not-started|in-progress|completed|cancelled`、`--terminal=true|false` はANDで組み合わせる。kindとlabelは現在値、`--lifecycle` は実効値で絞り込むため、配下の仕事が始まったGroupは `in-progress` に当たり `not-started` に当たらない。terminalは`Completed`または`Cancelled`で、着手できることや浮上とは別。`--search` は現在title・本文だけのcase-sensitiveなliteral一致。Unicode正規化やtrimをせず、空文字は構文エラー。%、_、正規表現記号に特殊な意味はない。検索時だけMatchedに該当field（Title、Description）を付記する。

`axon proposals|tasks`は `--kind`・`--label`・`--search` をANDで組み合わせて候補を絞ってから、必要な祖先を含め条件を評価する。`--label` はkindと同じく候補を絞るだけで、評価する条件の範囲の規則は変えない。一回の呼出しで同じ条件を重複評価しない。除外候補の条件は評価しないが、残った候補の祖先ならkindやlabelが異なっても評価し、`axon tasks` は残ったGroupの行の状況を導出するためにその子孫の着手可能なIssueと途中のGroupも評価する。評価失敗時に部分一覧をstdoutへ出さない。時間制限は正整数とms/s/m/hで、既定30s。詳細は [候補と外部条件](candidates.md)。`axon show`も既定で対象の行に必要な範囲の条件を評価し、`--skip-conditions` を付けると評価しない。`axon list`は外部条件コマンドを実行しない。

一覧とGroupの子一覧は、状態ごとに区切らず作成日時の古い順へ統一する。作成日時そのものを通常の各行へ表示する必要はない。同時刻はID順で安定させる。

通常行は `ID  Kind  Situation  Label  Title`。Kindは現在の種類、Labelは現在の [label](lifecycle.md#label) を示す。状況欄は保存された lifecycle だけの表示ではなく、保存状態と構造から導出する短い表現とする。状況の定義は [候補と外部条件](candidates.md#一覧の行と状況) に従い、Issueは次の表記を使う。

| 状況 | 保存状態・前提 | 意味 |
| --- | --- | --- |
| `Undecided` | `Undecided` | 未判断 |
| `Ready` | `NotStarted` で `Start` の前提を満たす | 着手できる |
| `Blocked` | `NotStarted` で `Start` の前提が不足 | 祖先の採用待ちまたは依存先の完了待ち |
| `Unsurfaced` | `NotStarted` で、自身または祖先の再浮上条件が未成立、または終了した祖先の配下にある | 浮上しておらず候補にならない。`Start` の前提の充足・不足より優先し、条件を評価した `axon show` だけが示す |
| `InProgress` | `InProgress` でdependencyが充足 | 着手中 |
| `InProgress+Blocked` | `InProgress` で未完了の依存先がある | 着手中で、完了に必要な依存先が残る |
| `Completed` | `Completed` | 完了 |
| `Cancelled` | `Cancelled` | 取りやめ |
| `Conflicted` | headが複数ある | 衝突中で現在値がない。他のすべての状況より優先する |

Groupは、衝突中なら `Conflicted`、保存値が `Undecided`・`Completed`・`Cancelled` ならその状態名を示し、保存値が `NotStarted` なら配下から導出した `Empty`・`Confirmable`・`Ready`・`InProgress`・`Blocked` のいずれかを示す。GroupにはIssueの `InProgress+Blocked` に当たる複合表示を設けず、Groupの状況欄に `Unsurfaced` も設けない。`axon tasks` と `axon show` は評価した再浮上条件を使って着手候補の子孫から `Ready` を決め、条件を評価しない `axon list` と `axon show --skip-conditions` は着手可能な子孫から決める。このため、浮上していない着手可能なIssueだけを配下に持つGroupは、`axon tasks`・`axon show` では `Blocked`、`axon list`・`axon show --skip-conditions` では `Ready` になる。`Undecided` のEntityの状況欄は浮上の有無にかかわらず `Undecided` のままにする。衝突中のEntityは現在値を持たないため、`axon proposals`・`axon tasks` の行にならず、`axon list` と `axon show` で `Conflicted` として読む。Groupの行の状況と子孫の集計では衝突中の子孫を存在しないものとして導出し、衝突中の祖先は採用済みでない祖先として扱う（配下のIssueは `Blocked`）。違反に含まれるEntity（[構造の違反と修復](storage.md#構造の違反と修復)）は、Issue・Groupとも状況の後ろに `+Invalid` を付す（`Completed+Invalid`、`InProgress+Blocked+Invalid` など）。衝突中のEntityの行のlabelとタイトルは、それぞれheadの値が一致すればその値、異なれば記録ID順で最初のheadの値を示す。衝突・違反・記録の欠けのある保存先を読む一覧と詳細は、件数と `axon storage check` への案内をstderrに一行で示す。

祖先の採用待ちも`Blocked`に含める。これは表示上のまとめ方で、保存する包含と明示dependencyの区別は維持する。Groupの未終了の子は最終確認前の進捗として子一覧へ示し、明示dependencyと混ぜない。一行の中に表示する値（タイトル、理由、actor、条件コマンド、親のタイトル）に含まれる改行は `\n` として表示し、保存された文字列が記録の行や節の見出しを装えないようにする。

```text
demo-k3m7pq2a  Group  Ready  feat  検索画面を実装する
demo-8bxw2r7n  Issue  InProgress+Blocked  feat  検索APIを実装する
demo-c9d4ts5e  Issue  Ready  feat  検索フォームを実装する
demo-9f2hjx8w  Issue  Blocked  feat  検索結果を表示する
demo-r5w8kn3d  Group  Empty  docs  検索の運用手順を整える
```

例は架空の内容である。列の間隔やグルーピングは実装が決め、固定列や機械向けの出力形式は保証しない。

## `axon show` と待ち理由

`axon show ID`は、一覧と同じID・種別・状況・label・タイトルの先頭行、Note件数、所属計画のID・タイトル、本文を基本とする。状況は `axon tasks` の行と同じく、対象の行に必要な範囲の再浮上条件を評価して導出する。評価範囲、関連するEntityの行の扱い、判定失敗の扱いは [候補と外部条件](candidates.md#評価契約) に定める。`--skip-conditions` は条件を評価せず、条件をすべて成立したものとして状況を導出する。`--condition-timeout` と `--trace-conditions` は `axon tasks` の同名optionと同じ意味で、`--trace-conditions` は `--skip-conditions` と併用できない。`--skip-conditions` を付けたときの `--condition-timeout` は、実行する条件がないため効果を持たない。判定失敗は表示全体を失敗させ、失敗したEntityと条件、および `axon show ID --skip-conditions` で保存情報を読めることを診断に示す。本文は保存された内容を、各行を2 spaceで字下げして表示する。IDや節の見出しなど構造を表す行は行頭から始まり、利用者の複数行の文章は行頭から始まらないので、本文が節や記録の行を装うことはない。Note本文、履歴、内部のcausal情報、不要な設定・件数の羅列、操作コマンドの案内は通常表示から外す。Noteがあれば `5 notes` のように存在を示し、0件ならその表示を省略する。

未充足の前提がある場合だけ、本文の前へ `Required to start` または `Required to complete` の節を置く。`Required to start` はIssueだけに置く。満たされていない前提を `Ancestor must be adopted:`（保存値が `NotStarted` でない祖先と、祖先の連なりの末尾で衝突中または保存先に無い祖先）・`Dependency must complete:`（自身の未完了の依存先）・`Ancestor dependency must complete:`（祖先の未完了の依存先）として、一覧と同じID・種別・現在の状況・label・タイトルの行で示す。保存先に無い祖先と依存先は行を持たないため、IDの後に `(missing)` を付けて示す。祖先の条件が未成立のためIssueが浮上していない場合は、同じ節に `Unsurfaced ancestor:` として、条件が未成立の祖先Groupの行を示す。これは候補にならない理由であり、`Start` の前提ではない（[再浮上条件](lifecycle.md#候補集合) はIDを明示した操作を縛らない）。祖先は上から評価して未成立の祖先より下は評価しないため、この行は一つになる。終了した祖先の配下のIssueも浮上せず状況欄は `Unsurfaced` になるが、終了した祖先は条件を評価しないため `Unsurfaced ancestor:` には示さず、`Ancestor must be adopted:` で示す。Issue自身の条件が未成立の場合は状況欄の `Unsurfaced` だけで示し、条件は `--details` で読む。`Required to complete` は同じ語で、全祖先の採用と自身の依存先を示す。Groupの未終了の子は全子孫のツリーで読めるため、この節に再列挙しない。満たされた依存や依存先の先のツリーは常時展開しない。親の所属表示と待ち理由が同じ情報になる場合は、重複を避けて配置する。

Groupの場合は、この共通表示の末尾へ短い集計と全子孫のツリーをこの順で加える。`Completed`・`Cancelled`も含めて全階層を展開する。各行は一覧と同じID・種別・状況・label・タイトルとし、兄弟を作成日時順（同時刻はID順）で揃え、別のDependencies節へ同じ情報を再列挙しない。

集計は `Descendants: 2/4 terminal (1 completed, 1 cancelled)` のように、全子孫の終了数と完了・取りやめの違いが読める形とする。集計はGroup・Issueの両方を含み、対象自身を除く。全状態の内訳は羅列しない。Groupの状況が `Confirmable` なら `Awaiting final confirmation` を示す。これは導出される案内であり、保存状態やレビュー済み状態を追加しない。

保存値が `NotStarted` のGroupが詰まっている場合は、本文の前へ `Stalled` の節を置き、[詰まっている理由](candidates.md#詰まっている-group-と理由) を、該当するEntityの行とともに示す。未完了のdependencyは、Group自身の依存先を `Dependency must complete:`、祖先の依存先を `Ancestor dependency must complete:`、終了していない子孫の依存先を `Descendant dependency must complete:` とし、最後のものは依存先を待つ子孫の行と依存先の行を並べる。`Undecided` の子は `Undecided child:`、未終了の子Groupは `Open subgroup:`、浮上していない着手可能な子孫は `Unsurfaced candidate:`（行はそのIssue）、自身の再浮上条件が未成立は `Own condition unsatisfied:`（行は当のGroup自身）、`Undecided` の祖先は `Undecided ancestor:`、終了した祖先と祖先の連なりの末尾で衝突中または保存先に無い祖先は `Ancestor must be adopted:`、浮上していない祖先は `Unsurfaced ancestor:`（行は条件が未成立の祖先Group。終了した祖先は含まない）とする。いずれも一覧と同じID・種別・現在の状況・label・タイトルの行で示し、保存先に無い依存先と祖先はIDと `(missing)` で示す。`--skip-conditions` では条件をすべて成立とみなすため、`Unsurfaced candidate:`・`Own condition unsatisfied:`・`Unsurfaced ancestor:` は現れない。`Stalled` の節を置くGroupには `Required to complete` の節を置かない。自身の依存先、`Undecided` の祖先、終了した祖先、祖先の連なりの末尾で衝突中または保存先に無い祖先は `Stalled` の中で示す。詰まっているGroupの `Stalled` には理由が一つ以上ある。

衝突中のEntityの `axon show` は本文の前へ `Conflicted` の節を置き、`axon resolve ID` と同じheadの一覧を示す。本文と所属は記録ID順で最初のheadの値を示し、待ち理由と `Stalled` の節は置かない。違反に含まれるEntityは `Invalid` の節に自身の違反を種類ごとに一行で示し、関係するEntity（存在しない親・依存先のID、終了した親、未採用の祖先、未完了の依存先、循環に含まれるEntity）を添える。

`axon show ID --details` は保存情報の明示的な詳細入口。通常の待ち理由節を保存情報の詳細へ置き換え、親・条件・全直接dependency・直接dependentを取得する。状況欄の導出は `--details` の有無で変わらず、評価を避けるには `--skip-conditions` を併せて付ける。同じ関係を複数の節へ重複して列挙しない。Issueでは状況と異なる場合だけ保存lifecycleを `Lifecycle:` として別記する。Groupでは `Lifecycle:` に実効値を示し、保存値と異なれば `Lifecycle: InProgress (stored NotStarted)` のように保存値を併記する。条件未設定、親なし、空の依存集合も `(none)` と明示する。Groupの全子孫ツリーは通常表示と同様に表示する。

## Noteと履歴

`axon note list ID` は指定EntityのNote本文を全文で、各行を2 spaceで字下げして、日時・actor・記録ID（Note ID）とともに日時順に表示する。Noteは因果を持たないため、同じ日時のNoteは記録ID順で固定する。`axon note show ID NOTE_ID` は同じEntityの個別Noteの原文を、字下げせずに取得する。`axon note add ID …` は追記し、編集・削除は設けない。衝突中のEntityにもNoteを追加できる。

`axon log ID` はEntityの記録（Note以外）を読む入口とする。lifecycle遷移は変更前後の状態、日時、記録者、任意の理由を人が読める形で示す。文面編集・所属変更・dependencyの増減・条件の設定と解除も一行ずつ示す。文面編集は変わった項目を `Edited: title` のように示し、titleとdescriptionのどちらも変えない記録は `Edited: no field changes` とし、理由があればlifecycle遷移と同じく `Reason:` に続けて添える。labelの設定は `Label set: bug` のように設定後の値を示す。`axon import apply` が書いた記録は `Declaration applied` として変わった項目（`label` を含む）を示す。種類の変換はlifecycle遷移と区別し、`Converted: Issue → Group` のように変換前後の種類とともに示す。解決記録は `Resolved` として、採ったheadの記録IDと、その現在値のlifecycle・種類・labelを示し、退けたheadは通常表示に列挙しない。記録は因果順で、並行する分岐は枝ごとにまとめて並べる。保存先にある親をすべて示し終えた記録を示せる記録とし、直前の記録を親に持つ示せる記録があればそれを続ける。無ければ、示せる子が残っている記録のうち最後に示したもの（直近の分岐点）へ戻ってその子を続けるので、枝の途中で分かれた分岐は、その分岐点から示せる記録を示し終えてから、より前の分岐点から分かれた枝へ移る。戻れる分岐点も無ければ、履歴が作成より前から始まって見えないよう、作成の記録を、親記録が保存先にない記録より先に示す。いずれの段でも候補が複数なら記録IDの小さいものを先にするので、並びは記録と親の関係だけで決まり、保存先の file の並びに依存しない。直前の記録と先後関係がない箇所、つまり枝の切り替わりには `Concurrent branch` と表示し、並行する分岐を時刻で逐次操作へ並べ替えない。親記録が保存先にない記録には `parent missing` を付し、変更前の状態は不明として示す。Entityが衝突中なら末尾にheadの数を示す。内部の因果辺や記録IDの羅列を通常表示へ出さない。

`axon note list ID --recorder-details`・`axon log ID --recorder-details` は保存済みdataを併記し、通常表示はactorのみとする。`axon actor` は現在環境で検出できたactor、未取得なら `—` を表示し、保存を行わない。取得の契約は [記録者連携](../development/lifecycle-recorder.md) を参照する。

最終確認は、Groupと必要な子の`axon show`と、それぞれのNoteを読む操作を組み合わせる。Noteから成果・検証結果を自動抽出しない。

## Note本文の横断検索

`axon note search <語句>` は管理root内の全Entity（Issue・Group、`Completed`・`Cancelled`を含む）のNote本文を検索し、条件を実行しない。追加filterは設けない。元の保存文字列にcase-sensitiveなliteral部分一致を適用し、trim・Unicode正規化をしない。空白・改行・%・_・正規表現記号は通常の文字として扱う。空文字は構文エラー（終了2）、該当なしはstdout空・stderrに案内を出して終了0。先頭hyphenの語句は `axon note search -- '--text'` と渡す。

同じNote内の複数一致も1 Note＝1行とし、所属Entityの完全ID、完全なNote ID、既存のlocal日時と数値UTC offset、`Excerpt:` 付き抜粋を示す。Entityは`axon list`と同じ作成日時昇順・同時刻ID順、Entity内は`axon note list`と同じ日時順・同時刻は記録ID順で並べる。見出し・空行・分岐説明行は加えない。

抜粋は最初の一致と前後24文字で、検索語そのものを省略せず、日本語を壊さない文字単位で切り出す。原文を省略した側に `…` を示す。原文で一致位置と範囲を決めた後、改行を可視の `\n`、元のバックスラッシュを `\\` にし、他の端末制御文字も可視化する。保存本文は変えない。長い検索語でも一行の固定上限で切り捨てない。行単位での絞り込み向けで、固定列・区切りや機械向け出力形式は保証しない。

`axon list|tasks|proposals`の `--search` は現在のtitle・本文だけに一致し、Noteだけの一致ではEntityを返さない。現在の主題は `axon list --search`、Noteに残る情報は `axon note search`、原文は `axon note show ID NOTE_ID` で読む。`axon note list ID` のEntity IDは必須。

## 登録・状態変更・編集

以下を公開コマンドの基本構成とする。`A` は操作対象、`B` は依存先、`G` は親GroupのIDを表す。

| 操作 | コマンド |
| --- | --- |
| 未判断のIssue / Groupを登録 | `axon capture …` / `axon capture --kind group …` |
| 採用済みのIssue / Groupを登録 | `axon capture --accept …` / `axon capture --kind group --accept …` |
| 採用 | `axon accept A` |
| 採用撤回 | `axon withdraw A` |
| 着手（Issueのみ） | `axon start A` |
| 作業を解放（Issueのみ） | `axon release A` |
| 完了 | `axon complete A` |
| 取りやめ | `axon cancel A` |
| 再検討 | `axon reconsider A` |
| 完了の取消・再開 | `axon reopen A` |
| 種類を変換 | `axon convert A --kind group` / `axon convert A --kind issue` |
| タイトル・本文を編集 | `axon write A …` |
| labelを変更 | `axon label set A VALUE` |
| 親Groupを設定・変更 / 解除 | `axon parent set A --parent G` / `axon parent unset A` |
| 依存先を追加 / 解除 | `axon dep add A --needs B` / `axon dep rm A --needs B` |
| 再浮上条件を設定 / 解除 | `axon condition set A --command '条件コマンド'` / `axon condition unset A` |

登録は一つのコマンドで行い、種別は `--kind issue|group`、採否は `--accept` の有無で指定する。`--kind` の既定は `issue`、`--accept` 省略時は`Undecided`、指定時は`NotStarted`で作成する。登録と採否は直交し、作成後は同じIDベースの操作を使う。操作対象のIDは位置引数、関係先のIDは役割を明示するoptionとし、登録時の親・依存指定も `--parent`・`--needs` に揃える。

登録時のlabelは `--label VALUE` で必ず指定する。値は [label](lifecycle.md#label) の7値のどれかで、省略と集合外の値はClapの構文エラー（終了2）とし、保存を変えない。`axon label set A VALUE` の値も同じ規則で検査する。labelの解除はない。

登録titleは `--title`。タイトルは一行の値で、改行や制御文字を含む値と200文字を超える値を拒否する。本文は `-m/--description` または `-F/--file`。Noteは `-m/--message` または `-F/--file`。本文option同士は排他で、`-F -` はUTF-8のstdinを一度読む。本文・Noteをtrimして保存しない。初期の `--parent`、反復可能な `--needs`、`--command` は作成と同時に検査・保存する。作成中に条件を実行しない。通常の`axon write`はtitleと本文を一transactionで編集し、lifecycleを変えない。長い本文は登録時と同じ本文optionでファイルから渡せる。

`axon accept|withdraw|start|release|complete|cancel|reconsider|reopen A` は `-r/--reason` を履歴へ保存する。理由も一行の値で、空白だけの値、改行や制御文字を含む値、500文字を超える値を拒否する。文字数はUnicodeの文字単位で数える。この検証は保存先の読取でも行う。複数行の説明や長い内容は本文かNoteに書く。Groupへの`axon complete`の実行自体を「計画全体の最終確認が通った」という明示入力とする。Axonは子・依存・状態を検査し、確認作業は呼び出す人・エージェントのskillと運用で担う。必須のレビュー確認フラグや独立したレビュー済み状態は設けない。どの変更コマンドも、状態・包含・dependencyの制約を迂回しない。

Groupへの`axon start`・`axon release`は拒否し、Groupは配下のIssueへの`axon start`で着手済みになること、保存値を変える操作がないことを診断で示す。`axon reopen A` は `Completed` のIssue・Groupを `NotStarted` へ戻し、子や依存元のlifecycleを変えない。`Completed` の依存元が残る場合は、その依存元を示して拒否する。

`axon convert A --kind issue|group` はEntityの種類を変換し、lifecycle・所属・dependency・文面・label・条件・Noteを変えない。前提は [種類の変換](lifecycle.md#種類の変換) に従い、子を持つGroupのIssueへの変換は子を示して、`InProgress` のIssueのGroupへの変換は先に`axon release`が必要であることを示して拒否する。lifecycle遷移ではないため `-r/--reason` を受け付けない。

## 衝突・違反と解決

衝突・違反・記録の欠け（gap）・保存先の破損の意味は [保存と統合の契約](storage.md) に従う。

`axon storage check [ROOT]` は、引数なしでは探索で確定した保存先（探索がheaderのない `.axon` で止まればその保存先をheaderの欠落として報告する）、`ROOT` を与えればその管理rootを探索せずに検査し（`ROOT` の指定が省くのは探索だけで、Gitの扱いは探索と同じ。`ROOT` がGit worktreeの中にあれば、Git indexで `.axon/` の下のpathがunmergedなら引数なしと同じ診断で終了1、探索が拒否するGitの境界（作業treeのないrepository、`.git` の中など）ならその診断で終了1。Gitの外ならindexを検査しない）、破損・衝突・違反・gapを種類ごとに一行ずつ示す。破損は `.axon/records/` からの相対pathと理由（記録IDの形でない名前、名前と内容のhashの不一致、名前と違うsubdirectory、途中で切れた・空の内容、読めないJSONと規則外の内容、headerの欠落・未知のformat。内容のCRLFをLFに戻すと名前のhashと一致するfileは、理由にその旨を書き、報告の末尾に改行変換の可能性と [保存先とworktree](../guide/storage.md#改行変換と-axongitattributes) への案内を一行添える）、衝突はEntityの行とheadの数、違反はEntityの行と種類、gapはEntityの行と親の欠けた記録ID（記録のないEntityのNoteも同じ節に示す）を示す。名前が `.tmp` で終わるfileは報告しない。破損があれば記録から導出する検査は行わない。headerのformatが変換の要る以前のformat（`axon-records/v1`）なら、破損とは別に、記録を読まずに変換が要ることだけを示して終了1とする（[保存先の破損](storage.md#保存先の破損)）。破損・衝突・違反のいずれかがあれば終了1、gapだけなら情報として示して終了0、何もなければ短い確認をstderrに出して終了0。Git indexで `.axon/` の下のpathがunmergedなら、`ROOT` の有無によらず、headerの有無の判定と破損の検査より先にunmergedの診断だけを示して終了1にする。診断はそのindexを持つworktreeのpathと、unmergedなpath（worktreeからの相対path、一行に一つ）を示し、headerが作業treeになくてもheaderの欠落を破損として示さず、headerの有無と破損は解決後の検査で示す。通常操作も同じ順序で検査し、同じ診断で拒否する（[探索](storage.md#探索)）。保存先を変更せず、条件コマンドを実行しない。

`axon resolve` は衝突中の全Entityを作成日時順（同時刻はID順）に、`axon resolve ID` は指定したEntityを対象に、Entityの行と各headを示す。衝突中のEntityがなければ一覧0件と同じくstdoutに行を出さず、短い案内をstderrへ出して終了0。headの行は記録ID、日時、actor、記録の種類（lifecycle遷移なら操作名）、その現在値のlifecycle・種類・label・タイトルを持ち、Entityがgapを持つとき、自身の親記録が保存先にないhead、または親をたどってもそのEntityの最も古い記録（作成の記録、なければ親記録がすべて欠けた記録のうち日時が最も早いもの）に届かないheadには `parent missing; likely newer`（親記録が欠けていて新しい可能性が高い）を付す。衝突していないEntityを指定した場合は、衝突していないことを示して終了1。保存先を変更しない。

`axon resolve ID --head RECORD_ID` は指定したheadの現在値を採る解決記録を書く。`RECORD_ID` は対象Entityのheadの完全な記録IDで、headでなければ拒否する。`-r/--reason` は他の状態変更と同じ規則で記録に保存する。成功出力は完全なEntity IDが先頭で、採ったheadと解決後の状況（他の変更コマンドと同じく条件は評価せず、違反に含まれれば `+Invalid` を付す）を短く示す。解決の後に残る違反は `axon show` の `Invalid` と `axon storage check` で読み、通常操作で直す。

衝突中のEntityが一つでもある保存先では、`axon resolve` と `axon note add` 以外の変更コマンドを拒否し、衝突中のEntityのIDと `axon resolve` を診断に示す。破損のある保存先では読取を含む全コマンド（`axon storage check`、および保存先を開かないコマンドを除く）を拒否し、破損したfileのpathと理由を診断に示す。headerのformatが変換の要る以前のformatの保存先でも、同じ範囲のコマンドを、変換が要ることを示して拒否する。改行変換が疑われるfile（[保存先の破損](storage.md#保存先の破損)）があれば、`axon storage check` と同じ案内を診断の末尾に添える。

`axon init [PREFIX]` は `.axon/records/`、`.axon/header.json`、`.axon/.gitignore`、`* -text` の1行だけの `.axon/.gitattributes`（記録fileをGitの改行変換から外す。mergeやunionの属性は書かない。[保存と統合の契約](storage.md#保存先と初期化)）を作り、成功時は作成したheaderのpathを示す。Git内ではさらに、保存先がuntrackedに見えること、無視する運用（`.git/info/exclude` などに `.axon/` を書く）と追跡する運用（`git add .axon` してcommitする）の手順、Axonの状態の取り消しにrevertを使わないことを表示する。repository rootのfileとGit configを作成も編集もせず、stage・commitもしない。`.axon/` に中断した初期化の残骸（lock、`.tmp` で終わるfile、空の記録のdirectory、CRLFをLFと読んで同じ内容の `.gitignore` と `.gitattributes`。CRLFのものは `axon init` が書く内容で置き換える）以外の何か（header、記録、内容の異なる `.gitignore` か `.gitattributes`、以前の形式のfile）があれば拒否し、そのpathを示す。

## 計画全体の取得と一括編集

`axon export` と `axon import prepare|check|apply`、`axon docs declaration` が扱うdeclarationの形式、識別子、競合判定、拒否する入力は [計画全体の取得と一括編集](declaration.md) に従う。declaration内のIDは完全IDだけを使い、`axon export`の引数は他のcommandと同じくsuffixも受け付ける。保存境界の表示はこの文書の「mutationの結果」と同じApplied、Not applied、Result unknownを使う。

`axon export` と `axon import prepare|check|apply` のhelpは、代表例と次に実行するcommandを保存先を開かずに表示する。

- `axon export ID...` は完全 ID または一意な suffix を一つ以上受け取り、Issue 単体または Group 全子孫の和集合を canonical YAML として stdout に出す。保存先を変更せず、条件を実行しない。
- `axon docs declaration` は field と新規・既存の違い、`axon import prepare` → `axon import check` → `axon import apply` → 再度`axon import check` の手順を stdout に説明する。`--example` は新規計画の canonical YAML だけを stdout に出す。どちらも保存先を開かない。引数なしの `axon docs` は状態モデルと基本workflowの説明、および declaration への案内を返す。

`axon import prepare FILE` は新規IDを割り当て、外部参照を再生成したcanonical YAMLで同じfileを置き換える。保存先は変更せず、成功時はfile名と保存先未変更、新規recordの `key -> 完全ID` の対応を一行ずつ表示する。書込前の失敗・bytes競合はNot applied、rename後の同期失敗はResult unknownとして診断し、残ったtemporary fileは診断で案内する。更新後にstdout出力だけが失敗した場合も、declarationがAppliedで保存先は未変更であることを示す。

`axon import check FILE` は全IDが確定したcanonical YAMLを要求し、違えば`axon import prepare`を案内する。schema・identityと参照・読み取り専用項目・競合・共通コアの拒否を区別し、作成、titleの前後、descriptionの変更有無、labelの前後、parentの前後、needsの増減、差分なしと適用後の状況をEntityごとに表示する。titleは`axon list`と同じく改行を `\n`、制御文字を可視escapeにして一行で表示する。条件は実行せず、fileと保存先を変更しない。

`axon import apply FILE` は書き込みlock内でFILEを読み、`axon import check`と同じ検証を再実行し、全変更を一回のlockの下で反映する。拒否時は全件Not applied。記録の公開の途中でrenameが失敗した場合はResult unknownと診断し、processが失われた場合も結果不明として扱い、再度の`axon import apply`がEntityごとに残りを反映する。成功後は保存した記録の集合からbase・lifecycle・referencesとcanonical順を更新し、keyを保持してFILEを置き換える。成功出力にはbase更新前に新規だったrecordの `key -> 完全ID` の対応を一行ずつ含める。既存の再浮上条件とNoteは保持し、新規の条件は未設定とする。

保存成功後のFILE更新失敗は、保存先のAppliedとdeclarationのNot appliedまたはResult unknownを分けて表示する。rename直前に元bytesを再照合し、編集されていればそのfileを保持する。同じFILEを再度`axon import apply`すると、Entityごとに最終値に一致するものを適用済み、`base` に一致するものを未適用として残りを反映し、全件が適用済みなら保存先はno-opでrewriteだけを完了する。どちらにも一致しないEntityがあれば競合。記録fileのrename後のsync失敗は保存先のResult unknownで、declarationは更新しない。

## 表示とstream

CLIが生成するhelp・ラベル・診断は英語。利用者のタイトル・本文・Note・理由は原文を保持する。human時刻はlocal時刻と数値UTC offset。C0/C1/ESC、tab、CRは可視escapeし、本文のUnicodeと改行は保持する。

装飾は対象streamがTTYでNO_COLORが存在しない場合だけ。同じ内容からANSIを除けば非TTYとテキスト・順序・空白が一致する。IDと着手はcyan＋bold、見出しはbold、成功/Readyはgreen、待ちはyellow、未判断はyellow＋bold、kind・terminal・no-op・補助情報はdim、エラーはred＋bold。ユーザー本文・タイトルは着色せず、色だけを意味の手掛かりにしない。`axon completion`は常に装飾なし。

一覧0件はstdoutに行を出さず、短い案内をstderrへ出して終了0。候補不在から保存情報の不存在を推測しない。通常行へ毎回操作例を付けず、helpと`axon docs`へ使い方を分ける。

引数なしの `axon`、`axon help`、`axon -h`、`axon --help`は同じ用途別root helpをstdoutへ出して終了0。leaf help、`axon docs`、`axon actor`、`--version`、`axon completion`は保存先を開かず取得できる。`axon docs`はbinary同梱の端末用説明で、ソースcheckoutやネットワークへ依存しない。`--version`はpackage versionだけを出し、実行directoryやGitから由来を推測しない。

## mutationの結果

成功確認は完全IDが先頭。Created、Note ID recorded、状態遷移の結果、実際に変わったtitle/本文/label/parent/dependency/conditionを短く示す。保存処理が返した結果を使い、lock前の読取から更新を推定しない。`axon write`・`axon label set`・関係・条件・種類の変換の同値操作はNo changesの成功で、保存状態・履歴を変えない。例外として、終了した（`Completed`・`Cancelled`）Entityへの `axon write` と `axon label set` は、指定した値が現在の値と同じでも文面とlabelが固定されていることを理由に拒否する。現在と同じ種類への `axon convert` は、変換の前提を検査せずNo changesとする。同値lifecycle遷移は拒否される。

成功はstdout/終了0、アプリケーションの拒否・失敗はError:を含むstderr/終了1。Clapの構文エラーは既定のerror:/Usage構造と終了2。原因、判明している対象・操作を示し、曖昧なFailedだけで済ませない。

保存境界に誤解の余地がある場合はApplied、Not applied、Result unknownを区別する。記録fileのrename後の同期失敗は再読まで結果不明。保存成功後の出力障害は適用済みを明示し、作成・Noteを盲目的に再送させない。stdoutのBrokenPipeは成功として扱うが、条件traceのstderr障害は一覧失敗。部分適用された複数command列の前段成功を後段失敗で未適用と説明しない。
