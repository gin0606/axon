---
name: axon-declaration-plan
description: Entity 数を問わず axon tracker data の strict YAML 宣言を操作し、axon export/import の review、canonicalize、check、apply を安全に行う。コマンド自体の実装・文書・テストや、宣言を使わない単一 Entity の通常操作には使わない。
---

# Plan 宣言を安全に編集する

宣言された Entity の plan declaration を一つの snapshot で検査し、原子的に反映する。編集対象は title、description、parent、outgoing dependency だけであり、Progress、Disposition、Resurface condition、claim、Declaration Revision、Note は変更しない。Revision は決定時に axon が作る不変記録、Note は別 command で追加する追記専用情報であり、宣言 file から作成・編集・削除しない。

この skill は内容を決める手順ではなく、確定した計画の反映経路である。内容の決定または DB への登録・apply が依頼範囲にあるとき、新規 Accepted Entity の目的、重複、分解、採否は `axon-plan-issue`、新規 Undecided Entity は `axon-capture-issue`、新規 Rejected Entity は `axon-capture-issue` で完成前の declaration を作ってから `axon-triage-issue`、既存 Entity の判断変更は `axon-triage-issue` の規約で先に確定する。独立した追加情報は宣言へ入れず、別途明示された範囲で `axon-add-note` を使う。artifact-only の相談では plan / capture / triage の調査と相談だけを使い、reflection command を実行しない。`capture`、`decide`、`when`、declaration apply、Note 追加など live DB の変更は、その変更自体をユーザーが明示的に依頼した場合だけ行う。読み取り専用の review / check と形式だけの canonicalize は、採用や状態変更を意味せず、plan / triage を要求しない。ユーザーが完成した宣言内容を明示した場合も、apply 前の重複・履歴・境界・波及の調査は省略せず、変更のない内容を再提示して確認する工程だけを省略してよい。

live DB の変更を合意して plan / capture / triage を併用するときは、次の順序を守る。

1. plan / triage skill の調査、相談、最終案確認までを行う。既存 Entity は `axon show` の全出力に加え、`axon revision list` と各 `axon revision show` で全 Declaration Revision を読み、表示された全 Note も省略しない。
2. 最終状態が Accepted / NotStarted / Always で宣言から直接作成できる Entity と、staging 済み Undecided Entity の title、description、parent、dependency の後続する個別反映コマンドは実行せず、宣言 apply に置き換える。直接作成できない新規 Undecided / Rejected と、最終 resurface condition が Always でない新規 Accepted は後述の staging 手順を使う。
3. 可能なら、状態軸を変える前の現状でも宣言する構造を preflight する。
4. declaration の実変更を伴う場合は、活性化・非活性化の別や既存の fixed / Undecided を問わず、Disposition を Undecided に保ったまま declaration apply と全 `show` 出力の確認を行う。全 Revision も `axon revision list|show` で確認する。最終 resurface condition の変更も Undecided の間に通常コマンドで反映・検証し、最終 Accepted / Rejected の判断を最後に行う。最終 Undecided なら再判断しない。この順序により、完成前の declaration が一時的に ready / active になることを防ぐ。
5. declaration の実変更を伴わない状態軸変更では、Accepted 化や resurface condition の充足など ready / active にし得る変更は必要な現状検証の後に行い、Rejected / Undecided 化や未充足の resurface condition など ready / active から外す変更は先に反映・検証する。最終状態の artifact が必要なら状態変更後に fresh export し、先行変更前の export を再利用しない。

