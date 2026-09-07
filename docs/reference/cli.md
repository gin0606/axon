# CLI の振る舞いと入出力

## 方針

axon の操作対象は `Issue` と `Group` の 2 kind を持つ Entity である。どちらも同じ公開 ID、文面、Progress、Disposition、Resurface condition、claim を持つ。対象 kind を先に選ばせる namespace は作らず、ID を受け取る top-level command が kind を解決する。

Usage、引数、option、コマンドツリーは Clap の定義から生成する。引数なしの `axon`、`axon help`、`axon -h`、`axon --help` は、次に調べる command をすぐ選べる同一の root help を返す。`axon help <command path>` と各 command の `--help` は Clap による個別の詳細を返す。全 command の help を連結する入口は持たない。`axon docs` は状態モデルと基本 workflow を Markdown ではない端末向け形式で返す。

| 分類 | command 順序 |
| --- | --- |
| Workflow | `plan`, `capture`, `ready`, `triage`, `start`, `done`, `release` |
| Inspect | `status`, `show`, `list`, `claims`, `log`, `note`, `revision`, `actor` |
| Plan management | `write`, `group`, `dep`, `decide`, `when`, `export`, `import` |
| Setup & utilities | `init`, `migrate`, `storage`, `merge`, `completion`, `docs`, `help` |

分類内では、対になる操作と同じ対象を扱う namespace を隣接させる。namespace 内は `group plan|capture|set|unset`、`dep add|rm`、`decide accept|reject|undecide`、`when at|after|manual|command|clear`、`note add|list|show`、`revision list|show|diff`、`import prepare|check|apply` の順とする。

## ハイフンで始まる自由記述

位置引数の title は `--` でオプション解釈を終えてから渡す。他のオプションは
必ずその前へ置く。Issue/Group の plan/capture で同じ規則を使う。

```sh
axon capture -m='説明' -- '--color フラグを扱う'
axon group plan -m='説明' -- '--help を整理する'
```

自由記述のオプション値は `=` で結ぶ。write の `--title`、作成/write/Note の
`--message` (`-m`)、decide/when/release の `--reason` (`-r`) が該当する。

```sh
axon write <id> --title='--color フラグを扱う'
axon write <id> --message='--help から始まる本文'
axon decide accept <id> --reason='--help の仕様を採用する'
axon note add <id> -m='--help'
```

引用符は shell が処理するため、引用だけではオプション解釈を止められない。
`--title -- '本文'` はオプション値を渡す記法ではない。値の欠落位置を検出した
構文エラーでは `=` を案内し、位置引数用の tip や類似オプションの tip を置換する。
既存オプションと同形の本文にも `=` を使う。`--help` を独立した引数として渡す
従来の help 動作は維持する。受け入れ規則、空値の意味、title の正規化は変えない。

## 現在の actor

