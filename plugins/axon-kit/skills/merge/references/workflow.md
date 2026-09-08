# Axon snapshot merge workflow

Axon file-backend merge の prepare、resolve、check、apply、recover を行う前に、この reference を読む。

## 完全な入力を固定する

base、ours、theirs の明示的で完全な 3 snapshot を使う。未使用の workspace directory と、その workspace 外の output path を選ぶ。正確な入力 byte 列を保存し、workspace copy、manifest、fixed context、output preimage を編集しない。

`axon merge prepare --base <base> --ours <ours> --theirs <theirs> --output <output> --workspace <unused-workspace>` を単独の artifact mutation として実行する。nonzero result でも workspace が作られ、有用な original と diagnostic が保存される場合がある。prepare が未適用、未解決、不明のどれかを判断する前に workspace を調査する。同じ workspace 名で再実行しない。

manifest は入力の absolute path と digest、output と preimage、該当する場合は active file-store binding、evaluation context を固定する。`choices.json` は自動選択を含む stable conflict ID と input-choice ID を保持する。`report.json` は valid、unresolved、input drift、invalid の結果を区別する。`candidate.jsonl` と check 済み metadata が完全に valid な結果を示す場合だけ candidate を公開できる。

## original を書き換えず解決する

`resolution.json` だけを編集する。選択には manifest の input digest を使う。`ours` と `theirs` は会話上の label であり、選択値として指定できる識別子ではない。Base は比較証拠であり、current result として選択できない。title や最新 timestamp で選ばず、完全な Entity bundle と関係する record ID を review する。

呼び出し側の依頼または与えられた判断によって正確な作用が確定している場合だけ repair を使う。support される repair は、dependency 変更、Note 追加、状態変更、start など Axon の通常の guarded operation を workspace の fixed context で使う。historical record の編集、claim の捏造、固定 declaration の transition rule の迂回を行わない。merge conflict は Disposition、declaration、dependency、work-state の判断を許可しない。

resolution を編集するたびに `axon merge check <workspace>` を実行する。candidate 全体を再計算し、input、resolution、context に drift があれば以前の approval を無効にする。残るすべての conflict と、結果の Entity state、関係、history、claim、store identity を調査する。新たに与えられた判断または検証済み訂正で進展がある間だけ繰り返す。

## check 済み candidate だけを公開する

公開の直前に、input、resolution、check 済み candidate、destination preimage、backend、store identity、output path が引き続き workspace と一致することを検証する。`axon merge apply <workspace>` を単独の storage mutation として実行する。

Apply は最後に check した valid candidate だけを公開する。結果の stage や Git の続行は行わない。output byte 列と store identity を検証し、`axon storage check <output>` を実行して、影響を受ける Entity を調査する。同じ candidate がすでに output にある場合、検証済みの再実行は no-op になりうるが、保存済み candidate と destination の一致なしにその場合だと推測しない。

公開が destination に到達した可能性はあるが完了が不明な場合、workspace と output の全体を保存する。再試行前に process の終了を確認し、candidate、destination、backend、記録済み digest を比較する。destination を手動で置き換えたり、drift を隠すため workspace を再生成したりしない。

## Git driver conflict を復旧する

low-level driver は raw input を `.axon/merge/<id>` 配下に保存できる一方、その `%A` output は Git temporary path である。その driver workspace を `.axon/state.jsonl` に直接 apply しない。保存済みの完全な input または検証済み Git stage 1/2/3 snapshot から新しい明示的な workspace を作り、実際の state file を output に設定する。

検証済みの明示的 apply 後、state file に対して `axon storage check` を実行する。`git add`、commit、merge/rebase の続行、abort を判断・実行するのはこの capability ではなく呼び出し側である。index entry が unmerged の間、通常の Axon 操作は引き続き blocked となる。

## 停止条件

意味上の選択が不足、repair が権限を拡張、必要な入力が不完全、drift を照合不能、公開結果が不明、または check の反復に進展がない場合は停止して workspace を保持する。どの入力が authoritative か、どの phase が完了したか、次に必要な判断または観測を報告する。