既存の Accepted / Rejected Entity に declaration の実変更がある場合、import は固定を迂回せず拒否する。triage flow で変更内容と最終 Disposition に合意したうえで、理由付き `decide undecide` を先に単独で反映し、fresh export から宣言 workflow を実行する。apply 後の全文と関係を確認してから、最終 Accepted / Rejected なら別の理由付き判断として反映し、最終 Undecided なら再判断せず draft のまま止める。宣言が同じ no-op だけならこの再判断を行わない。前後の判断と apply は一つの transaction にはならないため、各 phase 後に状態を確認し、途中失敗は下記の partial-completion として扱う。prepare / check 後は、新しく割り当てた ID を含む editable Entity の正確な ID 集合と key 対応を控える。最終判断がある場合、Undecided apply 後の file を intermediate apply working file と呼び、ユーザー所有の宣言 artifact をまだ置き換えない。最終判断後は intermediate を stale で再 apply 不可とし、控えた全 ID を明示して別の same-filesystem final-state working file へ fresh export する。動的な `--group` / `--recursive` selector は再利用しない。final-state working file の editable ID / key 集合が控えた意図と完全に一致し、no-diff check が通ることを確認する。そのうえで、作業開始時からの原本の content drift、symlink / hardlink、metadata を後述のユーザー所有 file の手順で再確認し、final-state working file で一度だけ atomic replace する。最終 refresh または置換が失敗したら原本を変更せず、intermediate と final-state working file の両方を保持し、各 path、stale / replayable の別、反映済み DB 状態を報告する。

組み合わせの判定表は次のとおりとする。

- 既存 fixed + 実変更 + 最終 Accepted / Rejected: Undecided へ戻す → apply → 全文確認 → 最終 resurface condition を反映・確認 → 最終判断
- 既存 fixed + 実変更 + 最終 Undecided: Undecided へ戻す → apply → 全文確認 → 最終 resurface condition を反映・確認して終了
- 既存 Undecided: apply → 全文確認 → 最終 resurface condition を反映・確認 → 合意がある場合だけ最終判断
- 新規 Accepted / NotStarted / Always: declaration から直接作成
- 新規 Accepted + Always 以外の resurface condition: capture → Undecided の間に apply → 全文確認 → resurface condition を反映・確認 → Accepted
- 新規 Undecided / Rejected: capture → Undecided の間に apply → 全文確認 → resurface condition を反映・確認 → Rejected のみ最終判断

状態軸変更と declaration apply は順序を問わず一つの transaction にならない。どちらかの phase を反映した後に残りの phase が失敗したら、そこで止まり、作業 file を保持し、反映済み変更と未反映部分を列挙する。自動で巻き戻さず、再試行または別途承認された補償変更のどちらにするかをユーザーへ確認する。

宣言内の readonly field を変えた入力や check error は宣言の誤りとして止める。Declaration Revision や Note を表す field を足した入力も、宣言 file の範囲外として apply しない。import / canonicalize の依頼や readonly field の編集自体を、`decide` / `when` / `note add` など別の変更への許可とみなさない。その状態判断を別に行う場合は、ユーザーが triage flow で明示的に結論を出したときだけ上記の二段階手順へ進む。

宣言から直接作成できるのは Accepted / NotStarted / Always の Entity だけである。新規 Undecided / Rejected や、Always 以外の resurface condition を持つ新規 Accepted を Accepted / Always に正規化しない。読み取り専用の review では契約外だと報告する。登録まで明示的に依頼された場合は、それらを `axon-capture-issue` で Undecided として先に作成する。fresh export から完全な declaration を apply・確認し、最終 resurface condition の変更があれば Undecided の間に反映・検証する。最終 Undecided ならそこで終了し、Accepted / Rejected の判断も明示されている場合は、最後に `axon-triage-issue` で理由付き判断を反映・検証する。固定後に declaration を apply しない。prepare / check 後は capture した ID と宣言内で新しく割り当てた ID を含む正確な editable ID 集合と key 対応を控える。この staging、declaration apply、状態変更は非原子的なので、後続が失敗した場合は上記の partial-completion 手順で引き渡す。最終判断を伴うユーザー所有 artifact は、Undecided apply 後に置き換えず、控えた ID を明示した最終判断後の fresh export に対して上記と同じ安全な一度だけの置換手順を使う。

