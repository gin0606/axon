> 過去資料: 置換前の三軸CLIの手順。新lifecycle版では実行しない。現行の入口は同directoryの親のSKILL.mdを参照。

# Declaration workflow

declaration の file への export、canonicalize、apply、または declaration 操作の recover を行う前に、この reference を読む。

## declaration と Control state を分離する

declaration が編集するのは title、description、parent、outgoing dependency だけである。既存の `Accepted` または `Rejected` declaration は固定され、`axon import` もこの rule を迂回しない。実際の declaration 変更を適用する前に、呼び出し側 workflow は `axon-kit:triage` で Entity を `Undecided` に戻す必要がある。その後の Disposition または Resurface condition の変更も別途行う。

declaration の新規 record は、claim のない `Accepted`、`NotStarted`、`Always` の Entity を表す。意図する condition または adoption が異なる単一 Entity の作成には、与えられた初期 condition とともに `axon-kit:plan` または `axon-kit:capture` を使う。後続 transition が必要な場合、通常の Control-state 操作は分離したままとする。declaration import に合わせるためだけに、呼び出し側の意図する state を normalize しない。

combined workflow が部分的に完了した場合、最初の未解決 phase で停止する。適用済み storage state と残りの phase を報告し、自動的な補償や rollback を行わない。

## export と review

明示的な ID、`--group`、`--recursive` selector で editable set を定義する。それらの和集合が完全な edit set であり、関係の endpoint は自動的に editable にならない。

file への export では temporary path に書き、`axon export` の成功を確認し、既存 destination を置き換える前に結果へ `axon import check` を実行する。依頼に置き換えが含まれない限り、呼び出し側所有の既存 artifact を上書きしない。

read-only check では file を prepare または rewrite せず `axon import check <file>` を実行する。read-only review では original を変更せず保持し、必要なら temporary copy を prepare して、declaration 内容と報告された構造的・導出的影響の両方を調査する。これらの check は導出影響に必要な Command condition を評価する場合がある。Command を実行してよいことを別途確認した environment を使い、saved text だけの調査と誤認しない。

## external dependency または parent snapshot を追加する

offline の手順と例には `axon docs declaration` を使う。destination edit set を明示したままにする。external Entity を別途 export し、その id、base、title、完全な observed mapping を destination の references.entities に copy し、source の issues/groups list から kind を追加して key/description は省略する。snapshot value を創作したり、ID を解決するためだけに target を destination edit set へ加えたりしない。

destination relation と observed AfterEntity target に必要な snapshot だけを含める。参照先の observed state がさらに参照する Entity も再帰的にたどる。すべての source record や relation ではなく、必要な source references.entities record を再利用する。destination の readonly relation は destination edit set を指す external owner に属するため、その export から保持する。不要になった snapshot は削除する。正しく必要な snapshot の追加は、参照先 Entity の編集ではない。

new owner と既存の Undecided owner は、固定された external parent/prerequisite への editable edge を所有できる。prepare/check/apply/check 後、残りの変更がないことを要求し、external declaration/Control value が不変であることを検証する（その export に新しい incoming edge が現れる場合はある）。

file に ID がないことは active storage に存在しない証拠ではない。ID を確認して snapshot を export する。storage に存在しないことを立証するには active root を検証する。stale または不正な reference base では fresh export を保存・比較し、未解決 conflict を返す。base が一致する snapshot の mismatch では external state を変えず、export 済み kind/title/observed を復元する必要がある。

## canonicalize または apply

呼び出し側所有の source を recoverable に保つ必要がある場合、private temporary copy で作業する。`axon import prepare` は comment を破棄し canonical YAML に書き換える。

1. `axon import prepare <working-file>` を単独の artifact mutation として実行する。
2. editable set、割り当て ID、owned relation、readonly relation、external snapshot を含む書き換え後の YAML を調査する。
3. `axon import check <working-file>` を実行する。続行前にすべての error、warning、構造変更、導出影響を review する。
4. canonicalization だけの場合、source が変わっていないことを検証してから要求された destination を置き換える。`storage result: not applied` と報告して停止する。
5. storage へ適用する場合、check 済み working file が変わっていないことを確認し、`axon import apply <working-file>` を単独の storage mutation として実行する。
6. `axon import check <working-file>` を再実行し、残りの変更なしで成功することを要求する。変更した Entity と関係する導出作用を検証する。

## conflict と不確かな結果

fingerprint conflict を隠すため stale file に `prepare` を実行しない。stale file を保存し、fresh export を取得して、conflict する declaration field と関係を呼び出し側 workflow に返す。自動 merge policy を創作しない。

storage への適用が成功し declaration file の rewrite が失敗した場合、正確な file を保持し、同じ `axon import apply` を再実行する。Axon がこの再試行を受け入れるのは、active storage がすでに完全な declared result と一致する場合だけである。

command の完了が不明な場合、まず process の終了を確認し、working file を変更せず保持する。その readonly snapshot と owned value を current storage と照合する。Axon の recovery contract に従い、同じ保存済み apply file だけを再試行する。それ以外では `storage result: unknown` と報告する。

この操作で作成した temporary file だけを、その storage と artifact の結果が判明した後にだけ削除する。保持した file は正確な path と recovery purpose を報告する。
