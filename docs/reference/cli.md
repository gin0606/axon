# CLIと表示の契約

この文書は、公開コマンドが受け取る識別子と値の検証、skill や script が読む表示の語、条件の許可の表記、現在の記録者の表示、保存結果と終了コード、外から届く文字列の表示の契約を定める。状態・遷移・包含・種類の変換・dependency・候補集合の意味は [lifecycleと構造の契約](lifecycle.md)、一覧の行と状況の導出と条件の評価は [候補と外部条件](candidates.md)、declarationの形式と `axon export`・`axon import prepare|check|apply` は [計画全体の取得と一括編集](declaration.md)、記録・衝突・違反と `axon storage check`・`axon resolve`・`axon init` は [保存と統合の契約](storage.md) に従う。使い方と構文は `axon help` と `axon docs` が示す。

## 識別子と入力

Issue/Groupは共通の `<prefix>-<ランダム8文字>` namespaceを使う。乱数部分は小文字Crockford Base32で紛らわしいi/l/o/uを除く。連番やkind、優先順位の意味を持たせず、同じ保存先で衝突したら再生成する。IDの生成規則と参照時の検証を分け、保存済みIDは不透明な文字列として扱い、乱数部分の長さで検証しない。prefixはASCII小文字の `a-z`、数字、ハイフンだけを許し、空、先頭のハイフン、末尾のハイフンは拒否する。Entity ID全体も同じ文字種に限り、保存先の読取とdeclarationの入力で、それ以外の文字を含むIDを拒否する。この文字種に限ることで、完全IDをshellでquoteせずに渡せる。`axon init PREFIX` の明示値は変換せず検証する。省略時は管理rootのdirectory名のASCII大文字を小文字化した結果を使い、規則に合わなければ保存先を作らずに失敗する。