`axon actor` は引数を取らず、現在の actor ラベルだけを改行付きで stdout に返す。
通常成功時は終了コード 0。DB を開かず、管理 root の有無や DB の状態に依存しない。
初期化、migration、記録追加、claim 取得、外部 Command 評価は行わない。
Note・判断履歴・claim と同じ判定関数を使い、優先順位は
[actor と作業場所](../development/architecture.md#actor-と作業場所)に従う。

Note の追記前には、追記と同じ環境・作業ディレクトリで実行する。過去の Note や
claim の actor は現在値の保証にならず、観測後に環境やディレクトリを変えた場合も
次の操作の actor を保証しない。通常シェルで `USER=gin0606`、ディレクトリ名が
`axon` なら `gin0606@axon` となる。

actor は表示・調査用ラベルであり、一意なセッション ID、認証、排他制御ではない。
複数の Codex 等が同じラベルを共有し得る。結果不明の Note 追記は actor 一致だけで
断定せず、追記前になかった Note ID 集合と凍結した本文などを照合する。
証拠で確定できない結果は unknown として扱う。

## 宣言の説明と新規例

`axon docs declaration` は宣言の必須フィールド、新規と既存の snapshot の違い、
prepare → check → apply → 再check の手順を端末向け英語で返す。
`axon docs declaration --example` は新規 Group 1件、その子 Issue 2件、
子 Issue 間の dependency 1本を含む完全な YAML だけを stdout に返す。
新規の id/base は null とし、既存 DB の ID や fingerprint を要求しない。
説明、Markdown fence、ANSI 装飾を含まず、そのまま保存して prepare へ渡せる。

両コマンドは DB を開かず、管理 root、DB の状態、ネット接続、ソース checkout に依存しない。
取得だけでは登録も着手も行わない。`axon docs` は従来の概念・基本 workflow を維持して
宣言の説明へ案内し、`import --help` と `import prepare --help` も説明と例へ案内する。
`export --help` も内蔵説明へ案内する。内蔵説明には外部 dependency・親 Group の
実在 snapshot を別 export から用意する手順を含め、DB 不要の新規例と区別する。
外部参照のファイル内欠落、現在 DB の不存在、古い/不正確な base、matching-base の
snapshot 不一致、不要 snapshot は原因と確認先を分けて案内する。
形式の正本は [宣言ファイル](declaration-file.md) とする。

## 操作を理解するための案内

利用者の判断を代行せず、axon を正しく操作するための説明を提供する。
採否、着手対象、優先順位、計画の変更内容は選ばない。原因を説明しないことで
利用者やエージェントに操作の意味や復旧方法を推測させない。

- 成功時は、対象と確定した効果を簡潔に示す。次の作業を一律に追加しない。
- 失敗時は、理由と分かっている対象を先に示す。確認用コマンドや、条件付きの
  解消手段とその影響を案内する。複数の判断があり得る場合に一つを既定の指示にしない。
- 空結果は成功として扱い、抽出条件による空とデータ全体の不存在を混同させない。
- help は構文に加えて操作の意味、成立条件、反復時の効果を説明し、共通概念は docs へつなぐ。
- 診断を作るための追加の状態変更や外部条件の実行は行わない。案内は stderr に置き、
  export / completion の生成内容や一覧の record 境界を崩さない。
- 復旧手段が存在しない場合はその制限を明示する。スキーマ番号だけの変更、DB削除、
  無条件の再実行をデータ保持の復旧手順として案内しない。部分成功を全失敗と説明しない。

例えば Group の完了条件を説明しても、子孫の Reject を推奨しない。固定宣言の編集は
Undecided への変更、編集、全文確認、再判断の意味を説明し、再採用を自動選択しない。

## コマンド境界

1 つのコマンドが進行と採否の両方を変えない。他の軸の状態が操作を妨げる場合は、その事実と成立条件を説明し、もう一方の軸を暗黙に変更しない。

### 作成

| kind | Accepted で作成 | Undecided で作成 |
| --- | --- | --- |
| Issue | `axon plan <title>` | `axon capture <title>` |
| Group | `axon group plan <title>` | `axon group capture <title>` |

4 コマンドはいずれも以下の初期入力を受け取る。

- `--parent <group-id>`: 親 Group。
- `--needs <entity-id>`: outgoing dependency。複数指定は `--needs A --needs B` と反復し、同じ参照は一つにまとめる。
- `--manual` / `--at <YYYY-MM-DD>` / `--after <entity-id>` / `--command <shell-string>`: 初期 Resurface condition。一種類だけ指定でき、未指定は Always。日付と条件の意味は `when` と同じ。

参照 ID は完全 ID または一意な suffix を受け取る。description は `-m/--message` または `-F/--file`（`-` は stdin）、title は従来どおり指定する。shell string は一引数として渡し、先頭がハイフンなら `--command='--help text'` のように `=` を使う。

Entity、親、dependencies、初期条件、Accepted の最初の Declaration Revision は同じ transaction で確定し、参照・包含・待機 graph 等の既存制約を保存境界で検査する。失敗時は Entity、関係、Revision を残さない。初期 Command は保存と成功表示のために実行しない（通常の導出照会では評価する）。

Progress は NotStarted、claim なし。plan は Accepted、capture は Undecided。Accepted の最初の Revision は dependencies を含む完成した宣言であり、Control state の条件を含めない。Undecided には Revision を作らない。初期採否と条件は初期値として保存し、架空の判断・条件変更履歴を作らない。

呼び出すたびに新しい Entity を作る追加操作であり、重複排除や idempotency key はない。既存の固定宣言の変更には引き続き明示的な undecide・編集・再判断が必要。

Group は Issue と同じ自動生成 ID で参照し、slug や kind ごとの参照 namespace は持たない。

### 共通の状態・宣言・関係操作

次の command は ID から kind を解決し、Issue / Group の両方へ同じ入口を使う。

- `show` / `write` / `log`
- `note add|list|show` / `revision list|show|diff`
- `start` / `done` / `release`
- `decide accept|reject|undecide`
- `when at|after|manual|command|clear`
- `dep add|rm`

`when after` の参照先と dependency の両端も kind の全組み合わせを許す。`AfterEntity` は参照先が Ended または Rejected なら浮上する。dependency は Rejected を前提喪失として扱う。

title、description、parent Group、outgoing dependency は対象 Entity 自身が所有する plan declaration である。Undecided は draft として実変更でき、Accepted / Rejected は判断対象を固定する。固定済み declaration の変更は `decide undecide`、編集、全文確認、再判断として露出させる。同じ値の再指定は実変更ではないため no-op とする。

Accepted / Rejected への判断時には declaration 全文を 安定IDを持つ Declaration Revision として保存する。直前の Revision と同じなら再利用し、判断履歴から対象 Revision を参照できる。`revision list|show|diff` は Entity と Revision を一つの read transaction から読み、`revision diff` は title、description、parent、outgoing dependency を別々に比較する。optional な description は `present` / `absent` を本文と分けて表示する。

`note add` は本文または file から非空の Note を一件追記する追加操作である。Issue / Group、Progress、Disposition を問わず使え、declaration、状態、関係、導出値を変えない。Note は安定ID、本文、actor、保存時刻を持ち、通常操作では編集・削除しない。`show` は状況と計画の見通しを先に、declaration の固定状態と記録件数を Details に示し、description と全 Note を一つの read transaction から保存順で省略せず表示する。

### 明示解除までの待機

`when manual` は明示解除まで非浮上にする条件を設定し、`when clear` で Always に戻す。
日付・参照先・シェル文字列は受け取らない。任意の reason は判断履歴に保存する。
全 Progress / Disposition への適用、軸の独立性と Group への作用は
[状態モデル](state-model.md#resurface-condition) に従う。

### Commandを実行しない閲覧

`list --skip-command-evaluation` と `show <id> --skip-command-evaluation` は、
対象・祖先・子孫・関係先の Command を一切実行せず保存情報を読む。`list --kind` は併用できる。
Progress、Disposition、宣言、条件文字列、claim、Note、進行履歴、関係を保持して表示する。
外部実行なしで確定する導出値は通常どおり計算し、Command が必要な Surfaced、Active scope、
Ready や root cause は `unevaluated` と明示する。別の確定した要因だけで false と決まる値は `no` とする。
これは閲覧時の観測であり、保存状態や永続キャッシュには追加しない。正常な読取は exit 0、
DB読取失敗などはエラーとする。通常モードの評価・表示と状態変更の成立判定は変えない。
`--trace-conditions` と併用できるが、実行する Command がないため trace block は出ない。
Note・履歴・Revision の専用閲覧経路も維持する。観測の省略は既存形式モデルの状態や成立意味を
変更しないため、非実行・未評価表示・通常評価の維持は Rust のテストで検証する。

### 外部条件の評価

`when command` はシェル文字列を設定し、`when clear` は Always に戻す。条件の設定・訂正・
解除自体は外部コマンドを実行しない。任意の reason は既存の when と同じ判断履歴へ記録する。

外部条件は `/bin/sh -c` で実行する。作業ディレクトリは現在の Git worktree の root、
Git 外では Axon 管理 root とし、起動元の環境変数を継承する。対話・ログイン用の shell 設定は
読み込まない。条件スクリプトは次の終了コード契約を満たす必要がある。

| 終了 | Axon の扱い |
| --- | --- |
| 0 | 条件成立 |
| 1 | 条件未成立 |
| その他、起動失敗、シグナル終了 | 判定失敗として呼び出した Axon コマンドもエラー |

これは既存ツール一般の終了コード規約ではない。必要な終了コード変換は利用者のスクリプトが担う。
判定失敗の診断は対象 Entity、シェル文字列、終了理由、取得できた stdout / stderr を示す。
正常時の外部出力は通常の Axon 出力へ混ぜない。`ready`、`triage`、`status`、`start`、
`list`、`show`、`import check`、`import apply` は `--trace-conditions` を受け付ける。
指定時は、その操作が実際に評価した終了 0 / 1 の Command ごとに Entity ID、cwd、成立可否と
終了コード、取得した stdout / stderr を一つの block として stderr へ評価順に表示する。
空 stream は `(empty)` と表示する。memoized 結果は再表示せず、判定失敗は既存診断だけを出す。

trace は取得した出力を省略・redactionせず、非 UTF-8 byteを lossy UTF-8 として表示するため、
元の byte列を完全には再現しない。秘密情報を除去する保証はなく、利用者が子processの出力を
公開する明示的な診断操作である。trace blockのstderrへの書き込みまたはflushに失敗した場合は、
その評価を失敗としてAxonの呼び出しも失敗させる。状態変更前の評価で失敗するため、変更は適用しない。

評価するのは surfaced などの導出状態が必要になったときだけであり、単なる Entity の DB 読取を
実行トリガーにしない。list / show、ready / triage、start の成立検査や import の導出差分でも、
必要な Entity だけを評価する。`--kind` で除外した候補は評価せず、対象候補の判定に必要な祖先
Group などの評価は行う。履歴・Note・Revision の参照、claims、export は条件を評価しない。

1 回の Axon コマンド内では各 Entity の評価は最大 1 回とし、一覧・依存・祖先 Group と
変更前後の参照で結果を共有する。次の呼び出しでは再評価する。この共有は DB snapshot とは
別の保証で、異なる Entity の外部条件を同一瞬間に観測する保証はない。

状態変更の成立に必要な評価は書き込み確定前に行い、失敗時は変更を適用しない。
確定後の表示のためだけに追加評価し、成功済みの変更を失敗として返すことはない。
評価失敗中でも条件の解除・訂正ができる。DB を変更しない読み取りでも、登録された外部
コマンドの実行は起こりうる。

専用の評価操作は設けない。タイムアウト、永続キャッシュ、実行間隔、ログと終了コードの再生は
外部コマンドの責務であり、外部コマンドが終了しなければ Axon も待ち続ける。
条件の非単調性と Group への作用は [状態モデル](state-model.md#resurface-condition) で定める。

### 包含

包含だけを group namespace に置く。

- `group set <entity-id> <parent-group-id>` は親の新規設定または移動
- `group unset <entity-id>` は親の解除

親は必ず Group とし、各 Entity は最大 1 つの親を持つ。設定操作なので、すでに同じ親である set と、親がない unset は成功 no-op になる。

### 一覧

`ready` / `triage` / `claims` / `list` は両 kind を同じ一覧に出し、`--kind issue|group` で任意に絞る。1 Entity を1行に出し、各行の第1列はID、第2列はkindとする。`triage` は `Reason:`、`claims` は `Claim:`、`Worktree:`、`Started:` をidentityの後に置く。`list` は Progress / Disposition と、該当する例外状態だけを表示する。

`list` は無指定なら全Entityを含む。`--progress not-started|in-progress|ended`、`--disposition undecided|accepted|rejected`、`--terminal=true|false` と `--kind` をANDで組み合わせて絞れる。terminalはProgress=EndedまたはDisposition=Rejectedで、未指定なら両方を含む。各optionは一度だけ指定できる。不正値や反復は入力エラー、矛盾する組合せは成功の空結果とする。

保存状態filterに一致すればManual等の未浮上やinactiveなEntityも含む。`--terminal=false` はready / triage / active scopeを意味しない。filterは保存情報・履歴・claimを変更せず、表示順と1 Entity 1行の契約を維持する。保存情報で除外したEntityの行のためにCommandを評価しないが、残した候補の表示に必要な祖先等は通常どおり評価する。`--skip-command-evaluation` と併用できる。

`list --search <text>` は現在のtitle、description、対象Entityの全Note本文をリテラル部分一致で検索する。大小文字を区別し、Unicode正規化や空白の除去を行わない。空文字は入力エラー。`%`、`_`、正規表現の記号は通常の文字であり、Noteのactor・日時、古いRevision、判断・進捗履歴は検索しない。kind・状態filterとはANDで併用し、未指定なら通常listの全Entity範囲を検索する。複数箇所の一致も1 Entity 1行とし、既存順序を維持する。検索時だけ行末に `Matched: title, description, note-…` を添え、一致したNote IDはID順に列挙する。全文は `show <id>` / `note show <id> <note-id>` で取得する。検索条件もCommand評価前に適用し、空結果は成功する。option形式の検索語は `--search='--help'` と渡す。

`triage` は非 terminal・自身が surfaced・active scope 内・Undecided または orphaned の4条件をすべて満たす Entity を返す。完全定義と自身の未浮上／祖先 gate による inactive の区別は[状態モデル](state-model.md#observed-情報と-triage-frontier)を参照する。非表示は作成・更新の失敗や不存在を意味しないため、作成を再実行する根拠にはしない。管理 root 全体の棚卸しは `list`、個別の保存状態と非表示理由は `show` で確認する。`status` は状況の要約であり全件一覧ではない。

`claims` は claim の経過時間やプロセス状態から staleness を推定しない。表示された保存済み事実を基に人が判断し、必要な claim だけ `release` で明示的に解放する。

### 計画の横断表示

`status` は root Group ごとの計画を ID 順に要約し、その後に所属なし Issue の一覧を ID 順で示す。
root Entity 自身が非 terminal、またはその subtree に保存済み claim があれば表示する。
Rejected root Group は、配下に非 terminal な保存状態だけが残る場合は省き、Group 自身または
配下に保存済み claim がある場合は観測と解消のため表示する。
`status --group <id>` は ID / 一意 suffix を解決し、指定 Group 自身と全子孫を対象にする。
指定時は terminal だけでも表示する。存在しない参照、曖昧参照、Issue 指定はエラーになる。

冒頭は対象の保存済み claim / triage 候補 / ready 候補の件数、続くブロックは Group 自身の
Progress / Disposition、完了可能性、全子孫の軸別内訳、nested Group の所属、claim、候補、
待ちを示す。候補集合は同じ条件下の ready / triage を対象範囲へ絞ったものと一致する。
未終了を実施の約束とはせず、Ended と Rejected を達成率へ合算しない。
Ended または Rejected の Group はすでに terminal なので、完了可否と descendant gate を
表示しない。Rejected Group の保存状態、claim、dependency、Resurface condition は維持する。

待ちは所有する scope ごとにまとめ、未解決 dependency、Rejected prerequisite、未成立の
Resurface condition、Group の descendant gate を区別する。gate の表示は `show` と同じく、
Group 自身の Progress、Disposition、Resurface condition、direct dependency に基づく。
祖先由来の共通理由は所有祖先 scope に一度だけ示し、nested Group 固有の失敗条件は残す。
nested scope の外にある祖先、
dependency / AfterEntity 参照先は External として説明するだけで構成員や集計には加えない。
NotStarted Group の ready は Group 自身の着手候補であり、子孫は gate が開くまで候補にしない。
inactive な scope 内も保存された claim の actor / worktree / 開始時刻を表示し、実際の
プロセス稼働、健全性、停止、staleness を推測しない。詳細な全 subtree、本文、Note、履歴は
`show <id>` に委ねる。空結果は他の一覧と同じ stdout / stderr 契約に従う。

ID 解決と Entity / 関係を一回の整合した read snapshot から取得する。候補と説明で同じ
条件評価 context を共有し、必要な Command の判定失敗はエラーとして返す。対象外の独立した
計画は評価しない。NO_COLOR / 非対話出力でも同じ意味と順序、保存文字列を保つ。

### Plan 宣言ファイル

複数 Entity と関係を一枚で編集するときは、`axon export` と `axon import prepare|check|apply` を使う。形式の正は [宣言ファイル](declaration-file.md) とする。

- `export <id>...` は明示 Entity、`export --group <id>` は Group と直下、`--recursive` 付きは全子孫を編集対象にする。selector の和集合だけを選び、関係から編集対象を広げない
- `import prepare <file>` は新規 Entity の最終 ID を割り当て、同じ file を canonical YAML へ atomic replace する。DB は変えない
- `import check <file>` は競合と制約を検査し、所有値の構造差分と導出値の差分を表示する。file と DB は変えない
- `import apply <file>` は write lock 内で同じ検査をやり直し、一つの transaction で全変更を反映してから file の snapshot を更新する

apply は暗黙の既定動作にせず、明示 subcommand だけで実行する。既存の固定済み Entity に一つでも declaration の実変更があれば、file 全体を変更せず拒否する。DB commit 後の file 更新だけが失敗したときは、DB が宣言の最終値に完全一致する場合に限り同じ file の再 apply を DB no-op として受け付ける。

## Group の進行

Group は計画範囲を明示的に進める Entity であり、子孫から自動完了しない。

1. ready な Group を `start` すると activation gate が開く。
2. 配下の active frontier が `ready` / `triage` に現れる。
3. 全子孫が terminal になった後、Group 自身を `done` する。

Group の `release` は InProgress の子孫が 0 件のときだけ成功する。Group の claim は子孫を lock せず、Group と子孫を別 actor が同時に claim できる。

`show` は Issue / Group とも Situation を先に表示する。Group は全子孫の Ended、Rejected、未終了かつ非却下の Unfinished を示す。Ended と Rejected の重なりを明記し、terminal 件数を達成率や将来実施の約束に読み替えない。非 terminal Group は Can complete と未充足条件を示し、Ended または Rejected の Group には完了可否を重ねない。Rejected Group はすでに terminal であり、非 terminal な子孫は inactive な保存状態として残るだけでは将来作業や後片付けを要求しないと示す。subtree に保存済み claim があれば件数を示し、外部作業を終了、release、Group 外で継続するかは claimed Entity ごとに判断するよう案内するが、自動変更はしない。続く `Subtree` は terminal を含む全子孫を包含階層どおりに並べ、各 Entity の ID、kind、Progress / Disposition、title と現在の候補を示す。祖先の activation gate は所有 scope にまとめ、子孫の固有の未解決 dependency / Rejected prerequisite / Resurface condition を項目の近くに示す。AfterEntity は Ended または Rejected で成立し、dependency と区別する。選択範囲外の祖先gateもIDとともに示す。Details には保存状態、導出値、claim、declaration と記録件数、直下・全子孫の詳細集計を配置し、その後に関係・長文・履歴を続ける。子孫の description、Note、Revision、履歴、claim 詳細は展開せず、必要な Entity を個別に `show` する。

`Dependencies` は選択した Group と全子孫が所有する direct outgoing dependency を owner ごとに表示する。Group 由来の dependency を子孫へ重複表示せず、target は `Satisfied` / `Unresolved` / `Rejected` を区別する。subtree 外の target は `External` と title を示すが、その先の subtree は展開しない。subtree の sibling、dependency owner、target は ID 順とし、状態変化で表示順を変えない。空 Group も `Subtree` に明示し、件数による省略は行わない。

## 反復実行と履歴

| 分類 | command | 同じ入力の反復 |
| --- | --- | --- |
| 状態遷移 | `start` / `done` / `release` / `decide` / `when` | 失敗 |
| 設定 | `write` / `group set|unset` / `dep add|rm` | 成功 no-op |
| 追加 | `plan` / `capture` / `group plan` / `group capture` | 新規 Entity を追加 |
| 追記 | `note add` | 新規 Note を追加 |

失敗した遷移と成功 no-op は状態、履歴、`updated_at` を変えない。失敗した遷移は非 0 で終了する。エラーは原因となる事実を先に示し、必要な確認先、操作の成立条件、解消手段とその影響を補足できる。入力された reason などの自由記述を診断へ無関係に転載しない。外部条件の判定失敗では、原因を確認できるよう上記の実行コマンドと外部診断出力を含める。`start` と `done` は reason を持たず、`release` の任意 reason は進行履歴、`decide` / `when` の任意 reason は判断履歴に保存する。reason の有無は操作の成否を変えない。作業結果や通常の申し送りは、declaration の description ではなく Note に残す。追加操作に idempotency key は持たない。

## 原子性と deadlock 防止

`start` は ready の検査と claim の取得、Group の `done` / `release` は子孫条件の検査と進行更新を、それぞれ同じ write transaction で行う。

dependency、`AfterEntity`、包含を activation wait graph と completion wait graph に射影する。relation の追加・置換は両 graph の非循環と包含の不変条件を同じ transaction 内で検査してから保存する。Group を待機元にした dependency / `AfterEntity` は Group 自身と全子孫へ展開する。既存データの循環を解消できるよう、辺を除く `dep rm` / `when clear` / `when at` / `when manual` / `when command` / `group unset` は他の循環が残っていても実行できる。

Ended Group は完了宣言を後から無効にしないため、親変更、subtree の出入り、依存元としての dependency 変更を拒否する。Ended Group 配下の terminal Entity を非 terminal に戻す Disposition 変更も拒否する。

## 出力と管理 root

help、一覧、詳細、成功確認は stdout、エラーは stderr に出す。一覧が空なら stdout を空に保ち、案内だけを stderr に出して成功する。

人向け出力は、Entity一覧、履歴と索引、一件の詳細、状態変更の確認という役割ごとに共通の文字構造と語彙を使う。一覧と履歴は1 recordを1行に置き、一件の詳細では短い metadata を長文やdiffより先に置く。状態変更の確認は対象IDから始め、保存されたsnapshot全体を繰り返さない。Note / Revision一覧は安定IDから始める。照会・diffはIDまたは4文字以上の一意な接頭辞を受け付け、旧番号を参照として受け付けない。判断・進行履歴にもIDを表示する。

### 更新結果の共通契約

更新系の確認・診断は、対象、結果と変更内容または原因、適用範囲、必要な補足案内の順に読む。
以下は個別コマンドの表示を実装する際の正本であり、代表例は期待する表示を示す。
状態遷移、設定、追加の意味、保存境界、原子性を変更するものではない。

| 要素 | 語彙と配置 |
| --- | --- |
| 対象 | Entity の成功確認は完全 ID を先頭に置く。関係の相手 ID は変更内容に置く。診断は既存の `Error:` に続けて、判明している対象 ID または path と操作を示す。ID 解決前は入力された識別子、保存先の問題は path を使い、未確定 ID を作らない。 |
| 実変更・追加 | `Title updated`、`Description updated` / `Description removed`、`Dependency added:` / `Dependency removed:`、`Parent:` と最終値を使う。追加は `Created` または `Note <stable-id> recorded`、遷移は `Started` / `Ended` / `Released` など既存の状態確認語を使う。これらは保存処理の成功後だけ表示する。 |
| 成功 no-op | `No changes` を結果とし、関係操作では `Dependency already present:` / `Dependency already absent:` と相手 ID、または `Parent:` と最終値（親なしは `(none)`）を添える。状態遷移の同値拒否には使わない。 |
| 拒否・失敗 | `Error:` の本文は原因となる事実を示す。`cannot depend on itself` のような制約違反と、I/O・外部評価失敗や保存データ異常を分ける。結果だけの `Failed` や、入力誤りを `invalid schema` とする表示では代替しない。 |
| 適用範囲 | 誤解の余地がある場合に `Applied:` / `Not applied:` / `Result unknown:` を使い、項目、Entity、DB、file のどの範囲かを示す。既存診断が同じ事実を本文で明示している場合は重ねない。部分適用は適用済みと未適用または不明を併記する。 |
| 補足案内 | 確認・復旧方法が必要な場合だけ末尾の `Help:` に置く。原因や適用済み範囲を Help だけへ隠さない。単純な原因で次の行動が分かる場合は省く。 |

単一対象で短い結果は `ID  結果  補足` の一行にする（項目間は空白2つ）。
複数項目も短ければ同じ行に置ける。複数対象、長い原因、段階結果は複数行とし、
先頭に対象と結果・原因、続いて必要なラベル付きの行を置く。各行の対象が曖昧になる場合は
ID または path を添える。同じ情報を埋めるためだけに空欄や全件の snapshot を追加しない。

成功確認は stdout / 終了0、アプリケーションの拒否・実行失敗は stderr / 終了1、
Clap の構文・引数検証エラーは既存の stderr / 終了2を維持する。
Clap の `error:`、Usage、help tip の既定構造はアプリケーション診断へ作り直さない。
一つの呼び出しが途中まで適用された場合も全体は失敗であり、段階結果は stderr の診断で説明する。
既に出力した成功情報は取り消せないため、終了コードと段階結果を合わせて読む。
外部 Command の終了値を Axon 自身の終了値と混同しない。既存の BrokenPipe 成功扱いも維持する。

保存処理が返した変更有無を表示へ使い、事前読取だけで実変更を推測しない。
`write` は実際に保存した項目だけを列挙し、一部項目のみ変更なら全体を `No changes` としない。
指定された title と description はそれぞれ保存処理へ渡し、同値の場合も保存時の判定を使う。
`dep add/rm` と `group set/unset` も同じ保存結果で実変更と `No changes` を切り替える。
成功 no-op は終了0で、保存状態・履歴・`updated_at` を変更しない。
複数段階の操作で後段が失敗しても、前段の成功を未適用と断定しない。
結果不明は成功でも未適用でもなく、照合が必要な状態として示す。
診断を作るためだけの追加 mutation、外部 Command 評価は行わない。
非 idempotent な作成・Note 追記の無条件再実行を案内せず、対象の保存情報と記録 ID を照合させる。

#### 代表出力

以下の ID と path は説明用。短い成功は stdout / 終了0で、例えば次のように読む。

```text
axon-a1b2c3  Title updated  Description removed
axon-a1b2c3  Dependency added: axon-d4e5f6
axon-a1b2c3  Parent: axon-g7h8i9
axon-a1b2c3  No changes
axon-a1b2c3  No changes  Dependency already present: axon-d4e5f6
axon-a1b2c3  No changes  Dependency already absent: axon-d4e5f6
axon-a1b2c3  No changes  Parent: (none)
axon-j1k2l3  Created  Issue  [Accepted]  新しい計画
axon-a1b2c3  Note note-0123456789abcdef0123456789abcdef recorded
```

新規追加の再実行は別 ID の追加である。`Created` は既存 Entity の再利用を表さない。
単純な入力制約拒否と同値遷移拒否の例（stderr / 終了1、保存変更なし）:

```text
Error: axon-a1b2c3 dep add: Entity axon-a1b2c3 cannot depend on itself
Error: axon-a1b2c3 decide accept: axon-a1b2c3: Disposition is already Accepted
Help: No transition was applied. Use `axon show axon-a1b2c3` to inspect the current state; repeating the same transition is an error.
```

適用前の I/O 失敗（置換前と確認できる場合、stderr / 終了1）:

```text
Error: /work/.axon/state.jsonl write: permission denied while creating temporary file
Not applied: state file replacement
Help: Check directory permissions before retrying.
```

`write --title ... -m ...` の title 保存後に description 保存が失敗した場合
（段階ごとの保存境界は維持、stderr / 終了1）:

```text
Error: axon-a1b2c3 write: description save failed: permission denied
Applied: axon-a1b2c3 Title updated
Not applied: axon-a1b2c3 Description update
Help: Inspect the saved Entity before deciding which fields to retry.
```

`import apply` は既存の `Plan is valid.`、`Changes:`、`Derived changes (...)`、
`Applied  <path>` の順と diff 構造を維持する。`Changes:` は DB の差分で、
`none` は DB の成功 no-op に対応する。`check` の同じ表示は予測であり保存成功を表さない。
`Applied  <path>` は DB の適用と宣言 file の更新の両方が完了した確認であり、
DB の実変更があるという意味ではない。複数 Entity の差分は ID ごとに示す。
例として、導出値に差分のない複数 Entity の title 更新は stdout / 終了0で次の形になる。

```text
Plan is valid.
Changes:
  axon-a1b2c3: title updated
  axon-d4e5f6: title updated
Derived changes (ready, blocked, orphaned, active_scope, group_completable):
  none
Applied  /work/plan.yml
```

DB 全体の適用後、宣言 file の置換前に失敗した場合（stderr / 終了1）:

```text
Error: /work/plan.yml import apply: declaration file refresh failed: permission denied
Applied: storage declaration values
Not applied: declaration file refresh at /work/plan.yml
Help: Inspect saved information with `axon list --skip-command-evaluation` and preserve the file; retry the same file only after verifying the declared final values match storage.
```

file backend の置換後の同期失敗など、保存結果を確定できない場合（stderr / 終了1）:

```text
Error: /work/.axon/state.jsonl note add: directory sync failed after replace
Result unknown: Note append in /work/.axon/state.jsonl
Help: Confirm the writer has stopped, then inspect saved Note IDs and bodies before retrying.
```

DB が no-op でも file 更新は別の段階であり、その失敗は成功に変わらない。
init、migration、merge の複数保存先も同じ規則で、既知の保存先、phase、backup と
適用状態を示す。個別の保存保証・復旧条件は[保存契約](file-storage.md)と
[手動移行](migration.md)に従い、この表示規則から全体の原子性を推測しない。

#### 診断の保存境界

通常の診断は操作と判明した対象を含み、自由記述の reason や title を診断の context に転載しない。
自己依存は通常操作・宣言とも `Entity <id> cannot depend on itself` として拒否する。
保存済み snapshot の不整合は引き続き保存データ異常として扱う。
SQLite は transaction の開始・保存・commit、file は置換前・置換後の同期を区別して保存 path を示す。
SQLite の commit 自体のエラーは保守的に `Result unknown` とし、保存前の拒否と混同しない。
`Applied: storage declaration values` は import の保存値が確定したことを表し、DB no-op も含む。
確認出力に失敗しても保存済みの結果は `Applied:` に残す。BrokenPipe の成功扱いは維持する。
init の Git integration 完了前に state や一部の補助 file を保存した場合も、
適用済みの段階を診断に残す。

時点・確認経路・維持/修正の処分は
[2026-09-06 の更新系診断棚卸し](../development/audits/mutation-diagnostics-2026-09-06.md)を参照する。
成功側との対応、端末条件、残る検証範囲は
[同日の横断確認](../development/audits/mutation-output-crosscheck-2026-09-06.md)を参照する。

#### 実装への対応

契約策定時点（2026-09-06、`e21b20f` のソース確認）の引継ぎは次のとおり。
これは実装済みの保証ではなく、後続 Issue が照合する対象の対応表である。

| 対象 | 契約への対応・後続担当 |
| --- | --- |
| `write` | 保存結果から変更項目と no-op を確定する。事前読取のみの表示判定を axon-1x115z で修正。複数保存段階の失敗範囲は axon-1ag24h で棚卸しする。 |
| `dep add/rm`、`group set/unset` | 相手 ID / Parent を保ち、保存時の no-op に `No changes` を付ける。axon-1x115z が担当。 |
| `plan/capture`、Group 作成、`note add` | 新規追加と ID の意味を維持。入力・保存失敗は axon-1ag24h の棚卸し対象。 |
| `start/done/release`、`decide`、`when` | 同値は拒否のまま。固定宣言、成立条件、循環・包含、外部評価失敗とともに axon-1ag24h で診断を確認する。 |
| 自己依存 | `invalid axon schema` という誤分類を axon-1ag24h で修正し、宣言経由と原因の語彙を揃える。 |
| `import prepare/apply` | DB 差分と file 更新を区別。既存成功 report は axon-1x115z で整合確認し、拒否・段階失敗は axon-1ag24h で棚卸しする。外部参照例・固有診断は axon-50qc1t が担当。 |
| `init`、migration、merge と共通入力・保存境界 | 既存の適切な診断は維持し、誤分類、適用結果、復旧案内を axon-1ag24h で棚卸しする。parser の自由記述 tip は axon-nnb208 の担当を維持。 |

axon-2b4xyp で両実装の成功、no-op、拒否、段階結果と保存状態を横断確認する。

### 装飾

`show` を含む人向け出力は、stdout が対話 terminal なら状態の識別を補助する ANSI style を使う。非対話出力と `NO_COLOR` では同じ文字、空白、改行、順序を無装飾で出し、状態の違いを色だけでは表さない。title、description、Note本文、reasonなど利用者が保存した文字列は装飾・省略・整形しない。下流でpipeが閉じた場合は成功として扱う。

装飾は保存状態の値ごとではなく、その情報が利用者の現在の操作に持つ意味で決める。
同じ意味は一覧、詳細、履歴、状態変更確認、help、診断で同じ style を使う。

| 意味 | style | 主な対象 |
| --- | --- | --- |
| 文書構造 | bold | section heading、record 内の index |
| identity | cyan + bold | Entity ID |
| 現在進行中 | cyan + bold | InProgress、Started |
| 成立・許可・成功 | green | Accepted、Ready、valid、成功を表す確認語 |
| 通常の待機 | yellow | Blocked、未解決 dependency、未完了 Group |
| 利用者の注意・介入が必要 | yellow + bold | Undecided、warning、明示的な判断を待つ印 |
| 前提喪失・失敗 | red + bold | Orphaned、Rejected prerequisite、error |
| terminal・非 active・補助情報 | dim | Ended、Rejected、not surfaced、inactive scope、kind、label、timestamp、満足済み関係、no-op |
| 中立 | plain | NotStarted、通常値、保存された自由記述 |

色と太字は組み合わせて意味を狭める。yellowだけは他Entityや条件の状態変化を待てば進める状態、
yellow + boldは利用者が確認・判断しなければ進まない状態を表す。redは通常の待機には使わず、現在の計画のままでは
前提が成立しない状態または失敗に限る。Rejected自体は選択済みのterminal状態なのでdimとし、
別Entityの前提を失わせている文脈だけredにする。diffの `+` / `-` はgreen / redという
端末上の慣例を使うが、記号を必ず残し、状態の成功・失敗とは解釈しない。

見出しとEntity-local indexはboldで構造を示し、identityを表すcyanとは分ける。背景色、固定RGB、
blink、invert、hidden、strikethroughは端末theme、対応差、可読性への依存が大きいため使わない。
underlineはClapの既定styleへ依存させず、Axonの見出しはboldへ統一する。利用者入力のほか、
actor、worktree、外部command、pathも値全体を意味色で塗らない。

非TTY の record 境界、先頭の識別子、標準 stream は安定した外部契約とする。

`export` と completion は生成内容そのものを標準出力へ書き、人向けの装飾を加えない。

Git 配下の file 正本は現在の worktree root の `.axon/state.jsonl`、
SQLite は common Git directory の親の `.axon/axon.db` を共有する。
Git 外は最寄りの正本または pending marker がある管理 root の同じ二つの名前を使う。
設定ファイルはなく、両方存在すれば混在エラー、どちらもなければ未初期化。
破損・途中生成で fallback しない。一 repository 一 backend を想定し、全 worktree は走査しない。
Issue と Group は同じ `<prefix>-<ランダム 6 文字>` namespace を使い、完全 ID または一意な suffix で解決する。

## DBの互換性検査

通常操作は対応するschema更新をbackup付きで自動実行し、成功後に続行する。現行の基点はv13で、退役した旧版・未来版・未知構造は変更せず拒否する。
`init` は新規作成専用で、既存正本や初期化途中への再実行を拒否する。既定は SQLite で ignore は変更しない。`--backend file` は Git 内外とも `.axon/.gitignore` と root `.gitattributes` を生成・補完する。Git driver の登録は利用者が通常の `git config` で行う。保存成功境界とinit復旧は[backendとfile保存](file-storage.md)を参照。help、docs、version、completionはDB不要。
backend変換は明示した現行schemaのSQLite入力から別directoryへ出力し、元DBの切替はしない。
診断はpath、版、処理段階、原因、backup先と出力の適用状態を示す。失敗時の途中成果を上書きせず、
結果不明なら出力とbackupを調べてから再開する。具体的な手順は[手動移行](migration.md)。

### 計画表示の情報密度

status は各項目の identity を一度だけ表示し、Ready / Triage、保存 claim と待ち理由をその項目に添える。所属なし Issue は Ungrouped Issues にまとめ、空セクションは省く。冒頭の件数はゼロでも表示する（対象全体が空の場合は既存の空表示案内）。Ended Group の完了不可と終了由来 gate は要約から外し、show の Details で確認できる。Rejected root Group は保存済み claim がある場合だけ通常表示に残し、その場合は配下の未終了項目・保存 claim・inactive 理由も表示する。

## 手動移行

`axon migrate --source <current-schema-db> --output <未使用directory> --backend <sqlite|file>` は、通常のroot探索を行わず指定DBを読み取り専用で開き、新しい保存先へ変換する。元DBの切替は行わない。詳細は[手動移行](migration.md)。

showは分岐・統合を含む履歴について因果参照と採用先端を表示する。並行記録のID順は時刻の前後を意味しない。

## File storage と merge

`storage check`、`merge prepare/check/apply/driver/setup` の保存・競合・Git 契約は
[file storage](file-storage.md#cli-workspace-と-git) を参照する。
これらの明示 snapshot 操作は通常 Store を開かず、stage/commit を行わない。

`--version` / `-V` は package version と、ビルド時の完全 commit hash・source 状態を表示する。
取得不能な情報は `unknown` とし、実行場所から推定しない。metadata 指定と検知範囲は
[ビルド時のソース由来](../development/architecture.md#ビルド時のソース由来)を参照。
