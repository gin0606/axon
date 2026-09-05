# CLI の振る舞いと入出力

## 方針

axon の操作対象は `Issue` と `Group` の 2 kind を持つ Entity である。どちらも同じ公開 ID、文面、Progress、Disposition、Resurface condition、claim を持つ。対象 kind を先に選ばせる namespace は作らず、ID を受け取る top-level command が kind を解決する。

英語の操作手順は [利用ガイド](../guide/usage.md)、状態の意味は [状態モデル](state-model.md)、設計理由は [設計判断](../design/decisions.md) に置く。この文書はコマンドの境界、反復、原子性、入出力の契約を定める。Usage、引数、option、コマンドツリーは Clap の定義から生成する。引数なしの `axon`、`axon help`、`axon -h`、`axon --help` は、次に調べる command をすぐ選べる同一の root help を返す。`axon help <command path>` と各 command の `--help` は Clap による個別の詳細を返す。全 command の help を連結する入口は持たない。`axon docs` は状態モデルと基本 workflow を Markdown ではない端末向け形式で返す。

| 分類 | command 順序 |
| --- | --- |
| Workflow | `plan`, `capture`, `ready`, `triage`, `start`, `done`, `release` |
| Inspect | `show`, `list`, `claims`, `log`, `note`, `revision` |
| Plan management | `write`, `group`, `dep`, `decide`, `when`, `export`, `import` |
| Setup & utilities | `init`, `completion`, `docs`, `help` |

分類内では、対になる操作と同じ対象を扱う namespace を隣接させる。namespace 内は `group plan|capture|set|unset`、`dep add|rm`、`decide accept|reject|undecide`、`when at|after|manual|command|clear`、`note add|list|show`、`revision list|show|diff`、`import prepare|check|apply` の順とする。

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

4 コマンドはいずれも `--parent <group-id>` を受け取り、作成と包含設定を同じ transaction で行う。呼び出すたびに新しい Entity を作る追加操作である。

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

Accepted / Rejected への判断時には declaration 全文を Entity 内連番の Declaration Revision として保存する。直前の Revision と同じなら再利用し、判断履歴から対象 Revision を参照できる。`revision list|show|diff` は Entity と Revision を一つの read transaction から読み、`revision diff` は title、description、parent、outgoing dependency を別々に比較する。optional な description は `present` / `absent` を本文と分けて表示する。

`note add` は本文または file から非空の Note を一件追記する追加操作である。Issue / Group、Progress、Disposition を問わず使え、declaration、状態、関係、導出値を変えない。Note は Entity 内連番、本文、actor、保存時刻を持ち、通常操作では編集・削除しない。`show` は declaration の固定状態と記録件数を冒頭に示し、description と全 Note を一つの read transaction から保存順で省略せず表示する。

### 明示解除までの待機

`when manual` は明示解除まで非浮上にする条件を設定し、`when clear` で Always に戻す。
日付・参照先・シェル文字列は受け取らない。任意の reason は判断履歴に保存する。
全 Progress / Disposition への適用、軸の独立性と Group への作用は
[状態モデル](state-model.md#resurface-condition) に従う。

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
正常時の外部出力は通常の Axon 出力へ混ぜない。

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

`claims` は claim の経過時間やプロセス状態から staleness を推定しない。表示された保存済み事実を基に人が判断し、必要な claim だけ `release` で明示的に解放する。

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

`show` は Group 自身の保存済み状態と、直下・全子孫の kind / Progress / terminal 集計、現在 done できるかを分けて表示する。続く `Subtree` は terminal を含む全子孫を包含階層どおりに並べ、各 Entity の ID、kind、Progress / Disposition、title、ready / blocked / orphaned / surfaced / active scope に関する例外状態を簡潔に示す。子孫の description、Note、Revision、履歴、claim 詳細は展開せず、必要な Entity を個別に `show` する。

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

人向け出力は、Entity一覧、履歴と索引、一件の詳細、状態変更の確認という役割ごとに共通の文字構造と語彙を使う。一覧と履歴は1 recordを1行に置き、一件の詳細では短い metadata を長文やdiffより先に置く。状態変更の確認は対象IDから始め、保存されたsnapshot全体を繰り返さない。Note / Revision一覧はEntity-local numberから始める。

`show` を含む人向け出力は、stdout が対話 terminal なら状態の識別を補助する ANSI style を使う。非対話出力と `NO_COLOR` では同じ文字、空白、改行、順序を無装飾で出し、状態の違いを色だけでは表さない。title、description、Note本文、reasonなど利用者が保存した文字列は装飾・省略・整形しない。下流でpipeが閉じた場合は成功として扱う。

非TTY の record 境界、先頭の識別子、標準 stream は安定した外部契約とする。

`export` と completion は生成内容そのものを標準出力へ書き、人向けの装飾を加えない。

Git 配下では common Git directory の親を管理 root とし、全 worktree で `.axon/axon.db` を共有する。Git 外ではカレントから祖先へ最も近い DB を探す。Issue と Group は同じ `<prefix>-<ランダム 6 文字>` namespace を使い、完全 ID または一意な suffix で解決する。

## DB の自動移行

DB を使う全コマンドは共通の open 境界で v9 / v10 を現行 v11 へ自動移行する。`list`、`show`、`export`、`import check` の読取用途も初回 open では書込権限が必要になる。`import prepare` も DB を参照し、自動移行対象である。`init` は新規作成専用で、既存 DB の reset や upgrade をしない。help、docs、version、completion は DB 不要である。

移行成功の版と backup 先は元の処理を始める前に stderr へ通知する。stdout の record / YAML は維持し、最新 DB では通知も backup も増やさない。移行の transaction と元の操作は独立し、元の操作の失敗は成功した移行を取り消さない。

backup は管理 root の `.axon/migration-backups` に移行直前の全保存情報を SQLite backup として残し、自動削除・上書きしない。失敗した backup は不完全な場合がある。診断は DB、版、処理段階、原因、backup 先（作成開始後）、適用状態を示す。未適用では元の操作を実行しない。適用済みなら移行は保持され、結果不明なら再実行前に DB と backup を保全・確認する。v9 未満・未来版・未知構造を変更せず拒否し、未来版には対応する新しい axon を案内する。権限、容量、他 writer の lock は表示した実 path を起点に調べる。