全Entity入力は完全IDまたは一意なsuffixを受け付ける。入力が保存済みの完全IDと一致すれば、それが別のIDの末尾であっても、そのEntityに解決する。対象だけでなく`parent`/`needs`も同じ規則。曖昧なときは候補IDを示して拒否し、保存を変更しない。mutationではlock取得後に読んだ記録の集合で解決する。`axon dep rm` の `--needs` は、対象Entityの依存先の中の完全ID、保存済みの完全ID、対象Entityの依存先の中の一意なsuffix、通常の解決の順に解決するため、保存先に存在しない依存先（[依存先の不在の違反](storage.md#構造の違反と修復)）も外せる。依存先の中で曖昧なら拒否する。`axon condition allow` は例外で、完全IDだけを受け付ける（[再浮上条件の許可](#再浮上条件の許可)）。記録ID（Note IDを含む）は記録の内容のhashで、小文字16進64文字の完全なIDだけを受け付け、suffixでは解決しない（[記録](storage.md#記録)）。

## タイトルと理由

タイトルは一行の値で、空白だけの値、改行や制御文字を含む値、200文字を超える値を拒否する。`-r/--reason` の理由も一行の値で、空白だけの値、改行や制御文字を含む値、500文字を超える値を拒否する。文字数はUnicodeの文字単位で数える。この検証は保存先の読取でも行うため、上限を狭めると既存の記録が読めなくなる。

保存形式は Note を含む全記録で任意の reason を許し、既存値を拒否・削除しない（[設計判断](../design/decisions.md#単独変更に操作と一体の理由を添える理由)）。

## 一覧

通常行は `ID  Kind  Situation  Label  Title` の順に並べる。状況欄は保存された lifecycle だけの表示ではなく、保存状態と構造から導出する（[候補と外部条件](candidates.md#一覧の行と状況)）。Issueは次の表記を使う。

| 状況 | 保存状態・前提 |
| --- | --- |
| `Undecided` | `Undecided` |
| `Ready` | `NotStarted` で `Start` の前提を満たす |
| `Blocked` | `NotStarted` で `Start` の前提が不足 |
| `Unsurfaced` | `NotStarted` で、自身または祖先の再浮上条件が未成立、または終了した祖先の配下にある。`Start` の前提の充足・不足より優先し、条件を評価した `axon show` だけが示す |
| `InProgress` | `InProgress` でdependencyが充足 |
| `InProgress+Blocked` | `InProgress` で未完了の依存先がある |
| `Completed` | `Completed` |
| `Cancelled` | `Cancelled` |
| `Conflicted` | headが複数ある。他のすべての状況より優先する |

Groupは、衝突中なら `Conflicted`、保存値が `Undecided`・`Completed`・`Cancelled` ならその状態名、保存値が `NotStarted` なら配下から導出した `Empty`・`Confirmable`・`Ready`・`InProgress`・`Blocked` のいずれかを示す。GroupにはIssueの `InProgress+Blocked` に当たる複合表示と `Unsurfaced` を設けない。

違反に含まれるEntity（[構造の違反と修復](storage.md#構造の違反と修復)）は、状況の後ろに `+Invalid` を付す。`axon proposals`・`axon tasks`・`axon show` は、その呼び出しで評価の範囲に入り自身の条件が [未許可](candidates.md#未許可の条件の評価) だったEntityの状況の後ろに `+Unapproved` を付す。両方が当たれば `+Unapproved` を先に置く（`Blocked+Unapproved+Invalid` など）。条件を評価しない `axon list`、`axon show --skip-conditions`、`axon condition unapproved` の行と、評価しない `axon show` の対象（`Undecided`・`InProgress`・終了したEntity）には `+Unapproved` を付けない。

列の間隔、端末での列揃えと折り返しは実装が決め、固定列や機械向けの出力形式は保証しない。

`axon list`、`axon show --skip-conditions`、`axon log`、`axon note list|show|search`、`axon condition unapproved|allow`、`axon storage check`、`axon resolve`、`axon export`、`axon import prepare|check|apply` は再浮上条件のコマンドを実行しない。

## `axon show` と待ち理由

`axon show ID` は、対象の行に必要な範囲の再浮上条件を評価して状況を導出する（[評価契約](candidates.md#評価契約)）。`--skip-conditions` は条件を実行せず、条件をすべて成立したものとして状況を導出する。判定失敗は表示全体を失敗させる。

本文は `Description:` の次の行から表示する。本文中にIDや節の見出しと同じ表記があれば、それが構造を表す行でないことは表示から区別できない。

未充足の前提と詰まっている理由は、本文の前に次の節と行の見出しで示す。各行は一覧と同じID・種別・現在の状況・label・タイトルの行で、保存先に無い祖先と依存先はIDの後に `(missing)` を付けて示す。

| 節 | 対象 | 行の見出し |
| --- | --- | --- |
| `Required to start` | 着手の前提が未充足のIssue | `Ancestor must be adopted:`・`Dependency must complete:`・`Ancestor dependency must complete:`・`Unsurfaced ancestor:` |
| `Required to complete` | 完了の前提が未充足のEntity（`Stalled` を置くGroupを除く） | `Ancestor must be adopted:`・`Dependency must complete:` |
| `Stalled` | 詰まっている保存値 `NotStarted` のGroup（[理由](candidates.md#詰まっている-group-と理由)） | `Dependency must complete:`・`Ancestor dependency must complete:`・`Descendant dependency must complete:`・`Undecided child:`・`Open subgroup:`・`Unsurfaced candidate:`・`Own condition unsatisfied:`・`Undecided ancestor:`・`Ancestor must be adopted:`・`Unsurfaced ancestor:` |
| `Conflicted` | 衝突中のEntity。待ち理由と `Stalled` の節は置かない | `axon resolve ID` と同じheadの一覧 |
| `Invalid` | 違反に含まれるEntity | 違反の種類ごとに一行 |

`Unsurfaced ancestor:` は候補にならない理由であり、`Start` の前提ではない。`--skip-conditions` では `Unsurfaced candidate:`・`Own condition unsatisfied:`・`Unsurfaced ancestor:` は現れない。

`axon show ID --details` は待ち理由の節の代わりに保存情報の詳細を示す。Issueでは状況と異なる場合だけ保存lifecycleを `Lifecycle:` として別記し、Groupでは `Lifecycle:` に実効値を示して、保存値と異なれば `Lifecycle: InProgress (stored NotStarted)` のように保存値を併記する。条件が外部コマンドなら、`Condition:` の次の行に `Approval: approved`・`Approval: unapproved`・`Approval: unknown` のいずれか、その次に `Digest: DIGEST`、その次に `Command: CMD`（[再浮上条件の許可](#再浮上条件の許可) の表記）を、字下げせずに示す。本文はこれらの行より後に字下げせずに続き、同じ形の行を書けるため、許可の判断には使わない。

## 再浮上条件の許可

許可の意味、自動で許可する経路、未許可の条件の評価は [条件の許可](candidates.md#条件の許可)、許可の保存場所と形式は [保存と統合の契約](storage.md#axon-が-axon-の外に書くもの) に定める。

条件の文字列を示す `Command: CMD` の `CMD` は、元のバックスラッシュを `\\`、改行を `\n`、tabを `\t`、CRを `\r` にし、それ以外の制御文字（U+0000〜U+001F、U+007F〜U+009F）を `\x1b` のように `\x` と小文字16進2桁、それ以外のASCII以外の文字（日本語、引用符に似た文字、不可視の書式文字、双方向制御文字、U+2028・U+2029を含む）を `\u{2028}` のように小文字16進の `\u{...}` にして一行で示す。規則は上から順に当て、最初に当たったものを使う。表示から元の文字列を一意に復元でき、実際の改行と2文字の `\n`、shellの引用符と似た文字を見分けられるようにするためである。`Digest: DIGEST` の `DIGEST` は小文字16進64文字の完全な値で示す。`axon condition unapproved` と `axon condition allow` の出力では、どちらも字下げしてEntityの行の後に `Digest:`、`Command:` の順に置く。

許可するかは、本文を含まない `axon condition unapproved` の出力と `axon condition allow` の出力（成功時と、digestの不一致の診断）で判断する。

`axon condition unapproved` は、保存先の衝突していないEntityのうち、条件が外部コマンドでdigestが許可されていないものを、lifecycleによらず示す。該当がなければstdoutに行を出さずに終了0、許可を読めなければ終了1とする。保存先と許可を変えない。

`axon condition allow A [--digest DIGEST]` の `A` は完全なEntity IDだけを受け付け、一意なsuffixでは解決しない。suffixを受け付けると、利用者が自分のEntityのsuffixのつもりで入力した文字列と同じ完全IDのEntityを他人が記録で持ち込み、完全IDの一致の優先によってそちらの条件を許可させられるためである。同じ理由で、`A` がほかのEntityの完全IDの末尾にも一致する場合は、そのEntityを示して拒否する。`DIGEST` は小文字16進64文字の完全な値だけを受け付け、それ以外の形は構文エラー（終了2）とする。Aが衝突中、Aの条件が未設定、`--digest` が現在の文字列のdigestと一致しない、または許可を読めない・書けない場合は、許可を変えずに終了1とする。digestの不一致の診断には、現在のdigestと文字列を `Digest:`・`Command:` と同じ表記で示す。許可は記録ではないため、`-r/--reason` を持たない。

許可を書く経路の結果は次のとおり示す。`ID` は完全なEntity ID、`DIGEST` は許可の対象の文字列のdigestである。

| 経路と結果 | 出力 | 終了 |
| --- | --- | --- |
| `axon condition allow` で新たに許可した / すでに許可済み | stdoutに `ID  Allowed condition` / `ID  Condition already allowed` と、字下げした `Digest:`・`Command:` | 0 |
| `axon condition allow` で、許可のfileのrename後のsyncに失敗 | `Result unknown` として、`axon condition unapproved` に `ID` が示されるかで確かめる案内 | 1 |
| 同じ文字列の `axon condition set` で、未許可の文字列を許可した | 同値操作の出力（`ID  No changes` と、理由を付けた場合の `Reason not saved; …`）の後に `ID  Allowed condition` と字下げした `Digest:`・`Command:` | 0 |
| 同じ文字列の `axon condition set` で、許可を読めない・書けない / rename後のsyncに失敗 | 同値操作の出力の後に、stderrへ `Error: approval not saved (DETAIL)` / `Error: approval result unknown (DETAIL); check whether ID is listed by axon condition unapproved` | 1 |
| 新しい文字列の記録を公開した後に許可を書けない（`axon condition set`・`axon capture --command`） | 通常の成功出力の後に、stderrへ `Warning: approval not saved (DETAIL); the condition is saved but will not run until approved; after fixing that, approve it with axon condition allow ID --digest DIGEST` | 0 |
| 新しい文字列の記録を公開した後、許可のfileのrename後のsyncに失敗 | 通常の成功出力の後に、stderrへ `Warning: approval result unknown (DETAIL); check whether ID is listed by axon condition unapproved` | 0 |
| 記録の公開が確定しない | 通常の `Result unknown` の診断に、条件は許可されていないことと `axon condition allow ID --digest DIGEST` での許可を添える | 1 |

記録を公開した後の許可の失敗で終了0とするのは、記録は適用済みなので、終了codeで未適用と読ませず、`axon capture` の再実行で別のEntityを作らせないためである。この警告の書込に失敗しても終了codeと成功出力を変えない。

## デスクトップアプリで開く

`axon gui` は他のcommandと同じ探索で管理rootを決め、保存先を開けることを確かめてから、macOSの `open` にbundle identifier（`me.gin0606.axon`）と `me.gin0606.axon://open?root=<percent-encodeした絶対path>`（symlinkを解決したpath）を渡してデスクトップアプリ（`Axon.app`）にその管理rootを渡す。macOSはほかのアプリが同じschemeを宣言することを禁じないため、`open` にはbundle identifierも渡し、`Axon.app` 以外に渡さない。リンクを受けたアプリの振る舞いは [デスクトップアプリ](gui.md#cli-から開く) に定める。`axon gui` は保存先へ書き込まない。macOS以外では保存先を探す前に終了1とする。

## Note本文の横断検索

`axon note search <語句>` は管理root内の全Entity（`Completed`・`Cancelled`を含む）のNote本文を、条件を実行せずに検索する。元の保存文字列にcase-sensitiveなliteral部分一致を適用し、trim・Unicode正規化をしない。空文字は構文エラー（終了2）、該当なしはstdout空・stderrに案内を出して終了0。

同じNote内の複数一致も1 Note＝1行とし、所属Entityの完全ID、完全なNote ID、日時、`Excerpt:` 付きの最初の一致の抜粋を示す。Entityは`axon list`と同じ順、Entity内は`axon note list`と同じ日時順（同時刻は記録ID順）で並べる。抜粋は原文を省略した側に `…` を示し、改行を `\n`、元のバックスラッシュを `\\` として可視化し、他の制御文字は[表示とstream](#表示とstream)の規則に従う。固定列・区切りや機械向けの出力形式は保証しない。

## 現在の記録者の表示

`axon actor` は、現在の環境から取得した記録者情報、つまりこの環境で次に作る記録に付く記録者情報を一行で出力する。その一行は、`axon log`・`axon note list`・`axon note show` に `--recorder-details` を付けたときの記録者の表示と同じ形式で、同じ記録者情報なら同じ文字列になる。skill はこの一致に依存し、`axon actor` の出力を履歴の記録者の表示とそのまま見比べる。記録者を取得できないときは `—` を出力する。どちらも終了0とする。記録者情報の取得と、権限・排他に使わないことは [記録者情報](lifecycle.md#記録者情報とエージェント連携) に従う。

## 表示とstream

利用者のタイトル・本文・Note・理由は原文を保持する。C0/C1制御文字（ESCを含む）は `\x1b` のように、tabとCRは `\t`・`\r` として可視escapeし、表示順を入れ替える双方向制御文字（U+061C、U+200E–U+200F、U+202A–U+202E、U+2066–U+2069）は `\u{202e}` のような小文字の `\u{...}` で表示する。この規則はタイトル・本文・Note・理由・記録者・path・条件のコマンドと出力・診断に、TTYかどうかを問わず同じ文字表現で適用し、保存値・`axon export`・declarationの値は変えない。本文のその他のUnicodeと改行は保持する。一行の中に表示する値（タイトル、理由、actor と記録者情報の data、条件コマンド、親のタイトル）では、改行を `\n`、行区切り・段落区切り（U+2028・U+2029）を `\u{2028}`・`\u{2029}` として表示し、保存された文字列が記録の行や節の見出しを装えないようにする。

`axon export`・`axon completion`・`axon docs declaration --example` は常にANSIの装飾なしで出力する。

## mutationの結果

成功確認は完全IDが先頭。`axon write`・`axon label set`・関係・条件・種類の変換の同値操作は `No changes` の成功で、保存状態・履歴を変えない。例外として、終了した（`Completed`・`Cancelled`）Entityへの `axon write` と `axon label set` は、指定した値が現在の値と同じでも拒否する。reason は同値判定より先に検証する。同値操作では理由が指定されても保存せず、`No changes` に加えて `Reason not saved; use axon note add to record it.` と表示する。同値lifecycle遷移は拒否される。

成功はstdout/終了0、アプリケーションの拒否・失敗は `Error:` を含むstderr/終了1。構文エラーは終了2。

保存境界に誤解の余地がある場合は `Applied`、`Not applied`、`Result unknown` を区別する。記録fileのrename後の同期失敗は再読まで結果不明。保存成功後の出力障害は適用済みを明示し、作成・Noteを盲目的に再送させない。部分適用された複数command列の前段成功を後段失敗で未適用と説明しない。
