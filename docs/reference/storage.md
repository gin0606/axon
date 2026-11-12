# 保存と統合の契約

保存先は不変な記録の集合とし、記録 1 件を管理 root の `.axon/records/` の下の 1 file として保存する。現在値は保存せず、読取のたびに記録から導出する。Git の merge・rebase・cherry-pick・revert・squash は記録 file の集合を変えるだけで、Axon は Git の統合時に呼ばれない。統合が生んだ衝突・構造の違反・記録の欠けは次の読取と `axon storage check` が検出し、衝突は `axon resolve`、違反は通常操作で直し、記録の欠けは情報として残る。この契約が対象にするのはローカルの Git 操作で、ホスティングサービスの web 上の merge は対象外とする。対応するモデルは [`spec/record_integration.qnt`](../../spec/record_integration.qnt)、この形にした理由は [設計判断](../design/decisions.md#保存層を記録の集合にしgit-統合を読取時の検出に委ねる理由) にある。

## 記録

記録は Entity ごとの不変な事実で、次を持つ。

- 対象の Entity ID。
- 記録の種類。登録、lifecycle 遷移（操作名を含む）、文面編集、label の設定、所属変更、dependency の追加と削除、再浮上条件の設定と解除、種類の変換、declaration の適用、解決、Note。
- 親記録の ID の集合。登録の記録は親を持たず、解決記録は衝突していた全 head を親にし、それ以外の記録は親を一つだけ持つ。Note は親を持たず、因果を持たない集合をなす。
- 日時、任意の記録者情報、任意の理由。
- 操作後の現在値（Note 以外）。種類（Issue・Group）、保存値の lifecycle、`InProgress` の Issue なら着手した actor、title、description、label、再浮上条件、親 Group、outgoing dependency。Note は本文を持つ。

記録 ID は記録 file の bytes 全体の BLAKE3 hash を小文字 16 進 64 文字で表したもので、接頭辞を持たない。同じ ID なら同じ内容であり、file 名と内容を照合できる。Note ID も記録 ID である。Entity の短い ID（[識別子と入力](cli.md#識別子と入力)）とは別のもので、CLI では完全な記録 ID を使う。

lifecycle 遷移の記録は、直前の記録の現在値から操作後の現在値への変化として読める。その妥当性は遷移の時点の種類（直前の記録の現在値の種類）の規則で判定し、Entity の現在の種類では判定しない（[種類の変換](lifecycle.md#種類の変換)）。種類の規則に反する遷移の記録（Group の `Start`、Issue の `Undecided` からの `Complete` など）は、読取が [保存先の破損](#保存先の破損) として扱う。親記録が欠けている記録（[gap](#記録の欠けgap)）では遷移元が分からないため、この検査を行わない。

## 現在値の導出

Entity ごとの Note 以外の記録は因果 DAG をなす。そのうち、どの記録の親にもなっていない記録を head と呼ぶ。Note は head にも衝突にも関わらない。head が一つならその記録の現在値がその Entity の現在値で、その Entity は settled である。head が複数ある Entity は衝突中で、現在値を持たない。head の値が等しくても、同じ ID を両側で登録した二重登録でも衝突とし、黙って畳まない。

衝突は `axon resolve` の解決記録で解く。解決記録は全 head を親にし、head のうち一つの現在値を採る。値の捏造も項目ごとの合成もしない。解決の直後にその Entity は settled になる。両側で同じ衝突を別々に解決すると解決記録どうしが並行になり再び衝突するが、もう一段の解決で収束する。

Issue の `Start` の排他性は現在値で表す。`InProgress` の現在値は着手した actor を含み、同じ Issue の並行した `Start` は別の head として衝突に見える。同じ actor の並行した `Start` や、両側で同じ提案を `Accept` した場合も衝突として見える。

Group の実効 lifecycle、一覧の状況、候補集合は settled な Entity の現在値から [lifecycle](lifecycle.md) と [候補と外部条件](candidates.md) の定義で導出する。衝突中の Entity は現在値を持たないため、その Entity を親や依存先に持つ Entity の判定では、祖先が採用済みであることや依存先が `Completed` であることを満たさない。「全祖先が採用済み」は親の連なりの各段が settled で保存値が `NotStarted` であることを要求し、連なりの途中（親の親など）に衝突中または記録の欠けた Entity があれば、その下の settled な祖先がすべて `NotStarted` でも満たさない。

## 衝突と通常操作

衝突中の Entity が一つでもある保存先では、解決と Note の追加以外の、記録を書く操作を拒否する。読取はでき、一覧と詳細は衝突中の Entity を示す（[CLI と表示の契約](cli.md#一覧)）。記録を書かない [条件の許可](candidates.md#条件の許可) の操作はこの規則の対象外とする。declaration の一括反映は、衝突に加えて違反のある保存先でも拒否する（[Declaration](declaration.md#保証と扱わないもの)）。

## 構造の違反と修復

settled な Entity の現在値がコアの構造の制約に反する箇所を、Entity ごとに種類を持つ「違反」の集合として導出する。種類は、包含の循環、親の不在（親が存在しない、または Group でない）、終了した親の下の未終了、`InProgress` の Issue か実効 `InProgress` の Group の未採用の祖先、`Completed` の Entity の未完了の依存先、依存先の不在、通常完了経路の循環である。違反は settled な保存先への通常操作からは生じず、Git が両側の有効な操作を組み合わせたとき、cherry-pick や revert で記録の一部だけが入ったり消えたりしたとき、解決の選択で生じる。

違反は通常操作を止めない。lifecycle 操作（`Reconsider`・`Reopen` を含む全遷移）、所属変更、dependency の追加と削除、登録、種類の変換、文面編集、label の設定、条件の設定、declaration の反映のすべての通常操作は、各操作の前提に加えて「操作後の違反が操作前の違反の部分集合」であることを要求する。所属変更、dependency の追加、登録は、これに加えて、変更で新しく持った関係（dependency、所属）が誘導する通常完了経路の前提の辺（依存先は自身と子孫にその依存先を待たせ、所属先は新しい子を待ち、新しく祖先の連なりに加わった Group は自身と子孫にその依存先を待たせる。移動の前後で共通の祖先の依存先は新しい関係ではない）のどれかが変更後の循環上に載る（辺の先から前提をたどって辺の元へ戻れる）なら、違反が増えなくても拒否する。辺は関係ごとに数え、別の関係がすでに同じ Entity の組を待たせていても検査する。無関係な違反が残っていても操作は進む。

違反に含まれる Entity には修復のための免除を与える。終了した親の配下を変更しないという固定、`Completed` の dependency の固定（削除だけ。追加は免除しない）、`Completed` の依存元による `Reopen` の阻止を、その Entity が違反に含まれるときだけ免除する。免除は操作の前提を緩めるだけで、違反を増やす操作は免除の下でも通らない。違反は Entity と種類の組で数え、同じ Entity の同じ種類の違反を重くする操作（すでに未完了の依存先を持つ `Completed` の依存元に、もう一つ未完了の依存先を残す `Reopen` など）は違反を増やさない。`Completed` の依存元による阻止の免除が働くのは、その依存元がすでに未完了の依存先を持つ違反にあるときだけである。修復は違反を減らす操作（解決記録、dependency の削除、依存元から順の `Reopen`、取り外し、再採用）で行う。免除は違反のない保存先では働かない。包含の循環と通常完了経路の循環の違反は循環上の Entity（包含または完了の前提をたどって自身に戻れる Entity）だけに付き、循環を待つだけの Entity は違反に含まれず、免除も受けない。解決記録は違反の検査を免除し、解決で head を一つに戻してから通常操作で直す。違反があり衝突がない保存先には、修復を確実に進める通常操作が一つはある。

Issue は通常操作では子を持たないが、記録の統合の結果として子を持つことがある（片側で子のない Group を Issue に変換し、他側でその Group に子を登録した、または移した場合など）。その子は親が Group でないため親の不在の違反に含まれる。子を持つ Issue の `Complete`・`Cancel` は、Group と同じく直属の子がすべて終了していることを要求する（[親子のlifecycle](lifecycle.md#親子のlifecycle)）。子の終了は `Completed`・`Cancelled` のどちらでも満たす。

## 記録の欠け（gap）

後続の記録が親として参照する記録が保存先にないとき、その Entity は gap を持つ。親が欠けた記録、またはそれに続く記録は、受け手が持つ古い祖先の記録と並んで head になり、実際には祖先どうしなのに衝突として見える。記録は親しか知らないため、Axon はこれを本当の衝突と区別しない。`axon resolve` は、親記録が欠けていて新しい可能性が高い head に `parent missing; likely newer` と示す（判定の規則は `axon resolve --help`）。この印は推定で、欠けた箇所が複数ある場合や解決記録の親が欠けた場合には、古い側や本当の衝突の側に付くこと、どの head にも付かないことがある。新しい方を選んで解決すれば正しい値になり、後で残りの記録が届いても値は変わらない。解決記録だけが先に届いた保存先でも、受け手が解決記録の親のどれかを持っていれば、解決記録が値を持つので現在値は決まり、gap として報告する。持っていなければ古い祖先と並ぶ偽の衝突になる。

gap は解決記録では埋まらず、欠けた記録 file が保存先に戻るか届けば埋まる。そのままでも読取と操作は続けられる。`axon storage check` は gap と、記録のない Entity の Note を情報として報告し、それだけでは非 0 にしない。

## 保存先の破損

`.axon/records/` の下にある記録以外の file は次のように扱う。

- 名前が `.tmp` で終わる file は書込途中の一時 file で、読取と `axon storage check` は無視する。
- それ以外の、名前が記録 ID の形でない file（OS が作る file を含む）、名前と内容の hash が一致しない file、名前の先頭 2 文字と違う subdirectory にある file、途中で切れた file と空の file、規則外の内容の file（現在値に label がない記録、固定集合の外の label を持つ記録を含む）は保存先の破損とする。読取と `axon storage check` は衝突や違反とは別に破損として報告し、利用者が file を直すまで読取と全操作を止める。破損は Entity に属さないので、違反の免除は当てはまらない。
- 名前と内容の hash が一致しない file のうち、内容の CRLF を LF に戻すと名前の hash と一致するものは、Git の改行変換が疑われる破損として、その file の理由にその旨を書き、報告の末尾に改行変換の可能性と [保存先と worktree](../guide/storage.md#改行変換と-axongitattributes) への案内を `Hint:` の一行で添える。読取は内容を戻さず、破損として止める扱いは変えない。

`.axon/` 直下の header・`.gitignore`・`.gitattributes`・lock 以外の file は読まず、報告もしない。header file の欠落、読めない header（空の file、JSON の object でない内容、`format` キーの欠落・文字列でない値、キーの欠落・未知のキー・重複、規則外の値など）、未知の format は、次の段落の新しい format を除いて破損と同じく操作を止める。未知の format は変換しない。

header はまず `format` の値を読み、現在の format と違えば他のキーを検査せずに format で判定する。`axon-records/vN`（N は先頭に 0 を付けない 10 進数）の形で N が現在の format の番号より大きい format は、より新しい axon が書いた保存先として、`.axon/` の中の他の file と directory を見る前に、破損ではなく axon の更新が要ることを示す診断で止める。診断は読取・変更のコマンドと `axon storage check` で同じ文言の `store format "axon-records/v3" in <root>/.axon/header.json is newer than axon <version> reads ("axon-records/v2"); update axon to read this store. The store is not changed.` とする。`axon storage check` はこれを破損の件数に数えず、この診断だけを示して終了 1 にする。それ以外の format（番号の小さいもの、別の名前のもの）は破損と同じ診断で止める（[新しい format の保存先を破損と分ける理由](../design/decisions.md#新しい-format-の保存先を破損と分ける理由)）。

将来の format でも次の三つは変えない。header の置き場所が `.axon/header.json` であること、header が JSON の object で最上位に文字列の `format` キーを持つこと、format の識別子を `axon-records/vN` の形で N を増やして付けること。

現在の header の format は `axon-records/v2` である（[保存形式ごとに検証規則を固定する理由](../design/decisions.md#保存形式ごとに検証規則を固定する理由)）。

## 保存先と初期化

`axon init [PREFIX]` は保存先を新規作成する専用の操作で、既存の保存先を変更しない。`.axon/` が存在しないか、中にあるのが中断した初期化の残骸（lock file、`.tmp` で終わる file、空の記録の directory、`axon init` が書くのと同じ内容の `.axon/.gitignore` と `.axon/.gitattributes`。内容は CRLF を LF と読んで比べる）だけの場合だけ作る。残骸の 2 file に CRLF があれば `axon init` が書く内容で置き換える。header file、記録、内容の異なる `.axon/.gitignore` か `.axon/.gitattributes`、その他の file が一つでもあれば、内容が有効でも拒否してその path を示し、修復・暗黙の変換を行わない。

prefix は Entity ID の先頭に使い、ASCII の小文字英数字とハイフンだけを許す。空文字と、先頭・末尾のハイフンは受け付けない。明示した値は変換せずに検証する。省略した場合は管理 root の directory 名の ASCII 大文字を小文字にした結果を使い、それがこの規則に合わなければ保存先を作らずに失敗する。

`axon init` が作るのは次の四つで、いずれも `.axon/` の中にある。初期化を直列化する lock file を除き、`.axon/` の外には何も書かず、repository root の file と Git config には触れず、stage・commit もしない。`axon init` 以外の操作が `.axon/` の外に書くものは [Axon が `.axon/` の外に書くもの](#axon-が-axon-の外に書くもの) にある。

| path | 内容 |
| --- | --- |
| `.axon/records/` | 記録の directory。記録は記録 ID の先頭 2 文字の subdirectory の下に、記録 ID を file 名として置く |
| `.axon/header.json` | header。format の識別子（`axon-records/v2`）、store ID、prefix を持つ JSON。axon は 1 行で書き、読取は JSON の空白（space・tab・LF・CR。CRLF を含む）と末尾の改行の有無を問わない。BOM などそれ以外の文字は読めない header とする（[header だけ整形を許す理由](../design/decisions.md#header-だけ整形を許す理由)） |
| `.axon/.gitignore` | `*.lock` と `*.tmp` の 2 行。lock と書込途中の一時 file だけを Git から除外し、記録と header は追跡できる |
| `.axon/.gitattributes` | `* -text` の 1 行。`.axon/` の下の file を Git の改行変換の対象から外す。merge・union などの統合の属性は持たない |

store ID と prefix は header file にだけ保持する。header file が保存先の目印であり、探索と `axon init` の拒否はこれで判定する。header があって記録の directory がない保存先は記録のない有効な保存先で、writer が必要なときに directory を作る。

`.axon/.gitattributes` の `* -text` は、repository root の `.gitattributes` の `text=auto eol=crlf` や Git の設定の `core.autocrlf=true` による checkout 時の改行変換を止める。作業 tree の属性 file より優先される設定（repository ごとの `info/attributes`（`git rev-parse --git-path info/attributes` が示す file）で `.axon/` にかかる `text`・`text=auto`・`!text`、`.axon/.gitattributes` のない tree を属性の読み元にする `attr.tree` など）と、改行以外の変換（`filter`・`ident`・`working-tree-encoding` の属性）が `.axon/` にかかる場合は防げず、記録 file の bytes が変わって破損になる。

### 探索

Git 内では現在の repository を探索境界とし、次の順で保存先を選ぶ。

1. 現在の worktree root の `.axon`。
2. main worktree の `.axon`。現在の worktree が linked worktree で、Git common directory が main worktree 直下の `.git` directory である場合だけ探す。bare repository に付けた worktree と submodule では探さない。

各段では、header file があればその保存先に確定する。header がなく、記録やその他の file があれば、破損または未知の format として停止する。中断した初期化の残骸しかない `.axon` では確定せず次へ進む。どの段でも確定しなければ未初期化とする。確定した保存先が破損・読取不能であれば停止し、別の保存先へ fallback しない。

Git 内では、header file のない段ごとに、header の有無による確定と破損・未知の format・未初期化の判定より先に、その段の worktree の Git index で `.axon/` の下に unmerged な path があるかを検査する（`.axon` が存在しない段を含む）。unmerged なら次の段へ進まずに停止し、unmerged な path とその index を持つ worktree の path を示す単独の診断で報告する。header が作業 tree になくても header の欠落を破損として併記しない。header file のある段では保存先に確定し、その保存先の unmerged 検査は読取の前に行う。

通常操作の OS lock、Git index の unmerged 検査、管理 directory が通常の directory であることの検査（symlink の拒否）は、現在の worktree ではなく、確定した保存先と、それを含む worktree に対して行う。

Git 外では最寄りの header file を持つ祖先を管理 root とし、中断した初期化の残骸しかない `.axon` では探索を止めない。header がなく記録やその他の file がある `.axon` は破損または未知の format として停止する。`.axon` のある祖先では、その中を読む前に、祖先の directory と `.axon` の所有者が実行ユーザーであることを検査する。どちらかが他のユーザーのものなら、header の有無や残骸かどうかにかかわらず停止し、別の祖先へ fallback しない。root で実行していて環境変数 `SUDO_UID` があれば、そのユーザーの所有も実行ユーザーの所有とみなす。

### 初期化の対象

`axon init` は探索の 2 を使わず、Git 内では現在の worktree root、Git 外では現在 directory を対象とする。Git 外では既存管理 root 内の入れ子初期化を拒否し、探索と同じ所有者の検査を祖先に行い、現在 directory が他のユーザーのものなら作成せずに拒否する。

探索の 2 が働く構成の linked worktree での `axon init` は、main worktree に保存先が既にあれば拒否し、その path を示す。main worktree の Git index で `.axon/` の下に unmerged な path があれば、unmerged として拒否する。main worktree を判定できなかった場合は、未初期化として扱わずにエラーとする。

`axon init` は OS lock（Git 内では common Git directory、Git 外では `.axon/` に置く lock file）の下で既存の保存先を確認し、記録の directory、`.axon/.gitignore`、`.axon/.gitattributes` を作り、最後に header file を一時 file から rename で公開する。header が公開される前に中断した保存先は未初期化のままであり、再実行で作り直せる。

## 書込

書込は記録 file の作成だけで行う。既存の記録 file を書き直す、置き換える、削除する機構は持たない。writer は保存先の OS lock（`.axon/write.lock`。writer は置換・削除しない）を取り、記録の集合を読み、通常操作の前提を検査し、新しい記録を目的の subdirectory に一時 file `<記録 ID>.tmp` として書いて sync し、記録 ID へ rename して directory を sync する。一つの操作が作る記録は一つで、Note の追加も同じ境界で直列化する。例外は `axon import apply` で、一回の lock の下で検証済みの複数の記録（Entity ごとに一つ）を公開する（[Declaration](declaration.md#preparecheckapply)）。

Git や editor は OS lock に従わないため、同じ worktree で checkout・merge・editor 保存と Axon の書込を並行しない。最終 sync 直後の非協調な書込や、network filesystem での透過的な保証は対象外とする。

### Axon が `.axon/` の外に書くもの

保存先に結び付いて Axon が `.axon/` の外に書くのは次のものだけで、どれも記録ではなく、Git で追跡されない場所に置く。repository root の file と Git config には触れない。利用者が path を指定した file（`axon import prepare` が書き戻す declaration file など）と、デスクトップアプリのデータ（[デスクトップアプリ](gui.md#データ-file)）はこの表に含めない。

| path | 書く操作 | 内容 |
| --- | --- | --- |
| Git 内: common Git directory の `axon-init.lock` | `axon init` | 初期化を直列化する OS lock。file は残る |
| Git 内: 保存先を含む worktree root から求めた common Git directory の `axon-condition-approvals` | `axon condition set --command`・`axon capture --command`・`axon condition allow` | [条件の許可](candidates.md#条件の許可) の file |
| Git 内: 同じ directory の `axon-condition-approvals.lock` | 同上 | 許可の file の書込を直列化する OS lock。file は残る |
| Git 外: `<状態 directory>/axon-condition-approvals/<key>` | 同上 | 管理 root ごとの許可の file |
| Git 外: `<状態 directory>/axon-condition-approvals/<key>.lock` | 同上 | 許可の file の書込を直列化する OS lock。file は残る |

書込の途中では、どの許可の file にも `.tmp` を付けた一時 file が同じ directory にできる。

Git 外の状態 directory は、`dirs::state_dir()`（Rust の `dirs` crate）が絶対 path を返せばそれ、なければ `dirs::data_local_dir()` が絶対 path を返せばそれとし、どちらも使えなければ許可を読めない場合として扱う。OS では分岐しない（Linux では `$XDG_STATE_HOME`、なければ `~/.local/state`。macOS では `~/Library/Application Support`）。その下の `axon-condition-approvals` という directory はこの用途だけに使う。Axon 独自の環境変数で場所を変える手段は持たない。

`<key>` は、header の store ID の UTF-8 bytes、NUL 1 byte、管理 root の symlink を解決した絶対 path の bytes をこの順に連ねた列の BLAKE3 hash を、小文字 16 進 64 文字で表したものである。管理 root の移動・改名でも、同じ path に store ID の異なる保存先を置いても、許可は引き継がれない。同じ store を複製したもの（header を共有するもの）を同じ path に置き直した場合は、許可を引き継ぐ。

Git 内の許可の file を置く common Git directory は、現在 directory からの探索の結果ではなく、確定した保存先を含む worktree root を作業 directory にして `git rev-parse --git-common-dir` を実行した結果（symlink を解決した絶対 path）とする。これを求められなければ、許可を読めない場合として扱う。そのうえで、同じ worktree root で `git rev-parse --git-dir` を実行し、Git directory が worktree root 直下の `.git` に由来することを確かめる。次のどれかに当たる場合は、場所を信頼できないものとして許可の file を読まず、許可を読めない場合として扱い、書込も拒否する。

- worktree root の直下に `.git` がない。
- Git directory（symlink を解決した絶対 path）が、`.git` が directory ならその directory、`.git` が gitfile ならそれが指す directory と一致しない。
- common Git directory が worktree root 自身か、その内側にあり、root 直下の `.git` でない（`--separate-git-dir` で worktree の内側に Git directory を置いた場合などを含む）。

受け取った directory 自体が `.git`（directory か gitfile）を含む場合は、Git directory ごと受け取ったものとして [範囲外](candidates.md#許可の範囲と保存場所) とする。Axon は Git を呼ぶとき `GIT_` で始まる環境変数を除くので、common Git directory は Git の環境変数で変わらない。`$XDG_STATE_HOME` や `HOME` を repository の中へ向けた場合は、Axon はそのまま使い、拒否しない。

許可の file は UTF-8 の text で、1 行目が形式の識別子 `axon-condition-approvals/v1`、続く各行が許可した digest（小文字 16 進 64 文字）で、各行は LF で終わる。writer は digest を昇順に重複なく書く。reader は手で編集した file も読めるよう、識別子の行を含む各行の前後の空白、空行（空白だけの行を含み、識別子より前でもよい）、CRLF の行末、最後の行の LF の欠け、digest の順序と重複を問わない。空の file と空行だけの file は許可の集合を空とし、writer はそれを置き換える。それ以外の内容（空行を除いた最初の行が識別子でない、digest の形でない行）、通常の file でないもの（symlink を含む）を読めない file として扱い、変換・修復しない。file がなければ許可の集合は空とする。利用者は digest の行を消して許可を取り消せ、file を消すとすべての許可を捨てる。読めない file は利用者が直すか消すまで読めないままで、診断はその path を示す。

許可の file の書込は、その file の lock を取り、file を読み直して digest を加え、一時 file に全体を書いて sync してから rename で置き換え、directory を sync する。読めない file があれば書込も拒否し、上書きしない。reader は lock を取らず、rename の前後どちらかの内容を読む。保存先の書込 lock と許可の lock を同時には持たない。

## 検査と解決の入口

`axon storage check [ROOT]` は、引数なしでは探索で確定した保存先を、`ROOT` を与えればその管理rootを探索せずに検査し、破損、衝突、違反、gap を種類ごとに一行ずつ報告する。破損・衝突・違反のいずれかがあれば終了 1、gap だけなら情報として示して終了 0 にする。`ROOT` の有無によらず、Git index で `.axon/` の下の path が unmerged なら、header の有無の判定と破損の検査より先に、その index を持つ worktree の path と unmerged な path だけを示して終了 1 にする。header の format が binary の読む format より新しい保存先も、[更新を求める診断](#保存先の破損) だけを示して非 0 で終了する。通常操作を変えず、条件コマンドを実行しない。追跡する運用の CI はこれを保存先に対して実行する。

`axon resolve` は衝突中の Entity と head を示し、`axon resolve ID --head RECORD_ID` が解決記録を書く。解決の後に残る違反は、`Reopen`・取り外し・dependency の削除・再採用などの通常操作で直す。統合の検査・解決で外部条件コマンドを実行しない。
