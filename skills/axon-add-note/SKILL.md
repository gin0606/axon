---
name: axon-add-note
description: 既存の axon Issue または Group へ、declaration や状態を変えない追加情報を追記専用 Note として安全に残す。明示された Note 追加と、他の axon workflow で生じた作業結果・申し送りに使う。新規 Entity、採否判断、進行操作、plan declaration 変更には使わない。
---

# Axon Entity に Note を追加する

既存 Entity について後から得られた情報を、plan declaration や Control state と混ぜず、一件の Note として追加・確認する。

## 共有 DB への書き込み権限

Git linked worktree では共有 `.axon` が作業ディレクトリの外に置かれることがある。`axon note add` が共有 DB への書き込み権限不足で拒否された場合は、その command だけを実行環境の許可機構で再実行する。読み取り command や他の program まで権限を広げず、再実行できなければ追加を推測せずに拒否と未反映を報告する。

## 情報を分類する

1. `axon show <id>` の出力を省略せず、kind、plan declaration、Control state、記録件数、表示された全 Note を読む。Declaration Revision があれば `axon revision list <id>` と各 `axon revision show <id> <number>` も読み、Revision と Note は古さを理由に読み飛ばさない。
2. 調査結果、作業結果、制約、申し送りなど、Entity が何であるかや現在状態を変えない追加情報だけを Note として扱う。Issue / Group、Progress、Disposition、Resurface condition を問わず追加できる。
3. title、description、parent、outgoing dependency を変える内容は plan declaration であり、Note として追加しない。Progress、Disposition、Resurface condition、claim を変える内容も Control state の操作であり、Note ではない。既存 Entity の declaration や状態の変更は対象を所有する workflow、新規 Entity は `axon-plan-issue` または `axon-capture-issue` で扱う。
4. 状態変更の理由は `decide`、`when`、`release` の reason に残す。同じ内容を Note に重複させない。単なる進捗実況、定型的な開始・完了報告、既存 Note と実質的に同じ内容も追加しない。
5. ユーザーが Note 本文を明示した場合と、着手中 Entity の調査・実装で後から参照すべき重要情報が確定した場合は、エージェントが本文を整えて追加してよい。情報分類または対象 Entity が不明な場合と、本文がユーザーの判断を代弁する場合だけ、追加前にユーザーへ確認する。意図した効果が plan declaration や Control state の変更だと確定した場合は、この workflow を終了して該当する workflow へ引き渡し、確認後も Note で代用しない。
6. 既存 Note の誤りや前提変化は上書きせず、訂正対象を `Note <number>` のように安定した Entity 内番号で示す新しい Note にする。

## 一度だけ追加して確認する

1. 一回の依頼に複数の独立した Note が必要でなければ本文を一件にまとめ、追加する正確な文字列と、この実行環境で axon が記録する actor label を固定する。`-F <file>` や標準入力を使う場合は最初の実行前に一度だけ読み、他 actor が書けないエージェント所有の一時 file に保存して read-only にし、byte digest を控える。最初の追加と許可された再試行は同じ snapshot だけを使い、直前に digest を照合する。ユーザー所有 file や標準入力を再読込しない。
2. `axon note list <id>` を読み、追加前の件数と最新 Entity 内番号を控える。Note がなければ件数 0、最新番号 0 として記録する。
3. `axon note add <id> -m <body>` または snapshot を渡す `-F <file>` を、actor label を決める環境と作業 directory を保った状態変更 command として単独で一度だけ実行する。
4. 成功と Note 番号を観測したら、その番号を控えて追加 command は二度と実行しない。`axon note show <id> <number>` で本文、actor、時刻を確認し、`axon show <id>` の全出力から Note の保存と plan declaration、Control state、関係が変わっていないことを確認する。読取検証が失敗した場合は読み取りだけを再試行するか、番号と `recorded but unverified` を報告して停止する。
5. `note add` 自体の失敗が明らかな場合は、原因を変えずに再実行しない。成否が不明な場合は先の process が終了したことを確認し、現在の `axon note list` から追加前より後の全番号を列挙して各 `axon note show` を読む。固定した本文と actor の両方が一致する候補だけを今回の追加候補とし、他の Note は独立した並行追記として保持・報告する。
6. 一致する候補が 1 件なら、その番号を成功結果として使う。複数件なら追加 command を再実行せず、`DB applied: unknown` と全候補番号を報告して停止する。0 件であり、追加前より後の全 Note を確認できた場合だけ、snapshot と actor context が不変なことを再確認して同じ追加を一度だけ再試行してよい。再試行後も同じ手順で全候補を調べ、1 件なら成功、複数件または観測不能なら `DB applied: unknown`、0 件と確定できたなら `DB applied: no` として、それ以上再試行せず停止する。
7. 追加した Note 番号と要点、他の情報分類を変更していないこと、観測した独立の並行 Note をユーザーまたは呼び出し元の workflow へ返す。エージェント所有の snapshot は検証成功または `DB applied: no` の確定後だけ片付け、不明な場合は正確な path と digest を報告して保持する。
