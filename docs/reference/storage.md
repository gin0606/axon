# 保存と統合の契約

保存先は不変な記録の集合とし、記録 1 件を管理 root の `.axon/records/` の下の 1 file として保存する。現在値は保存せず、読取のたびに記録から導出する。Git の merge・rebase・cherry-pick・revert は記録 file の集合を変えるだけで、Axon は Git の統合時に呼ばれない。統合が生んだ衝突・構造の違反・記録の欠けは次の読取と `axon storage check` が検出し、衝突は `axon resolve`、違反は通常操作で直し、記録の欠けは情報として残る。実装の手順と失敗境界は [file 保存と Git 統合](../development/lifecycle-file.md) と [CLI と保存の接続](../development/lifecycle-cli.md)、対応するモデルは [`spec/record_integration.qnt`](../../spec/record_integration.qnt)（[モデルの読み方](../../spec/README.md#統合モデルが表す規則)）、この形にした理由は [設計判断](../design/decisions.md#保存層を記録の集合にしgit-統合を読取時の検出に委ねる理由) にある。

## 記録

記録は Entity ごとの不変な事実で、次を持つ。

- 対象の Entity ID。
- 記録の種類。登録、lifecycle 遷移（操作名を含む）、文面編集、label の設定、所属変更、dependency の追加と削除、再浮上条件の設定と解除、種類の変換、declaration の適用（title・description・label・parent・needs の変更をまとめて持つ）、解決、Note。
- 親記録の ID の集合。登録の記録は親を持たず、解決記録は衝突していた全 head を親にし、それ以外の記録は親を一つだけ持つ。Note は親を持たず、因果を持たない集合をなす。
- 日時（UTC の瞬間と小数秒）、任意の記録者情報（`actor` と任意の JSON `data`）、任意の理由。
- 操作後の現在値（Note 以外）。種類（Issue・Group）、保存値の lifecycle、`InProgress` の Issue なら着手した actor、title、description、label、再浮上条件、親 Group、outgoing dependency。label は固定集合（[label](lifecycle.md#label)）の値で、どの種類の記録の現在値も常に持ち、記録 file では `description` の次に置く。Note は本文と、同じ本文・日時・記録者の Note を別の記録にするための乱数（nonce）を持つ。

記録 ID は記録 file の bytes 全体の BLAKE3 hash を小文字 16 進 64 文字で表したもので、接頭辞を持たない。同じ ID なら同じ内容であり、file 名と内容を照合できる。Note ID も記録 ID である。Entity の短い ID（[識別子と入力](cli.md#識別子と入力)）とは別の契約で、CLI では完全な記録 ID を使う。

文面・label・関係・条件の編集も記録にする。lifecycle 遷移の記録は、直前の記録の現在値から操作後の現在値への変化として読める。種類の変換は lifecycle 遷移ではないが記録として残り、変換前後の種類が読める。lifecycle 遷移の記録の妥当性は、その遷移の時点の種類（直前の記録の現在値の種類）の規則で判定し、Entity の現在の種類では判定しない（[種類の変換](lifecycle.md#種類の変換)）。種類の規則に反する遷移の記録（Group の `Start`、Issue の `Undecided` からの `Complete` など）は、通常の writer が作ることはなく、読取は [保存先の破損](#保存先の破損) として扱う。遷移元は親記録の現在値なので、親記録が欠けている記録（[gap](#記録の欠けgap)）ではこの検査を行わない。

## 現在値の導出

Entity ごとの Note 以外の記録は因果 DAG をなす。そのうち、どの記録の親にもなっていない記録を head と呼ぶ。Note は head にも衝突にも関わらない。head が一つならその記録の現在値がその Entity の現在値で、その Entity は settled である。head が複数ある Entity は衝突中で、現在値を持たない。head の値が等しくても、同じ ID を両側で登録した二重登録でも衝突とし、黙って畳まない。

衝突は `axon resolve` の解決記録で解く。解決記録は全 head を親にし、head のうち一つの現在値を採る。値の捏造も項目ごとの合成もしない。解決の直後にその Entity は settled になる。両側で同じ衝突を別々に解決すると解決記録どうしが並行になり再び衝突するが、もう一段の解決で収束する。

Issue の `Start` の排他性は現在値で表す。`InProgress` の現在値は着手した actor を含み、同じ Issue の並行した `Start` は別の head として衝突に見える。同じ actor の並行した `Start` や、両側で同じ提案を `Accept` した場合も衝突として見え、二重作業に気づける。

Group の実効 lifecycle、一覧の状況、候補集合は settled な Entity の現在値から [lifecycle](lifecycle.md) と [候補と外部条件](candidates.md) の定義で導出する。衝突中の Entity は現在値を持たないため、その Entity を親や依存先に持つ Entity の判定では、祖先が採用済みであることや依存先が `Completed` であることを満たさない。「全祖先が採用済み」は親の連なりの各段が settled で保存値が `NotStarted` であることを要求し、連なりの途中（親の親など）に衝突中または記録の欠けた Entity があれば、その下の settled な祖先がすべて `NotStarted` でも満たさない。

読取は file の列挙順に依存しない。同じ日時の Note の表示順など、同順位の並びは記録 ID で固定する。

## 衝突と通常操作

衝突中の Entity が一つでもある保存先では、解決と Note の追加以外の操作を拒否する。読取はでき、一覧と詳細は衝突中の Entity を示す（[CLI と表示の契約](cli.md#衝突違反と解決)）。declaration の一括反映だけは、衝突に加えて違反のある保存先でも拒否する。例外は途中で止まった反映の再試行で、残りの反映で消える違反は拒否の対象にしない（[Declaration](declaration.md#目的と用途)）。

## 構造の違反と修復

settled な Entity の現在値がコアの構造の制約に反する箇所を、Entity ごとに種類を持つ「違反」の集合として導出する。種類は、包含の循環、親の不在（親が存在しない、または Group でない）、終了した親の下の未終了、`InProgress` の Issue か実効 `InProgress` の Group の未採用の祖先、`Completed` の Entity の未完了の依存先、依存先の不在、通常完了経路の循環である。違反は settled な保存先への通常操作からは生じない。両側の有効な操作を Git が組み合わせたとき（片側が Group を `Complete` し他側がその Group に子を登録した、両側の移動で循環ができた、片側が完了しつつ他側でその依存先を `Reopen` した、片側で Group を未採用の下へ移し他側でその配下を完了した）、cherry-pick や revert で記録の一部だけが入ったり消えたりしたとき（親 Group の登録の記録を伴わない子の記録など）、解決の選択が完了済みの相互依存を作ったときに生じる。

違反は通常操作を止めない。lifecycle 操作（`Reconsider`・`Reopen` を含む全遷移）、所属変更、dependency の追加と削除、登録、種類の変換、文面編集、label の設定、条件の設定、declaration の反映のすべての通常操作は、各操作の前提に加えて「操作後の違反が操作前の違反の部分集合」であることを要求する。所属変更、dependency の追加、登録は、これに加えて、変更で新しく持った関係（dependency、所属）が誘導する通常完了経路の前提の辺（依存先は自身と子孫にその依存先を待たせ、所属先は新しい子を待ち、新しく祖先の連なりに加わった Group は自身と子孫にその依存先を待たせる。移動の前後で共通の祖先の依存先は新しい関係ではない）のどれかが変更後の循環上に載る（辺の先から前提をたどって辺の元へ戻れる）なら、違反が増えなくても拒否する。辺は関係ごとに数え、別の関係がすでに同じ Entity の組を待たせていても検査する。そうでないと、祖先の dependency で待っている組に自身の dependency を重ねる変更が通り、祖先の dependency を外しても循環が残る。有効な保存先では循環上に載る辺は必ず違反を増やすので、この検査は部分集合の検査に含まれる。無関係な違反が残っていても操作は進む。

違反に含まれる Entity には修復のための免除を与える。終了した親の配下を変更しないという固定、`Completed` の dependency の固定（削除だけ。追加は免除しない）、`Completed` の依存元による `Reopen` の阻止を、その Entity が違反に含まれるときだけ免除する。免除は操作の前提を緩めるだけで、違反を増やす操作は免除の下でも通らない。違反は Entity と種類の組で数え、同じ Entity の同じ種類の違反を重くする操作（すでに未完了の依存先を持つ `Completed` の依存元に、もう一つ未完了の依存先を残す `Reopen` など）は違反を増やさない。`Completed` の Group の下の `Cancelled` の Issue が循環に含まれていても、その `Reconsider` は終了した親の下に未終了の Entity を作るため拒否される。`Completed` の依存元による阻止の免除が働くのは、その依存元がすでに未完了の依存先を持つ違反にあるときだけで、依存元にその違反を新しく作る `Reopen` は拒否される。修復は違反を減らす操作（解決記録、dependency の削除、依存元から順の `Reopen`、取り外し、再採用）で行う。終了した Group どうしの包含の循環は取り外しで、完了済みの相互依存は dependency の削除で直せる。免除は違反のない保存先では働かない。包含の循環と通常完了経路の循環の違反は循環上の Entity（包含または完了の前提をたどって自身に戻れる Entity）だけに付き、循環を待つだけの Entity は違反に含まれず、免除も受けない。待つ側まで含めると、待つ側どうしに新しい循環を足しても違反の集合が増えず、通常操作が循環を作れてしまう。すでに循環上にある Entity どうしの間の dependency の追加、所属変更、登録（同じ循環への chord や、別の循環の構成員どうしで新しい循環を閉じる辺）は、構成員を変えなくても新しい辺が循環上に載るため拒否する。これを通すと、元の循環を直した後に通常操作で足した循環が残る。循環を待つ Entity を経由して構成員を増やす追加も拒否し、循環上の Entity を待つだけの追加（循環を閉じない辺）は通る。解決記録は違反の検査を免除する。解決の選択で違反ができることがあり、それを解決時に拒否するとどの選択も拒否されて行き止まりになる場合があるためで、解決で head を一つに戻してから通常操作で直す。違反があり衝突がない保存先には、修復を確実に進める通常操作が一つはある。

Issue は通常操作では子を持たないが、記録の統合の結果として子を持つことがある（片側で子のない Group を Issue に変換し、他側でその Group に子を登録した、または移した場合など）。その子は親が Group でないため親の不在の違反に含まれる。子を持つ Issue の `Complete`・`Cancel` は、Group と同じく直属の子がすべて終了していることを要求する（[親子のlifecycle](lifecycle.md#親子のlifecycle)）。未終了の子を残して終了すると、終了した親の下に未終了の Entity が残るためである。子の終了は `Completed`・`Cancelled` のどちらでも満たす。

dependency の追加と完了は黙って混ざらない。dependency は依存元の Entity の記録なので、片側で dependency を足し他側でその Entity を完了すると、同じ Entity の異なる head になり衝突として見える。完了済みの Entity に未完了の依存先が黙って入るのは、依存先を他側で `Reopen` した場合に限る。

## 記録の欠け（gap）

cherry-pick と revert は commit に含まれる記録 file の単位で記録を持ち込み、または消す。後続の記録が親として参照する記録が保存先にないとき、その Entity は gap を持つ。親が欠けた記録、またはそれに続く記録は、受け手が持つ古い祖先の記録と並んで head になり、実際には祖先どうしなのに衝突として見える（欠けた記録の後に同じ Entity の記録が 2 件以上残ると、親の欠けた記録は head 自身ではなくその祖先になる）。記録は親しか知らないため、Axon はこれを本当の衝突と区別できない。これを受け入れ、`axon resolve` は、自身の親記録が欠けた head と、親をたどってもその Entity の最も古い記録（作成の記録、なければ親記録がすべて欠けた記録のうち日時が最も早いもの）に届かない head を「親記録が欠けていて新しい可能性が高い」と示す。普通の偽の衝突では、古い側は最も古い記録に直接届き、gap を解決した後に両側で操作した本当の衝突では、どの head も解決記録を通して届くので、どちらも示さない。記録は親しか知らないため、この印は推定であり、欠けた箇所が複数ある場合や解決記録の親が欠けた場合には、古い側や本当の衝突の側に付くこと、どの head にも付かないことがある。新しい方を選んで解決すれば正しい値になり、後で残りの記録が届いても値は変わらない。解決記録だけが先に届いた保存先でも、受け手が解決記録の親のどれかを持っていれば、解決記録が値を持つので現在値は決まり、gap として報告する。持っていなければ古い祖先と並ぶ偽の衝突になる。

gap は記録が消せないため解決記録では埋まらない。欠けた記録 file を Git の履歴から戻す（`git checkout <commit> -- <path>`）か、残りの記録が届けば埋まり、そのままでも読取と操作は続けられる。`axon storage check` は gap を情報として報告し、それだけでは非 0 にしない。記録のない Entity の Note（Note だけが cherry-pick された場合など）も同じく保持して情報として報告する。gap を作るのは、Axon の記録を含む commit を後から部分的に取り消す（revert、rebase での drop）か拾う（cherry-pick）操作と、未 commit の記録 file を残したまま別の commit へ移る操作（checkout・reset・stash。untracked の記録 file はそのまま残る）である。merge、rebase、squash merge、rebase merge は記録を追加するだけで、それ自体では gap を作らないが、片側が revert で消した記録 file の削除は merge で他側にも伝わり、他側にその記録を親とする後続の記録があれば gap になる。後続の記録がない記録の revert は記録を消すだけで、gap にも衝突にもならない。code と Axon の記録を同じ commit に入れる運用は無関係である。祖先集合を記録に持たせて偽の衝突をなくす案は、困ったときに後から足す。

## 保存先の破損

`.axon/records/` の下にある記録以外の file は次のように扱う。

- 名前が `.tmp` で終わる file は書込途中の一時 file で、読取と `axon storage check` は無視する。
- それ以外の、名前が記録 ID の形でない file（OS が作る file を含む）、名前と内容の hash が一致しない file、名前の先頭 2 文字と違う subdirectory にある file、途中で切れた file と空の file、規則外の内容の file（現在値に label がない記録、固定集合の外の label を持つ記録を含む）は保存先の破損とする。読取と `axon storage check` は衝突や違反とは別に破損として報告し、利用者が file を直すまで読取と全操作を止める。破損は Entity に属さないので、違反の免除は当てはまらない。
- 名前と内容の hash が一致しない file のうち、内容の CRLF を LF に戻すと名前の hash と一致するものは、Git の改行変換が疑われる破損として、その file の理由にその旨を書き、報告の末尾に改行変換の可能性と [保存先と worktree](../guide/storage.md#改行変換と-axongitattributes) への案内を一行添える。読取は内容を戻さず、破損として止める扱いは変えない。

`.axon/` 直下の header・`.gitignore`・`.gitattributes`・lock 以外の file は読まず、報告もしない。header file の欠落、読めない header、未知の format は破損と同じく操作を止める。未知の format は変換しない。

現在の header の format は `axon-records/v2` である（[保存形式ごとに検証規則を固定する理由](../design/decisions.md#保存形式ごとに検証規則を固定する理由)）。

## 保存先と初期化

`axon init [PREFIX]` は保存先を新規作成する専用の操作とする。配置や形式を選ぶ option は持たない。`.axon/` が存在しないか、中にあるのが lock file、`.tmp` で終わる file、空の記録の directory、`axon init` が書くのと同じ内容の `.axon/.gitignore` と `.axon/.gitattributes` だけの場合（中断した初期化の残骸）だけ作る。この 2 file の内容は CRLF を LF と読んで比べ、Git の改行変換による行末の違いは同じ内容とみなす。残骸の 2 file に CRLF があれば `axon init` が書く内容で置き換え、追跡する運用で CRLF のまま stage されないようにする。header file、記録、内容の異なる `.axon/.gitignore` か `.axon/.gitattributes`、その他の fileが一つでもあれば、内容が有効でも拒否してその path を示し、修復・暗黙の変換を行わない。

prefix は Entity ID の先頭に使い、ASCII の小文字英数字とハイフンだけを許す。空文字と、先頭・末尾のハイフンは受け付けない。明示した値は変換せずに検証する。省略した場合は管理 root の directory 名の ASCII 大文字を小文字にした結果を使い、それがこの規則に合わなければ保存先を作らずに失敗し、明示指定を求める。

`axon init` が作るのは次の四つで、いずれも `.axon/` の中にある。初期化を直列化する lock file（[初期化の対象](#初期化の対象)。Git 内では common Git directory に残る）を除き、`.axon/` の外には何も書かず、repository root の file と Git config には触れず、stage・commit もしない。

| path | 内容 |
| --- | --- |
| `.axon/records/` | 記録の directory。記録は記録 ID の先頭 2 文字の subdirectory の下に、記録 ID を file 名として置く |
| `.axon/header.json` | header。format の識別子（`axon-records/v2`）、store ID、prefix を持つ 1 行の JSON |
| `.axon/.gitignore` | `*.lock` と `*.tmp` の 2 行。lock と書込途中の一時 file だけを Git から除外し、記録と header は追跡できる |
| `.axon/.gitattributes` | `* -text` の 1 行。`.axon/` の下の file を Git の改行変換の対象から外す |

store ID と prefix は header file にだけ保持する。header file が保存先の目印であり、探索と `axon init` の拒否はこれで判定する。

`.axon/.gitattributes` は追跡する運用で記録 file の bytes を保つためにある。記録 ID は末尾の LF を含む file の bytes 全体の hash なので、repository root の `.gitattributes` の `text=auto eol=crlf` や Git の設定の `core.autocrlf=true`（Git for Windows では system の設定にある）のような Git の改行変換が checkout 時に LF を CRLF へ変えると、名前と内容の hash が一致しない破損になり、読取と全操作が止まる。`text` 属性を外すと Git は `core.autocrlf`・`core.eol`・`eol` 属性によらず改行を変換せず、作業 tree の属性 file は深い directory のものが優先されるので、`* -text` は root の `text` 属性も打ち消す。作業 tree の属性 file より優先される設定（repository ごとの `info/attributes`（`git rev-parse --git-path info/attributes` が示す file）で `.axon/` にかかる `text`・`text=auto`・`!text`、`.axon/.gitattributes` のない tree を属性の読み元にする `attr.tree` など）と、改行以外の変換（`filter`・`ident`・`working-tree-encoding` の属性）が `.axon/` にかかる場合は防げず、記録 file の bytes を変えて破損になる。この file は merge・union などの統合の属性を持たない。統合は属性に頼らず、衝突などは次の読取で検出する（[Git 統合の範囲](#git-統合の範囲)）。

### 無視する運用と追跡する運用

Git が保存先を無視するか追跡するかに Axon は関与しない。`axon init` の直後、header、`.axon/.gitignore`、`.axon/.gitattributes` は Git から untracked に見える（空の記録の directory は Git に現れない）。header があって記録の directory がない保存先は記録のない有効な保存先で、writer が必要なときに directory を作る。Git 内での保存先の使い方は二つあり、利用者の Git の運用だけで決まる。Axon は二つを区別せず、設定にも持たない。一つの repository では、どちらか一つに揃える。

- 無視する運用。利用者が `.git/info/exclude` や global の ignore などで `.axon/` を Git に無視させる。linked worktree には `.axon` が現れないため、下の探索順によって全 worktree が main worktree の保存先を共有し、worktree ごとに `axon init` を繰り返す必要はない。
- 追跡する運用。利用者が `.axon/` を stage・commit する。`.axon/.gitignore` が lock と一時 file を除くので、`git add .axon` で追跡対象になるのは記録、header、`.axon/.gitignore`、`.axon/.gitattributes` である。各 worktree は checkout した自分の保存先を持つ。変更は Git で取り込むまで他の worktree から見えず、同じ Issue に別々に着手できる。

`axon init` は Git 内では二つの運用の手順と、Axon の状態の取り消しに revert を使わないことを表示するだけで、repository root の `.gitignore`・`.gitattributes`・Git config を作成も編集もしない。Git の属性として持つのは `.axon/.gitattributes` の改行変換の抑止だけで、統合は Git の属性や設定に頼らない。

Git は untracked な file を checkout・merge の上書きから保護するが、無視されている file は保護しない。無視されている保存先がある作業 directory で、`.axon/` を追跡している commit を checkout・merge すると、Git は警告なしに file を置き換え、記録が失われる。これは無視する運用と追跡する運用を一つの repository で混ぜた場合にだけ起きる。Axon はこの混在を検出しない。

### Git 統合の範囲

追跡する運用での統合は、ローカルの Git 操作（merge・rebase・cherry-pick・revert・squash）に対する契約である。両側が記録を追加した branch は、記録が別 file なので merge の属性や Git の設定なしで衝突せず統合される。記録 ID は内容の hash なので、同名の file は同じ内容であり、両側が同じ file を追加しても衝突しない。同じ Entity への両側の操作は Git 上では衝突せず、次の読取で Axon の衝突として見える。cherry-pick と revert の効果は [記録の欠け](#記録の欠けgap) に定める。

ホスティングサービスの web 上の merge はこの契約の外にある。GitHub の merge button と Update branch が、この形式では merge の属性なしで両側の記録を取り込むことを 2026-09-25 に確認している（[設計判断](../design/decisions.md#記録-1-件-1-file-にした理由)）。

Git の revert で Axon の状態を取り消さない。revert は記録 file を消すだけで、後続の記録があれば gap と偽の衝突になり、状態は戻らない。Axon の状態の取り消しは `Reopen`・`Release`・`Reconsider` などの lifecycle 操作で行う。

### 探索

Git 内では現在の repository を探索境界とし、次の順で保存先を選ぶ。

1. 現在の worktree root の `.axon`。
2. main worktree の `.axon`。現在の worktree が linked worktree で、Git common directory が main worktree 直下の `.git` directory である場合だけ探す。bare repository に付けた worktree と submodule では探さない。

各段では、header file があればその保存先に確定する。header がなく、記録やその他の fileがあれば、破損または未知の format として停止する。中断した初期化の残骸（lock、`.tmp` で終わる file、空の記録の directory、`axon init` が書くのと同じ内容の `.gitignore` と `.gitattributes`。内容は CRLF を LF と読んで比べる）しかない `.axon` では確定せず次へ進む。どの段でも確定しなければ未初期化とする。確定した保存先が破損・読取不能であれば停止し、別の保存先へ fallback しない。

追跡する運用で、保存先を持たない branch（`axon init` より前に分岐した branch など）の linked worktree から操作すると、2 によって main worktree の追跡対象の保存先に記録を書く。変更は main worktree の差分として見え、記録は失われないため、この副作用は許容する。main worktree で保存先を持たない branch を checkout した場合は未初期化になる。既存の store を使うには保存先を Git で取り込む。そこで `axon init` を実行すると別の store の新規作成になり、後で一つの保存先に統合できない（header が衝突し、記録は別の store のものになる）。

通常操作の OS lock、Git index の unmerged 検査、管理 directory が通常の directory であることの検査（symlink の拒否）は、現在の worktree ではなく、確定した保存先と、それを含む worktree に対して行う。保存先の確定より前の unmerged 検査は、次の段落のとおり header のない段に対して行う。

Git 内では、header file のない段ごとに、header の有無による確定と破損・未知の format・未初期化の判定より先に、その段の worktree の Git index で `.axon/` の下に unmerged な path があるかを検査する（`.axon` に記録などがある場合、残骸しかない場合、`.axon` が存在しない場合のいずれも）。unmerged なら次の段へ進まずに停止し、unmerged を報告する。統合が未解決なまま header が作業 tree から消えた保存先では、header の欠落や未初期化より Git での解決と stage が要ることを示し、その worktree が追跡する保存先を飛ばして次の段の保存先を使わないためである。unmerged の報告は、unmerged な path と、その index を持つ worktree の path を示す単独の診断で、header が作業 tree になくても header の欠落を破損として併記しない。header の有無と破損は、Git での解決と stage の後の探索と読取で判定するためである。linked worktree から main worktree の index で止まった場合も、どの worktree で解決するかが分かる。header file のある段では保存先に確定し、その保存先の unmerged 検査は読取の前に行う。

Git 外では最寄りの header file を持つ祖先を管理 root とし、中断した初期化の残骸しかない `.axon` では探索を止めない。header がなく記録やその他の file がある `.axon` は破損または未知の format として停止する。

### 初期化の対象

`axon init` は探索の 2 を使わず、Git 内では現在の worktree root、Git 外では現在 directory を対象とする。Git 外では既存管理 root 内の入れ子初期化を拒否する。

探索の 2 が働く構成の linked worktree での `axon init` は、main worktree に保存先が既にあれば拒否し、その path を示す。無視する運用では手前に保存先ができて読む先が気づかないまま切り替わり、追跡する運用では別の store ができて後で統合できなくなるためである。main worktree に保存先がない場合と、探索の 2 が働かない構成では拒否せず、作成した保存先が他の worktree からは見えないことを表示する。ただし main worktree の Git index で `.axon/` の下に unmerged な path があれば、統合が未解決なまま header が消えた main worktree の保存先がありうるため、unmerged として拒否する。main worktree を判定できなかった場合は、未初期化として扱わずにエラーとする。それ以外の取り違え（読む先の思い違い、worktree の削除による保存先の消失）は利用者の運用に委ね、`axon init` は既存の保存先を壊さないことだけを保証する。

`axon init` は OS lock（Git 内では common Git directory、Git 外では `.axon/` に置く lock file。file は残る）の下で既存の保存先を確認し、記録の directory を作り、`.axon/.gitignore` と `.axon/.gitattributes` をそれぞれ一時 file から rename で作り、最後に header file を一時 file から rename で公開する。header が公開される前に中断した保存先には記録がなく、header もないので未初期化のままであり、再実行で作り直せる。成功時は作成した header file の path を示す。

## 書込

書込は記録 file の作成だけで行う。既存の記録 file を書き直す、置き換える、削除する機構は持たない。writer は保存先の OS lock を取り、記録の集合を読み、通常操作の前提を検査し、新しい記録を目的の subdirectory に一時 file `<記録 ID>.tmp` として書いて sync し、記録 ID へ rename して directory を sync する。一つの操作が作る記録は一つで、Note の追加も同じ境界で直列化する。例外は `axon import apply` で、一回の lock の下で検証済みの複数の記録（Entity ごとに一つ）を、新規 Entity の登録を親と依存先が先になる順に、次いで既存 Entity の変更の順に公開する（[Declaration](declaration.md#preparecheckapply)）。

lock は `.axon/write.lock` で、writer は置換・削除しない。Git や editor は OS lock に従わないため、同じ worktree で checkout・merge・editor 保存と Axon の書込を並行しない。

## 検査と解決の入口

`axon storage check [ROOT]` は保存先の破損、衝突、違反、gap を報告し、破損・衝突・違反のいずれかがあれば非 0 で終了する。通常操作を変えず、条件コマンドを実行しない。追跡する運用の CI はこれを保存先に対して実行する。

`axon resolve` は衝突中の Entity と head を示し、`axon resolve ID --head RECORD_ID` が解決記録を書く。解決の後に残る違反は、`Reopen`・取り外し・dependency の削除・再採用などの通常操作で直す。入出力は [CLI と表示の契約](cli.md#衝突違反と解決) に定める。統合の検査・解決で外部条件コマンドを実行しない。
