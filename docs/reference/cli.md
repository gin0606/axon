# CLIと表示の契約

この文書は、公開コマンドが受け取る入力と、何をどう表示し、保存結果をどう伝えるかの契約を定める。状態・遷移・包含・dependency・候補集合の意味は [lifecycleと構造の契約](lifecycle.md)、候補一覧の条件評価は [候補と外部条件](candidates.md)、declarationの形式は [計画全体の取得と一括編集](declaration.md) に従う。

## 情報を見る目的

採否判断、着手する仕事の選択、着手から完遂、最終確認の四つを、情報を見る目的として扱う。Entityの保存状態とは別の分類であり、四種類の状態や専用画面を追加しない。再開・引継ぎ・計画修正は着手から完遂に含め、採否の再判断が必要なら採否判断へ戻る。

利用者はIDを使って操作するため、表示の先頭はID・種別・状況・タイトルとする。内部fieldを並べて利用者に解読させず、その場の判断に必要な内容を短く示す。通常表示へ操作例やコマンド案内を毎回付けず、使い方はhelpへ置く。本文を機械的に要約・分類して判断材料を生成する機能は設けない。

## 識別子と入力

Issue/Groupは共通の `<prefix>-<ランダム6文字>` namespaceを使う。乱数部分は小文字Crockford Base32で紛らわしいi/l/o/uを除く。連番やkind、優先順位の意味を持たせず、同じ保存先で衝突したら再生成する。prefixはASCII小文字の `a-z`、数字、ハイフンだけを許し、空、先頭のハイフン、末尾のハイフンは拒否する。この文字種に限ることで、完全IDをshellでquoteせずに渡せる。`axon init PREFIX` の明示値は変換せず検証する。省略時は管理rootのdirectory名のASCII大文字を小文字化した結果を使い、規則に合わなければ保存先を作らずに失敗し、`axon init PREFIX` での明示を求める。小文字化以外の自動補正はしない。

全Entity入力は完全IDまたは一意なsuffixを受け付ける。対象だけでなく`parent`/`needs`も同じ規則。曖昧なときは候補IDを示して拒否し、保存を変更しない。mutationではlock取得後のsnapshotで解決する。Note・状態記録・storeの安定IDは、Entityの短いIDと別の契約である。

option値の先頭hyphenは `--description='--text'` のように渡す。構文は `axon help <COMMAND PATH>` で確認できる。

## 一覧

