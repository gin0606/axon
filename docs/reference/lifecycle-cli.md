# 単一lifecycle CLIの入出力契約

状態・構造・候補集合・通常表示の内容は [正本spec](../../spec/lifecycle_proposal.md#cli-と表示)。この文書はそれと両立する識別子、引数、表示・保存結果の公開契約を定義する。旧契約との対応と理由は [2026-09-12の照合](../development/audits/lifecycle-contract-continuity-2026-09-12.md) に記録する。

## 識別子と入力

Issue/Groupは共通の `<prefix>-<ランダム6文字>` namespaceを使う。乱数部分は小文字Crockford Base32で紛らわしいi/l/o/uを除く。連番やkind、優先順位の意味を持たせず、同じ保存先で衝突したら再生成する。prefixは管理root名が既定で、`init PREFIX` で上書きできる。旧版同様、空でないroot名のUnicode・空白も保持する。端末制御文字は表示で可視escapeする。prefixに空白がある完全IDはshellでquoteするかsuffixで参照する。

全Entity入力は完全IDまたは一意なsuffixを受け付ける。対象だけでなくparent/needsも同じ規則。曖昧なときは候補IDを示して拒否し、保存を変更しない。mutationではlock取得後のsnapshotで解決する。既存の長いIDを改番しない。Note・状態記録・storeの安定IDはEntityの短いIDと別の契約であり、内部識別子の長さは変更しない。

登録titleは `--title`。本文は `-m/--description` または `-F/--description-file`。Noteは `-m/--message` または `-F/--file`。旧位置引数や旧本文optionの互換入口は提供しない。`-F -` はUTF-8のstdinを一度読む。本文・Noteをtrimして保存しない。初期の `--parent`、反復可能な `--needs`、`--command` は作成と同時に検査・保存する。作成中に条件を実行しない。通常writeはtitleと本文を一transactionで編集する。

option値の先頭hyphenは `--description='--text'` のように渡す。構文は `axon help <COMMAND PATH>` で確認できる。

## 一覧と詳細

listは保存済み全件を作成日時の昇順、同時刻はID順で表示する。`--kind issue|group`、`--lifecycle undecided|not-started|in-progress|completed|cancelled`、`--terminal=true|false` はANDで組み合わせる。terminalはCompletedまたはCancelledで、着手可能・浮上とは別。`--search` は現在title・本文・全Noteのcase-sensitiveなliteral一致。Unicode正規化やtrimをせず、空文字は構文エラー。%、_、正規表現記号に特殊な意味はない。検索時だけMatchedに該当fieldと安定Note ID（ID順）を付記する。

triage/tasksはkind/searchで候補を絞ってから、必要な祖先を含め条件を評価する。一回の呼出しで同じ条件を重複評価しない。除外候補の条件は評価しないが、残った候補の祖先ならkindが異なっても評価する。評価失敗時に部分一覧をstdoutへ出さない。時間制限は正整数とms/s/m/hで、既定30s。詳細は [条件契約](../development/lifecycle-candidates.md)。

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
