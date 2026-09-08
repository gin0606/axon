# Entity 作成 contract

新しい Issue または Group を作成する前に、この reference を読む。

## 既存 Entity を再利用するかは呼び出し側が決める

Axon は Entity の意味的な一意性を定めない。呼び出し側の依頼または workflow が既存 Entity を再利用するか判断し、意図する kind、declaration、parent、outgoing dependency を与える。呼び出し側が要求しない限り、repository 全体の重複 policy を適用しない。

作成時の会話がなくても Entity を理解できるよう、title と任意の description を記述する。Issue は 1 つの懸念または作業項目、Group は明示的な計画境界を表す。後から得た finding や handoff は description ではなく Note に置く。

## 完全な初期状態を与える

4 つの creator はすべて、`--parent <group-id>`、複数の `--needs <entity-id>`、および `--manual`、`--at <YYYY-MM-DD>`、`--after <entity-id>`、`--command <shell-string>` のいずれか 1 つの初期 condition を受け入れる。condition option なしは `Always`、`--needs` なしは dependency なしを意味する。ID には完全な ID または一意な suffix を指定でき、重複 dependency は 1 件として保存される。shell 文字列は 1 argument として quote し、先頭が hyphen の場合は `--command='--help text'` を使う。

作成は Entity、関係、condition、初期 Revision を 1 transaction で保存する。`plan` は Accepted、`capture` は Undecided で開始し、どちらも claim のない NotStarted である。Accepted Revision は dependency を含む完全な declaration を保持するが、condition は含まない。Undecided での作成に Revision はない。初期値は架空の decision history や condition-change history を作らない。明確な失敗では部分的な Entity や record は残らない。

Command 文字列は保存時にも確認出力の生成時にも評価されない。saved state の検証には `show --skip-command-evaluation` を使い、Accepted での作成では最初の Revision を調査する。通常の derived query は引き続き Command を評価する。既知の初期入力に技術上の capture/edit/accept staging は不要だが、未解決作業の採用や既存の固定 declaration の自動再判断を許可するものではない。

## 追加を非 idempotent として扱う

`plan`、`capture`、`group plan`、`group capture` は毎回新しい Entity を割り当てる。正確な作成 payload を記録し、返された ID を観測してから後続 phase へ進む。

command の結果または割り当て ID が不明な場合、まず process が終了したことを確認する。`axon list --skip-command-evaluation` と candidate Entity を調査し、意図した正確な kind、declaration、parent、actor context、作成時刻を確認する。`--search` で保存済み literal text により候補を絞れるが、意味的な重複 check ではない。特定可能な新しい一致が 1 件なら、作成された Entity とみなす。一致がなければ同じ固定済み作成を 1 回再試行できる。複数の一致がありうるなら結果を不明として報告し、別の Entity を作成しない。
