# Axon モデル contract

操作で Entity data の解釈または変更が必要なとき、この reference を読む。

## Entity と状態軸

Axon には Issue と Group の 2 種類の Entity kind がある。どちらも stable ID と、同じ 3 つの独立した状態軸を持つ。

- Progress: `NotStarted`、`InProgress`、`Ended`
- Disposition: `Undecided`、`Accepted`、`Rejected`
- Resurface condition: `Always`、`AtDate`、`AfterEntity`、`Manual`、`Command`

1 つの軸を別の軸の代用にしない。Progress を変える command は Disposition や Resurface condition を暗黙に変えてはならず、逆も同様である。

Entity は Progress が `Ended` または Disposition が `Rejected` のとき terminal である。`ready`、`blocked`、`orphaned`、`surfaced`、`terminal`、active scope、blocking cause、Group completion fact は保存済み state と関係から導出される。独立して編集可能な data として扱わない。

`ready` は Entity が active scope 内、`NotStarted`、`Accepted`、surfaced、not blocked、not orphaned であることを意味する。`triage` が Entity を含むのは、non-terminal、自身の Resurface condition を満たす（surfaced）、active scope 内（すべての ancestor Group の activation gate が開いている）、`Undecided` または orphaned、という 4 条件をすべて満たす場合だけである。これらの定義は判断や作業を人または agent に割り当てない。

`Manual` は明示的に置き換えるか `axon when clear` で clear するまで unsurfaced のままである。payload を持たず、Progress、Disposition、claim のいずれも変更しない。Manual Group は descendant state を変えずに descendant の active scope を閉じる。root Entity は常に active scope 内だが、自身の condition が Manual の間は `triage` に現れない。surfaced child も ancestor gate が閉じている間は現れない。Undecided または orphaned Entity は 4 条件すべてを満たすと `triage` に入る。

`Command` は観測結果ではなく shell 文字列を保存する。導出 status が必要な query はこれを実行できる。exit 0 は condition を満たし、exit 1 は満たさず、その他の exit、signal、spawn failure は Axon command を失敗させる。結果は 1 invocation 内で共有され、次回は再評価される。Progress、Disposition、claim を変えなくても、condition の充足結果は後の評価で覆りうる。condition の clear または訂正に評価成功は不要である。実行 contract は `axon when command --help` を参照する。

## 情報の所有範囲

Entity の plan declaration は次の要素だけで構成される。

- title
- description
- parent Group
- outgoing dependency

Progress、Disposition、Resurface condition、claim は Control state である。decision reason と progress reason はそれぞれの typed history に属する。`ready` などの導出 fact は観測値であり、declaration や state field ではない。

`Undecided` Entity は編集可能な draft declaration を持つ。`Accepted` または `Rejected` Entity は固定された declaration を持つ。固定 declaration を変更するには `Undecided` に戻し、完全な draft を編集・検証してから、意図する最終 Disposition を別途適用する。それらの transition に与えられた reason は typed history に保存する。

初期作成では dependency と condition を atomic に与えられる。claim のない NotStarted を保ち、架空の transition なしに初期値を記録する。Accepted での作成は最初の Revision に完全な declaration を記録し、Undecided での作成には Revision がない。初期 Command 文字列は保存時にも確認時にも実行されない。

accepted または rejected の各 declaration は immutable な Declaration Revision として保存される。Note は declaration や Control state を変更しない append-only の補足情報である。調査結果、実装結果、後から判明した制約、handoff の詳細は Note に属する。description を activity log にしたり、Note で状態変更を模倣したり、古い Note を編集したり、状態変更 reason を Note に重複させたりしない。

## Entity context

既存 Entity を変更する前に `axon show <id> --skip-command-evaluation` を読み、要求された操作に関係する保存済み field を調査する。依頼が Declaration Revision、Note、typed history の保持情報を変更する、またはそれに依存する場合は、それらを読む。操作に必要な導出 condition は別途評価する。frontier listing から不足 context を推測しない。

関連 Entity の state または declaration が操作の validity や呼び出し側が理解すべき結果を変えうる場合、それらを調査する。inactive Entity を含む完全な saved-state inventory が必要なら `axon list --skip-command-evaluation` を使い、導出された観測が重要な場合だけ condition を別途評価する。