一覧の入口は三つとし、候補集合の定義は [lifecycleと構造の契約](lifecycle.md#候補集合) に従う。

| コマンド | 表示対象・役割 |
| --- | --- |
| `axon proposals` | 判断候補のIssue・Group |
| `axon tasks` | 浮上した未着手と着手中のIssue・Group |
| `axon list` | 非浮上・完了・取りやめも含む保存済みEntityを、必要な条件で絞り込む汎用一覧 |

`axon tasks` は依存先の完了待ちや親の着手待ちの未着手も含み、着手できるものだけには限定しない。着手中だけを見るには `axon list --lifecycle in-progress` を使う。

`axon list`は保存済み全件を作成日時の昇順、同時刻はID順で表示する。`--kind issue|group`、`--lifecycle undecided|not-started|in-progress|completed|cancelled`、`--terminal=true|false` はANDで組み合わせる。terminalは`Completed`または`Cancelled`で、着手できることや浮上とは別。`--search` は現在title・本文だけのcase-sensitiveなliteral一致。Unicode正規化やtrimをせず、空文字は構文エラー。%、_、正規表現記号に特殊な意味はない。検索時だけMatchedに該当field（Title、Description）を付記する。

`axon proposals|tasks`はkind/searchで候補を絞ってから、必要な祖先を含め条件を評価する。一回の呼出しで同じ条件を重複評価しない。除外候補の条件は評価しないが、残った候補の祖先ならkindが異なっても評価する。評価失敗時に部分一覧をstdoutへ出さない。時間制限は正整数とms/s/m/hで、既定30s。詳細は [候補と外部条件](candidates.md)。`axon list`と保存情報を読む`axon show`は外部条件コマンドを実行しない。

一覧とGroupの子一覧は、状態ごとに区切らず作成日時の古い順へ統一する。作成日時そのものを通常の各行へ表示する必要はない。同時刻はID順で安定させる。

通常行は `ID  Kind  Situation  Title`。状況欄は保存された lifecycle だけの表示ではなく、保存状態と構造から導出する短い表現とする。再浮上条件の成立はこの欄での着手可能性の判定に使わない。

| 状況 | 保存状態・前提 | 意味 |
| --- | --- | --- |
| `Undecided` | `Undecided` | 未判断 |
| `Ready` | `NotStarted` で親・dependencyの着手前提を満たす | 着手できる |
| `Blocked` | `NotStarted` で着手前提が不足 | 親の着手待ちまたは依存先の完了待ち |
| `InProgress` | `InProgress` でdependencyが充足 | 着手中 |
| `InProgress+Blocked` | `InProgress` で未完了の依存先がある | 着手中で、完了に必要な依存先が残る |
| `Completed` | `Completed` | 完了 |
| `Cancelled` | `Cancelled` | 取りやめ |

親の着手待ちも`Blocked`に含める。これは表示上のまとめ方で、保存する包含と明示dependencyの区別は維持する。Groupの未終了の子は最終確認前の進捗として子一覧へ示し、明示dependencyと混ぜない。保存したタイトルに改行があれば一覧の中では `\n` として一行に保つ。

```text
demo-k3m7pq  Group  InProgress  検索画面を実装する
demo-8bxw2r  Issue  InProgress+Blocked  検索APIを実装する
demo-c9d4ts  Issue  Ready  検索フォームを実装する
demo-9f2hjx  Issue  Blocked  検索結果を表示する
```

例は架空の内容である。列の間隔やグルーピングは実装が決め、固定列や機械向けの出力形式は保証しない。

## `axon show` と待ち理由

`axon show ID`は、ID・種別・状況・タイトル、Note件数、所属計画のID・タイトル、本文を基本とする。本文は保存された内容を表示する。Note本文、履歴、内部のcausal情報、不要な設定・件数の羅列、操作コマンドの案内は通常表示から外す。Noteがあれば `5 notes` のように存在を示し、0件ならその表示を省略する。

未充足の前提がある場合だけ、本文の前へ `Required to start` または `Required to complete` の節を置く。満たされていない直接の前提を `Parent must start:`・`Dependency must complete:` として、ID・種別・現在の状況・タイトルの行で示す。満たされた依存や依存先の先のツリーは常時展開しない。親の所属表示と待ち理由が同じ情報になる場合は、重複を避けて配置する。

Groupの場合は、この共通表示の末尾へ全子孫のツリーと短い集計を加える。`Completed`・`Cancelled`も含めて全階層を展開する。各行は一覧と同じID・種別・状況・タイトルとし、兄弟を作成日時順（同時刻はID順）で揃え、別のDependencies節へ同じ情報を再列挙しない。

集計は `Descendants: 2/4 terminal (1 completed, 1 cancelled)` のように、全子孫の終了数と完了・取りやめの違いが読める形とする。集計はGroup・Issueの両方を含み、対象自身を除く。全状態の内訳は羅列しない。進行中のGroupで、子が全員終了し自身の依存先も完了していれば `Awaiting final confirmation` を示す。これは導出される案内であり、保存状態やレビュー済み状態を追加しない。

`axon show ID --details` は保存情報の明示的な詳細入口。通常の待ち理由節を保存情報の詳細へ置き換え、親・条件・全直接dependency・直接dependentを取得する。同じ関係を複数の節へ重複して列挙しない。状況と異なる場合だけ保存lifecycleを `Lifecycle:` として別記する。条件未設定、親なし、空の依存集合も `(none)` と明示する。Groupの全子孫ツリーは通常表示と同様に表示する。

## Noteと履歴

`axon note list ID` は指定EntityのNote本文を全文で、日時・actor・安定Note IDとともに因果順に表示する。通常の逐次記録は保存順に古いものから読み、分岐した記録は因果関係を保持して表示する。並行する記録だけをID順で並べ、分岐間の先後を時刻から捏造しない。`axon note show ID NOTE_ID` は同じEntityの個別Noteの原文を取得する。`axon note add ID …` は追記し、編集・削除は設けない。

`axon log ID` は状態変更・統合の経緯を読む入口とする。変更前後の状態、日時、記録者、任意の理由を人が読める形で示す。統合では実際の操作と採用結果を区別し、内部の因果辺や記録IDの羅列を通常表示へ出さない。並行する分岐を時刻で逐次操作へ並べ替えない。

`axon note list ID --recorder-details`・`axon log ID --recorder-details` は保存済みdataを併記し、通常表示はactorのみとする。`axon actor` は現在環境で検出できたactor、未取得なら `—` を表示し、保存を行わない。取得の契約は [記録者連携](../development/lifecycle-recorder.md) を参照する。

最終確認は、Groupと必要な子の`axon show`と、それぞれのNoteを読む操作を組み合わせる。Noteから成果・検証結果を自動抽出しない。配下の全Noteをまとめて読む専用の入口は設けない。

## Note本文の横断検索

`axon note search <語句>` は管理root内の全Entity（Issue・Group、`Completed`・`Cancelled`を含む）のNote本文を検索し、条件を実行しない。追加filterは設けない。元の保存文字列にcase-sensitiveなliteral部分一致を適用し、trim・Unicode正規化をしない。空白・改行・%・_・正規表現記号は通常の文字として扱う。空文字は構文エラー（終了2）、該当なしはstdout空・stderrに案内を出して終了0。先頭hyphenの語句は `axon note search -- '--text'` と渡す。

同じNote内の複数一致も1 Note＝1行とし、所属Entityの完全ID、完全な安定Note ID、既存のlocal日時と数値UTC offset、`Excerpt:` 付き抜粋を示す。Entityは`axon list`と同じ作成日時昇順・同時刻ID順、Entity内は`axon note list`と同じ因果順・並行記録のID順で並べる。見出し・空行・分岐説明行は加えない。

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
| 着手 | `axon start A` |
| 作業を解放 | `axon release A` |
| 完了 | `axon complete A` |
| 取りやめ | `axon cancel A` |
| 再検討 | `axon reconsider A` |
| タイトル・本文を編集 | `axon write A …` |
| 親Groupを設定・変更 / 解除 | `axon parent set A --parent G` / `axon parent unset A` |
| 依存先を追加 / 解除 | `axon dep add A --needs B` / `axon dep rm A --needs B` |
| 再浮上条件を設定 / 解除 | `axon condition set A --command '条件コマンド'` / `axon condition unset A` |

登録は一つのコマンドで行い、種別は `--kind issue|group`、採否は `--accept` の有無で指定する。`--kind` の既定は `issue`、`--accept` 省略時は`Undecided`、指定時は`NotStarted`で作成する。登録と採否は直交し、作成後は同じIDベースの操作を使う。操作対象のIDは位置引数、関係先のIDは役割を明示するoptionとし、登録時の親・依存指定も `--parent`・`--needs` に揃える。

登録titleは `--title`。本文は `-m/--description` または `-F/--file`。Noteは `-m/--message` または `-F/--file`。本文option同士は排他で、`-F -` はUTF-8のstdinを一度読む。本文・Noteをtrimして保存しない。初期の `--parent`、反復可能な `--needs`、`--command` は作成と同時に検査・保存する。作成中に条件を実行しない。通常の`axon write`はtitleと本文を一transactionで編集し、lifecycleを変えない。長い本文は登録時と同じ本文optionでファイルから渡せる。

`axon accept|withdraw|start|release|complete|cancel|reconsider A` は `-r/--reason` を履歴へ保存する。Groupへの`axon complete`の実行自体を「計画全体の最終確認が通った」という明示入力とする。Axonは子・依存・状態を検査し、確認作業は呼び出す人・エージェントのskillと運用で担う。必須のレビュー確認フラグや独立したレビュー済み状態は設けない。どの変更コマンドも、状態・包含・dependencyの制約を迂回しない。

## 計画全体の取得と一括編集

`axon export` と `axon import prepare|check|apply`、`axon docs declaration` が扱うdeclarationの形式、識別子、競合判定、拒否する入力は [計画全体の取得と一括編集](declaration.md) に従う。declaration内のIDは完全IDだけを使い、`axon export`の引数は他のcommandと同じくsuffixも受け付ける。保存境界の表示はこの文書の「mutationの結果」と同じApplied、Not applied、Result unknownを使う。

`axon export` と `axon import prepare|check|apply` のhelpは、代表例と次に実行するcommandを保存先を開かずに表示する。

- `axon export ID...` は完全 ID または一意な suffix を一つ以上受け取り、Issue 単体または Group 全子孫の和集合を canonical YAML として stdout に出す。保存先を変更せず、条件を実行しない。
- `axon docs declaration` は field と新規・既存の違い、`axon import prepare` → `axon import check` → `axon import apply` → 再度`axon import check` の手順を stdout に説明する。`--example` は新規計画の canonical YAML だけを stdout に出す。どちらも保存先を開かない。引数なしの `axon docs` は状態モデルと基本workflowの説明、および declaration への案内を返す。

`axon import prepare FILE` は新規IDを割り当て、外部参照を再生成したcanonical YAMLで同じfileを置き換える。保存先は変更せず、成功時はfile名と保存先未変更、新規recordの `key -> 完全ID` の対応を一行ずつ表示する。書込前の失敗・bytes競合はNot applied、rename後の同期失敗はResult unknownとして診断し、残ったtemporary fileは診断で案内する。更新後にstdout出力だけが失敗した場合も、declarationがAppliedで保存先は未変更であることを示す。

`axon import check FILE` は全IDが確定したcanonical YAMLを要求し、違えば`axon import prepare`を案内する。schema・identityと参照・読み取り専用項目・競合・共通コアの拒否を区別し、作成、titleの前後、descriptionの変更有無、parentの前後、needsの増減、差分なしと適用後の状況をEntityごとに表示する。titleは`axon list`と同じく改行を `\n`、制御文字を可視escapeにして一行で表示する。条件は実行せず、fileと保存先を変更しない。

`axon import apply FILE` は書き込みlock内でFILEを読み、`axon import check`と同じ検証を再実行し、全変更を一回の保存境界で反映する。拒否時は全件Not applied。成功後は保存したsnapshotからbase・lifecycle・referencesとcanonical順を更新し、keyを保持してFILEを置き換える。成功出力にはbase更新前に新規だったrecordの `key -> 完全ID` の対応を一行ずつ含める。既存の再浮上条件とNoteは保持し、新規の条件は未設定とする。

保存成功後のFILE更新失敗は、保存先のAppliedとdeclarationのNot appliedまたはResult unknownを分けて表示する。rename直前に元bytesを再照合し、編集されていればそのfileを保持する。同じFILEを再度`axon import apply`し、編集集合の全Entityが宣言の最終値に一致すれば保存先はno-opでrewriteだけを完了する。部分一致は競合。SQLite commit失敗とfile正本のrename後sync失敗は保存先のResult unknownで、declarationは更新しない。

## 表示とstream

CLIが生成するhelp・ラベル・診断は英語。利用者のタイトル・本文・Note・理由は原文を保持する。human時刻はlocal時刻と数値UTC offset。C0/C1/ESC、tab、CRは可視escapeし、本文のUnicodeと改行は保持する。

装飾は対象streamがTTYでNO_COLORが存在しない場合だけ。同じ内容からANSIを除けば非TTYとテキスト・順序・空白が一致する。IDはcyan＋bold、見出しはbold、着手はcyan、成功/Readyはgreen、待ちはyellow、未判断はyellow＋bold、kind・terminal・no-op・補助情報はdim、エラーはred＋bold。ユーザー本文・タイトルは着色せず、色だけを意味の手掛かりにしない。`axon completion`は常に装飾なし。

一覧0件はstdoutに行を出さず、短い案内をstderrへ出して終了0。候補不在から保存情報の不存在を推測しない。通常行へ毎回操作例を付けず、helpと`axon docs`へ使い方を分ける。

引数なしの `axon`、`axon help`、`axon -h`、`axon --help`は同じ用途別root helpをstdoutへ出して終了0。leaf help、`axon docs`、`axon actor`、`--version`、`axon completion`は保存先を開かず取得できる。`axon docs`はbinary同梱の端末用説明で、ソースcheckoutやネットワークへ依存しない。`--version`はpackage versionだけを出し、実行directoryやGitから由来を推測しない。

## mutationの結果

成功確認は完全IDが先頭。Created、Note ID recorded、状態遷移の結果、実際に変わったtitle/本文/parent/dependency/conditionを短く示す。保存処理が返した結果を使い、lock前の読取から更新を推定しない。`axon write`・関係・条件の同値操作はNo changesの成功で、保存状態・履歴を変えない。同値lifecycle遷移は拒否される。

成功はstdout/終了0、アプリケーションの拒否・失敗はError:を含むstderr/終了1。Clapの構文エラーは既定のerror:/Usage構造と終了2。原因、判明している対象・操作を示し、曖昧なFailedだけで済ませない。

保存境界に誤解の余地がある場合はApplied、Not applied、Result unknownを区別する。SQLite commit失敗とfile置換後の同期失敗は再読まで結果不明。保存成功後の出力障害は適用済みを明示し、作成・Noteを盲目的に再送させない。stdoutのBrokenPipeは成功として扱うが、条件traceのstderr障害は一覧失敗。部分適用された複数command列の前段成功を後段失敗で未適用と説明しない。
