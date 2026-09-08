# Axon mutation contract

Axon storage または storage 関連 artifact を変更する前、または結果が不確かな mutation を照合するとき、この reference を読む。

## active backend を尊重する

Axon は設定 file を使わず、規定の canonical state path から backend を発見する。File state は worktree-local、SQLite は Git worktree 間で共有される。両方の path がある場合は error で、invalid または pending な state は discovery を停止する。他の worktree を scan せず、repository ごとに 1 backend を support する。mutation の前に意図する root と canonical path を調査し、legacy backup を authoritative として扱わない。

file backend では、通常の mutation は active worktree の `.axon/state.jsonl` を変更する。これは Git tracking を意図し、すでに tracked の場合がある。`.gitattributes` と `.axon/.gitignore` は同じ worktree-local artifact set に属する。mutation はそれらの file の staging、commit、merge、破棄を許可しない。無関係な working tree 変更を保持し、操作で変更された storage artifact を報告する。1 worktree での read は現在の snapshot だけを観測し、別 worktree に divergent state や claim がないことは証明しない。

Git 内の SQLite では、authoritative な `.axon/axon.db` は common Git directory の parent 配下にあり、current sandbox または worktree 外の場合がある。host permission が必要なら、access 要件を確認済みの許可された Axon command だけに escalation を限定する。lock または support された自動 storage update のため write access が必要な read command も含む。1 command への permission は他の command や無関係な program へ拡張されない。必要な操作ごとに host の permission process に従う。

file backend の Git index が unmerged の場合、通常の操作は意図的に拒否される。入力を保存し、storage または merge workflow により検証済み snapshot を解決して stage する。state file を直接書いて guard を迂回しない。

## 作用を 1 つずつ実行する

- 状態を変える各 Axon command は単独の shell call として実行し、別 command が終了 status を隠さないようにする。
- read-only discovery と無関係な program を同じ shell call に含めない。
- 呼び出し側が許可した正確な対象と payload を使う。selector の拡大、関係の追加、別 Entity の選択、後続 phase への暗黙の進行を行わない。

## 観測した状態を検証する

mutation の成功後、対象または artifact の完全な状態を読み、操作の postcondition を検証する。capability の作用で変わりうる Revision、Note、関係、claim、frontier、storage artifact は再読する。command output は証拠として扱い、関係する postcondition の代用にはしない。

multi-phase workflow が一部の mutation だけを完了した場合、以下の retry rule に従って失敗 phase を照合する。結果と安全な次操作を立証できたら、復旧可能な実行 error を訂正し、許可済み phase を続ける。結果が不明なまま、重要な判断が不足、または復旧に未許可の作用が必要なら停止する。適用済み state と recovery artifact を保存し、完了済み・残りの phase を別々に報告し、compensating mutation による自動 rollback は行わない。

## 照合後にだけ再試行する

明確な失敗は、原因を変えず同じ command を繰り返す許可ではない。command の完了または storage 適用が不明なら、まず process の終了を確認し、stable ID、record 件数、actor label、payload、操作固有の postcondition で現在の状態を調査する。

失敗原因を解消した後、capability の recovery rule または CLI の repetition contract と、観測した現在値・関係する history によって繰り返しが安全だと立証できる場合に mutation を再試行する。たとえば `write`、`group set|unset`、`dep add|rm` は同じ保存値を成功した no-op として受け入れる。再試行前に、対象、意図する作用、該当する precondition が許可された依頼と引き続き一致することを検証する。Entity 作成と Note 追加は非 idempotent のままであり、capability 固有の重複 check が必要である。証拠から適用済みと未適用を区別できない場合、storage 結果を不明と報告して停止する。

## 入力 snapshot を保存する

file または stdin を source とする mutation で、後からの再読により payload が変わりうる場合は、最初の試行前に正確な byte 列を保存する。操作の retry contract が必要とする場合は digest を記録し、許可された再試行にはその検証済み byte 列だけを再利用する。mutable source を暗黙に再読しない。

temporary snapshot は、操作が適用済みまたは未適用だと検証できた後にだけ削除する。不明または部分的な結果の照合に必要なら、正確な path と digest を保存する。