## 編集面を作る

既存 Entity を含むときは、先に各 Entity の `axon show` と全 `axon revision list|show` を省略せず読み、明示 ID、`--group`、必要なら `--recursive` を組み合わせて `axon export` する。selector の和集合だけが編集対象であり、relation endpoint は自動的に編集対象にならない。作業用 YAML はリポジトリへ残す成果物と合意されていない限り、一時ファイルとして扱う。

新規 plan を含む場合は、先に repository root の `docs/declaration-file.md` を最後まで読み、canonical example と field 契約を使って宣言を作る。最低限の top-level shape は `schema`、`issues`、`groups`、`relations.editable`、`relations.readonly`、`references.entities` であり、省略しない。新規 Entity は `id: null`、file 全体で一意な `key`、`base: null`、NotStarted / Accepted / Always / claim null で記述する。title、description、親、outgoing dependency 以外の保存状態を編集せず、Revision や Note の field を加えない。外部 snapshot と incoming relation は `references` / `relations.readonly` のまま保つ。

export だけを依頼された場合は、出力先が stdout、エージェント所有の一時 file、ユーザー指定 artifact のどれかを先に確定し、既存のユーザー file を明示許可なく置換しない。file へ出す場合は destination と同じ filesystem のエージェント所有 working file に stdout を受け、export 成功と `axon import check <working-file>` の成功・変更差分なしを確認してから rename する。既存の user-owned destination では後述の symlink / hardlink、metadata、source drift の規則も守り、失敗時は原本を変更しない。selector の範囲、editable / readonly 境界、参照 snapshot を確認し、出力内容または正確な保存先、`DB applied: no`、artifact の所有・保持状態を報告する。それ以上の編集や import を依頼されていなければ終了する。

## 検査して反映する

読み取り専用の check だけを依頼された場合は、`axon import check <file>` を実行して結果を報告する。non-canonical と判定されてもユーザー所有 file を prepare せず、rewrite が必要なことを報告する。

読み取り専用の review を依頼された場合は、原本を変更せず、必要なら一時 copy に prepare してから check する。YAML の意図を依頼内容と比較し、編集対象、title / description、完全宣言された parent / outgoing dependency、readonly 境界、structural / derived impact、warning を確認して finding を報告する。mechanical check の成功だけを review 完了とみなさない。

apply または canonicalize が依頼範囲に含まれる場合だけ、次へ進む。

ユーザー所有 file は原本の内容と metadata を控え、同じ filesystem 上のエージェント専用 working directory に置いた working copy に対して prepare / check / apply する。directory と file は他 actor が書き込めない権限にする。symlink または hardlink は自動置換せず、扱いをユーザーに確認する。

通常 file では、作業前に mode、owner/group、ACL、全 xattr、macOS / BSD file flags を列挙して保存する。content rewrite に伴う mtime / ctime は保持対象にしない。prepare / apply は atomic rewrite で ACL / xattr / flags などを落とし得るため、最後の rewrite 後に保存した metadata を working file へ再適用し、原本との一致を各項目で検証してから rename する。利用環境や権限のため一項目でも保存・再適用・検証できなければ原本を置換せず、失われる項目を明示してユーザーの許可を得るか、working copy を保持して停止する。原本は成功まで変更しない。canonicalize 成功時、または apply と検証の成功後だけ、原本が作業開始後に変わっていないことも確認して canonical working copy で atomic replace する。置換に失敗したら working copy を保持して報告する。

1. prepare が comment を破棄して file を canonical rewrite することを踏まえ、`axon import prepare <working-file>` を単独で実行する。DB が変わっていないことを前提に次へ進む。
2. rewrite 後の YAML を読み、編集対象、完全宣言された親と outgoing dependency、readonly 境界が意図どおりか確認する。
3. `axon import check <working-file>` が成功することを確認し、構造差分、ready / blocked / orphaned / active scope / Group 完了可能性の変化、警告を確認して出力と working file の byte digest を apply 後の照合用に保持する。確認済みの最終案に含まれない重大な warning や structural / derived impact があれば apply せず、ユーザーの判断を得る。