`ready` と `triage` は frontier であり完全な inventory ではない。`triage` にないことは Entity の不在や作成・更新失敗を意味しない。その証拠だけで作成を繰り返さない。

saved state には `axon show <id> --skip-command-evaluation` を使い、readiness、surfacing、active-scope の評価が必要なら通常の derived query を続けて使う。完全な inventory には `axon list --skip-command-evaluation` を使い、`ready`、`triage`、`claims` の非表示を Entity の不在の証拠にしない。

## 関係と Group

Dependency は prerequisite である。target が `Ended` かつ `Rejected` でないとき dependency は満たされ、rejected target は dependent を orphaned にする。一方、`AfterEntity` は schedule condition であり、ended または rejected の target は waiter を surfaced にする。

Group は tag ではなく明示的な plan Entity である。開始すると activation gate が開くが descendant は開始しない。Group を done にできるのは `InProgress` で、すべての descendant が terminal になった後だけである。Group の release には `InProgress` descendant が 0 件である必要がある。Group を reject すると Group は terminal になり active scope が閉じるが、descendant state は変更しない。配下の non-terminal descendant は安定した inactive saved state として残りうる。可視であることだけを理由に reject、release、その他 cleanup を要求しない。保存済み claim またはその外部作業に実際に disposition が必要なときは、descendant を自動変更せず、観測した claim と Entity ごとの選択肢を報告する。

別 capability の副作用として descendant を自動開始したり、ancestor Group を完了したり、child を移動したり、dependency を書き換えたりしない。それらの可能な次操作を呼び出し側 workflow に返す。

## 外部 condition を実行せず調査する

保存済み情報だけが必要な場合、または Command condition が失敗・未完了の場合は、
`axon list --skip-command-evaluation` または `axon show <id> --skip-command-evaluation` を使う。
これらは ancestor、descendant、関連 condition を含め、Command を一切実行しない。
`unevaluated` は read 時の観測であり、false や保存済み state ではない。他の condition は
引き続き評価できる。この option は `--trace-conditions` と併用できるが Command trace を
出力せず、lifecycle mutation に対する readiness を立証しない。通常の read と lifecycle check は
引き続き condition を評価する。

呼び出し側が評価証拠を必要とする場合だけ `--trace-conditions` を使う。実行した各 shell 文字列と取得した stdout/stderr を redaction や truncation なしで出力するため、sensitive な出力を不必要に公開・保存しない。abnormal exit は trace block を出さずに Axon invocation を失敗させる場合がある。

### 保存済み text の literal search

`axon list --search <text>` は現在の title と description、およびすべての Note 本文を検索する。case-sensitive な literal matching であり、regex、`%`、`_` に特別な意味はなく、actor、history、Revision は対象外である。Command evaluation の前に kind filter と saved-state filter と組み合わせられる。純粋な saved-text search には `--skip-command-evaluation` を加える。一致箇所と stable Note ID により、完全な内容を調査する場所を特定できる。search で候補を絞れるが、1 つの literal query に存在しないことは意味的な一意性を証明しない。

### inventory の state filter

filter なしの `axon list` はすべての Entity を含む。`--progress not-started|in-progress|ended`、`--disposition undecided|accepted|rejected`、`--terminal=true|false`、`--kind issue|group` は AND で組み合わせ、各 option は 1 回指定できる。Terminal は Ended または Rejected（両方の場合を含む）を意味する。両方を含めるには `--terminal` を省略する。一致結果には inactive と unsurfaced の Entity も残る。`--terminal=false` は ready/triage frontier や active scope ではない。一致 0 件でも成功する。filter は saved state、history、claim、row format、ordering を保持する。

たとえば、未開始の accepted plan には `axon list --progress not-started --disposition accepted`、rejected Entity には `axon list --disposition rejected`、ended work には `axon list --progress ended` を使う。Saved-state filter は row/Command evaluation の前に実行され、保持された row に必要な ancestor は引き続き評価されうる。すべての Command 実行を防ぐには `--skip-command-evaluation` を加える。
