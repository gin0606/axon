# 単一lifecycle CLIの入出力契約

状態・構造・候補集合・通常表示の内容は [正本spec](../../spec/lifecycle_proposal.md#cli-と表示)。この文書はそれと両立する識別子、引数、表示・保存結果の公開契約を定義する。旧契約との対応と理由は [2026-09-12の照合](../development/audits/lifecycle-contract-continuity-2026-09-12.md) に記録する。

## 識別子と入力

Issue/Groupは共通の `<prefix>-<ランダム6文字>` namespaceを使う。乱数部分は小文字Crockford Base32で紛らわしいi/l/o/uを除く。連番やkind、優先順位の意味を持たせず、同じ保存先で衝突したら再生成する。prefixは管理root名が既定で、`init PREFIX` で上書きできる。旧版同様、空でないroot名のUnicode・空白も保持する。端末制御文字は表示で可視escapeする。prefixに空白がある完全IDはshellでquoteするかsuffixで参照する。

全Entity入力は完全IDまたは一意なsuffixを受け付ける。対象だけでなくparent/needsも同じ規則。曖昧なときは候補IDを示して拒否し、保存を変更しない。mutationではlock取得後のsnapshotで解決する。既存の長いIDを改番しない。Note・状態記録・storeの安定IDはEntityの短いIDと別の契約であり、内部識別子の長さは変更しない。

登録titleは `--title`。本文は `-m/--description` または `-F/--description-file`。Noteは `-m/--message` または `-F/--file`。旧位置引数や旧本文optionの互換入口は提供しない。`-F -` はUTF-8のstdinを一度読む。本文・Noteをtrimして保存しない。初期の `--parent`、反復可能な `--needs`、`--command` は作成と同時に検査・保存する。作成中に条件を実行しない。通常writeはtitleと本文を一transactionで編集する。

option値の先頭hyphenは `--description='--text'` のように渡す。構文は `axon help <COMMAND PATH>` で確認できる。

## 一覧と詳細

listは保存済み全件を作成日時の昇順、同時刻はID順で表示する。`--kind issue|group`、`--lifecycle undecided|not-started|in-progress|completed|cancelled`、`--terminal=true|false` はANDで組み合わせる。terminalはCompletedまたはCancelledで、着手可能・浮上とは別。`--search` は現在title・本文だけのcase-sensitiveなliteral一致。Unicode正規化やtrimをせず、空文字は構文エラー。%、_、正規表現記号に特殊な意味はない。検索時だけMatchedに該当field（Title、Description）を付記する。

proposals/tasksはkind/searchで候補を絞ってから、必要な祖先を含め条件を評価する。一回の呼出しで同じ条件を重複評価しない。除外候補の条件は評価しないが、残った候補の祖先ならkindが異なっても評価する。評価失敗時に部分一覧をstdoutへ出さない。時間制限は正整数とms/s/m/hで、既定30s。詳細は [条件契約](../development/lifecycle-candidates.md)。

通常行は `ID  Kind  Situation  Title`。状況はUndecided、Ready、Blocked、InProgress、InProgress+Blocked、Completed、Cancelled。Ready/Blockedは保存されたNotStartedの親・依存前提から導出し、浮上条件は使わない。保存したタイトルに改行があれば一覧の中では `\n` として一行に保つ。

showは本文、Note件数、親、直接の未充足前提、Groupの全子孫ツリーとterminal数（Group・Issueを含み、対象自身を除く）を表示する。親の着手待ちは所属欄と重複しない。満たされた依存は常時展開しない。子孫はCompleted・Cancelledも含め、兄弟を作成日時順（同時刻はID順）で表示する。

`show ID --details` は保存情報の明示的な詳細入口。通常の待ち理由節を置き換え、親・条件・全直接dependency・直接dependentを取得する。同じ関係を待ち理由と再列挙しない。状況と異なる場合だけ保存lifecycleを別記する。条件未設定、親なし、空の依存集合も明示する。Groupの全子孫ツリーは通常表示と同様に表示する。通常show/details/listは条件を実行しない。

`note list ID` は全文、日時、actor、安定Note IDを因果順に表示する。`note show ID NOTE_ID` は同じEntityの個別Noteを取得する。logは状態変化と統合を表示し、並行する分岐を時刻で逐次操作へ並べ替えない。Note/logの `--recorder-details` は保存済みdataを併記する。`actor` は現在環境で検出できたactor、未取得なら `—` を表示し、保存を行わない。

## 表示とstream

CLIが生成するhelp・ラベル・診断は英語。利用者のタイトル・本文・Note・理由は原文を保持する。human時刻はlocal時刻と数値UTC offset。C0/C1/ESC、tab、CRは可視escapeし、本文のUnicodeと改行は保持する。

装飾は対象streamがTTYでNO_COLORが存在しない場合だけ。同じ内容からANSIを除けば非TTYとテキスト・順序・空白が一致する。IDはcyan＋bold、見出しはbold、着手はcyan、成功/Readyはgreen、待ちはyellow、未判断はyellow＋bold、kind・terminal・no-op・補助情報はdim、エラーはred＋bold。ユーザー本文・タイトルは着色せず、色だけを意味の手掛かりにしない。completionは常に装飾なし。

一覧0件はstdoutに行を出さず、短い案内をstderrへ出して終了0。候補不在から保存情報の不存在を推測しない。通常行へ毎回操作例を付けず、help/docsへ使い方を分ける。

bare `axon`、help、-h、--helpは同じ用途別root helpをstdoutへ出して終了0。leaf help、docs、actor、version、completionは保存先を開かず取得できる。docsはbinary同梱の端末用説明で、ソースcheckoutやネットワークへ依存しない。versionはビルド時commitとsource状態で、実行directoryから由来を推測しない。

## mutationの結果

成功確認は完全IDが先頭。Created、Note ID recorded、状態遷移の結果、実際に変わったtitle/本文/parent/dependency/conditionを短く示す。保存処理が返した結果を使い、lock前の読取から更新を推定しない。write・関係・条件の同値操作はNo changesの成功で、保存状態・履歴を変えない。同値lifecycle遷移は拒否される。

成功はstdout/終了0、アプリケーションの拒否・失敗はError:を含むstderr/終了1。Clapの構文エラーは既定のerror:/Usage構造と終了2。原因、判明している対象・操作を示し、曖昧なFailedだけで済ませない。

保存境界に誤解の余地がある場合はApplied、Not applied、Result unknownを区別する。SQLite commit失敗とfile置換後の同期失敗は再読まで結果不明。保存成功後の出力障害は適用済みを明示し、作成・Noteを盲目的に再送させない。stdoutのBrokenPipeは成功として扱うが、条件traceのstderr障害は一覧失敗。部分適用された複数command列の前段成功を後段失敗で未適用と説明しない。

## 計画全体の取得と一括編集

`export` と `import prepare|check|apply` のhelpは、代表例と次に実行するcommandを保存先を開かずに表示する。

- `axon export ID...` は完全 ID または一意な suffix を一つ以上受け取り、Issue 単体または Group 全子孫の和集合を canonical YAML として stdout に出す。保存先を変更せず、条件を実行しない。
- `axon docs declaration` は field と新規・既存の違い、prepare → check → apply → 再 check の手順を stdout に説明する。`--example` は新規計画の canonical YAML だけを stdout に出す。どちらも保存先を開かない。引数なしの `axon docs` は従来の説明と declaration への案内を返す。

`axon import prepare FILE` は新規IDを割り当て、外部参照を再生成したcanonical YAMLで同じfileを置き換える。保存先は変更せず、成功時はfile名と保存先未変更、新規recordの `key -> 完全ID` の対応を一行ずつ表示する。書込前の失敗・bytes競合はNot applied、rename後の同期失敗はResult unknownとして診断し、残ったtemporary fileは診断で案内する。更新後にstdout出力だけが失敗した場合も、declarationがAppliedで保存先は未変更であることを示す。

`axon import check FILE` は全IDが確定したcanonical YAMLを要求し、違えばprepareを案内する。schema・identityと参照・読み取り専用項目・競合・共通コアの拒否を区別し、作成、titleの前後、descriptionの変更有無、parentの前後、needsの増減、差分なしと適用後の状況をEntityごとに表示する。titleはlistと同じく改行を `\n`、制御文字を可視escapeにして一行で表示する。条件は実行せず、fileと保存先を変更しない。

`export` と `import prepare|check|apply`、`docs declaration` の識別子、declaration の形式、保存結果と診断の区別は [正本spec](../../spec/lifecycle_proposal.md#計画全体の取得と一括編集) の「計画全体の取得と一括編集」に従う。declaration内のIDは完全IDだけを使い、exportの引数は他のcommandと同じくsuffixも受け付ける。保存境界の表示はこの文書の「mutationの結果」と同じApplied、Not applied、Result unknownを使う。

`axon import apply FILE` は書き込みlock内でFILEを読み、checkと同じ検証を再実行し、全変更を一回の保存境界で反映する。拒否時は全件Not applied。成功後は保存したsnapshotからbase・lifecycle・referencesとcanonical順を更新し、keyを保持してFILEを置き換える。成功出力にはbase更新前に新規だったrecordの `key -> 完全ID` の対応を一行ずつ含める。既存の再浮上条件とNoteは保持し、新規の条件は未設定とする。

保存成功後のFILE更新失敗は、保存先のAppliedとdeclarationのNot appliedまたはResult unknownを分けて表示する。rename直前に元bytesを再照合し、編集されていればそのfileを保持する。同じFILEを再applyし、編集集合の全Entityが宣言の最終値に一致すれば保存先はno-opでrewriteだけを完了する。部分一致は競合。SQLite commit失敗とfile正本のrename後sync失敗は保存先のResult unknownで、declarationは更新しない。

## Note本文の横断検索

`axon note search <語句>` は管理root内の全Entity（Issue・Group、Completed・Cancelledを含む）のNote本文を検索し、条件を実行しない。追加filterは設けない。元の保存文字列にcase-sensitiveなliteral部分一致を適用し、trim・Unicode正規化をしない。空白・改行・%・_・正規表現記号は通常の文字として扱う。空文字は構文エラー（終了2）、該当なしはstdout空・stderrに案内を出して終了0。先頭hyphenの語句は `axon note search -- '--text'` と渡す。

同じNote内の複数一致も1 Note＝1行とし、所属Entityの完全ID、完全な安定Note ID、既存のlocal日時と数値UTC offset、`Excerpt:` 付き抜粋を示す。Entityはlistと同じ作成日時昇順・同時刻ID順、Entity内はnote listと同じ因果順・並行記録のID順で並べる。見出し・空行・分岐説明行は加えない。

抜粋は最初の一致と前後24文字で、検索語そのものを省略せず、日本語を壊さない文字単位で切り出す。原文を省略した側に `…` を示す。原文で一致位置と範囲を決めた後、改行を可視の `\n`、元のバックスラッシュを `\\` にし、他の端末制御文字も可視化する。保存本文は変えない。長い検索語でも一行の固定上限で切り捨てない。行単位での絞り込み向けで、固定列・区切りや機械向け出力形式は保証しない。

list・tasks・proposalsの `--search` はNoteだけの一致ではEntityを返さなくなった。現在の主題は `list --search`、Noteに残る情報は `note search`、原文は `note show ID NOTE_ID` で読む。`note list ID` のEntity IDは引き続き必須。