canonicalize / prepare だけが依頼範囲なら、step 3 の成功後に user-owned 原本の drift と metadata を再確認し、上記の安全な atomic replace を行う。`DB applied: no`、file 内だけで割り当てた ID、置換結果を報告し、working copy を規則どおり片付けて終了する。以下の apply 手順へ進まない。

4. ユーザーが依頼した変更の範囲に apply が含まれる場合だけ、状態変更の直前に working file の bytes が step 3 の digest と一致し、ユーザー所有原本の内容と relevant metadata も作業開始時から変わっていないことを再確認する。どちらかが変わっていれば apply せず、再調整後に check と impact 確認からやり直す。一致していれば `axon import apply <working-file>` を状態変更コマンドとして単独で実行する。
5. apply の差分出力が保持した check 出力と一致することを確認する。違いがあれば成功扱いにせず原因を調べる。
6. apply 後の working file に `axon import check <working-file>` を実行し、成功かつ変更差分なしであることを確認する。そのうえで base と observed が更新されたこと、および最初の check が示した全 structural / derived impact を `axon show` / `axon list` / `axon ready` / `axon triage` で確認する。変更した各 Entity の `axon show` は全出力を読み、全 Declaration Revision を `axon revision list|show` で確認する。事前に読んだ Note が同じ Entity 内番号と内容で残っていることを確かめ、後続番号の新しい Note は独立した並行追記として保持・報告し、declaration apply の不一致とは扱わない。

check の競合を prepare で上書きして進めない。stale file、現行 DB の fresh export、stale file で意図した最終値を三者比較する。fresh export と意図した編集が重ならない部分だけ自動で載せ直し、同じ field / relation が双方で変わった場合や derived impact が変わった場合はユーザーの判断を得る。DB commit 後の file rewrite 失敗が明示された場合は、元の YAML を保持して同じ apply を再実行する。axon は宣言内の全 owned 値と readonly snapshot / relation が commit 済みの最終状態に一致するときだけ file 更新を復旧する。

apply command の出力を失い commit 成否が不明なら、`DB applied` を推測しない。先の process が終了したことを確認し、working file を編集せず bytes と path を保持して、宣言内の readonly snapshot / relation と現在の DB を読み取りで照合する。その後、同じ working file への apply だけを再実行してよい。未適用なら通常 apply、適用済みで file 未更新なら復旧、競合変更があれば拒否される。再試行の成否も確認できなければ `DB applied: unknown` として停止し、保持 path と次の安全な確認手順を報告する。

## 一時ファイル

apply 成功後、宣言 file を永続的な成果物として残す合意がなければ、結果確認後にエージェント自身が作った一時ファイルだけを片付ける。最終判断を伴う場合は、final-state working file による置換と検証が成功した後に、intermediate と final-state の両方のエージェント所有一時 file を片付ける。ユーザーが用意した file は削除しない。失敗中、競合調査中、または再 apply に必要な file は片付けない。最終判断後の stale intermediate は復旧用の証拠としてのみ保持し、再 apply に使わない。

## 引き渡し

ユーザーへ `DB applied: yes/no/unknown` を明記し、実際に DB へ作成・変更した ID と kind、各 key から prepare 後 ID への対応、check/apply で確認した structural / derived impact と warning、競合や未解決事項を報告する。apply していない場合、prepare で割り当てた ID は file 内だけで未登録だと明記する。最後に、宣言原本を置換したか、ユーザー所有として変更していないか、intermediate / final-state working file を保持または削除したかを別々に明記する。working file を保持する場合は正確な path、保持理由、stale / replayable の別、同じ apply を安全に再実行できるか、次に有効な command も伝える。
