# CLI

## 方針

axon の操作対象は `Issue` と `Group` の 2 kind を持つ Entity である。どちらも同じ公開 ID、文面、Progress、Disposition、Resurface condition、claim を持つ。対象 kind を先に選ばせる namespace は作らず、ID を受け取る top-level command が kind を解決する。

完全な利用者向け help は英語の `docs/help.md`、状態モデルと設計理由の正は `docs/axes.md` に置く。Usage、引数、option、コマンドツリーは Clap の定義から生成する。`axon help` と `axon --help` は完全 help、`axon -h` は短い一覧を返す。

## コマンド境界

### 作成

| kind | Accepted で作成 | Undecided で作成 |
| --- | --- | --- |
| Issue | `axon plan <title>` | `axon capture <title>` |
| Group | `axon group plan <title>` | `axon group capture <title>` |

4 コマンドはいずれも `--parent <group-id>` を受け取り、作成と包含設定を同じ transaction で行う。呼び出すたびに新しい Entity を作る追加操作である。

旧 slug ベースの `group new` / `group list` / `group show` / `group reject` / `group dep` は提供しない。Group は Issue と同じ自動生成 ID で参照する。

### 共通の状態・宣言・関係操作

次の command は ID から kind を解決し、Issue / Group の両方へ同じ入口を使う。

- `show` / `write` / `log`
- `note add|list|show` / `revision list|show|diff`
- `start` / `done` / `release`
- `decide accept|reject|undecide`
- `when at|after|clear`
- `dep add|rm`

`when after` の参照先と dependency の両端も kind の全組み合わせを許す。`AfterEntity` は参照先が Ended または Rejected なら浮上する。dependency は Rejected を前提喪失として扱う。

title、description、parent Group、outgoing dependency は対象 Entity 自身が所有する plan declaration である。Undecided は draft として実変更でき、Accepted / Rejected は判断対象を固定する。固定済み declaration の変更は `decide undecide`、編集、全文確認、再判断として露出させる。同じ値の再指定は実変更ではないため no-op とする。

Accepted / Rejected への判断時には declaration 全文を Entity 内連番の Declaration Revision として保存する。直前の Revision と同じなら再利用し、判断履歴から対象 Revision を参照できる。`revision list|show|diff` は Entity と Revision を一つの read transaction から読み、`revision diff` は title、description、parent、outgoing dependency を別々に比較する。optional な description は `present` / `absent` を本文と分けて表示する。

`note add` は本文または file から非空の Note を一件追記する追加操作である。Issue / Group、Progress、Disposition を問わず使え、declaration、状態、関係、導出値を変えない。Note は Entity 内連番、本文、actor、保存時刻を持ち、通常操作では編集・削除しない。`show` は declaration の固定状態と記録件数を冒頭に示し、description と全 Note を一つの read transaction から保存順で省略せず表示する。

### 包含

包含だけを group namespace に置く。

- `group set <entity-id> <parent-group-id>` は親の新規設定または移動
- `group unset <entity-id>` は親の解除

親は必ず Group とし、各 Entity は最大 1 つの親を持つ。設定操作なので、すでに同じ親である set と、親がない unset は成功 no-op になる。

### 一覧

`ready` / `triage` / `claims` / `list` は両 kind を同じ一覧に出し、`--kind issue|group` で任意に絞る。1 Entity を1行に出し、各行の第1列はID、第2列はkindとする。`triage` は `Reason:`、`claims` は `Claim:`、`Worktree:`、`Started:` をidentityの後に置く。`list` は Progress / Disposition と、該当する例外状態だけを表示する。

`claims` は claim の経過時間やプロセス状態から staleness を推定しない。表示された保存済み事実を基に人が判断し、必要な claim だけ `release` で明示的に解放する。

### Plan 宣言ファイル

複数 Entity と関係を一枚で編集するときは、`axon export` と `axon import prepare|check|apply` を使う。形式の正は `docs/declaration-file.md` とする。

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

失敗した遷移と成功 no-op は状態、履歴、`updated_at` を変えない。`start` と `done` は reason を持たず、`release` の任意 reason は進行履歴、`decide` / `when` の任意 reason は判断履歴に保存する。作業結果や通常の申し送りは、declaration の description ではなく Note に残す。

## 原子性と deadlock 防止

`start` は ready の検査と claim の取得、Group の `done` / `release` は子孫条件の検査と進行更新を、それぞれ同じ write transaction で行う。

dependency、`AfterEntity`、包含を activation wait graph と completion wait graph に射影する。relation の追加・置換は両 graph の非循環と包含の不変条件を同じ transaction 内で検査してから保存する。Group を待機元にした dependency / `AfterEntity` は Group 自身と全子孫へ展開する。既存データの循環を解消できるよう、辺を除く `dep rm` / `when clear` / `when at` / `group unset` は他の循環が残っていても実行できる。

Ended Group は完了宣言を後から無効にしないため、親変更、subtree の出入り、依存元としての dependency 変更を拒否する。Ended Group 配下の terminal Entity を非 terminal に戻す Disposition 変更も拒否する。

## 出力と管理 root

help、一覧、詳細、成功確認は stdout、エラーは stderr に出す。一覧が空なら stdout を空に保ち、案内だけを stderr に出して成功する。

人向け出力は、Entity一覧、履歴と索引、一件の詳細、状態変更の確認という役割ごとに共通の文字構造と語彙を使う。一覧と履歴は1 recordを1行に置き、一件の詳細では短い metadata を長文やdiffより先に置く。状態変更の確認は対象IDから始め、保存されたsnapshot全体を繰り返さない。Note / Revision一覧はEntity-local numberから始める。

`show` を含む人向け出力は、stdout が対話 terminal なら状態の識別を補助する ANSI style を使う。非対話出力と `NO_COLOR` では同じ文字、空白、改行、順序を無装飾で出し、状態の違いを色だけでは表さない。title、description、Note本文、reasonなど利用者が保存した文字列は装飾・省略・整形しない。下流でpipeが閉じた場合は成功として扱う。

`export` と completion は生成内容そのものを標準出力へ書き、人向けの装飾を加えない。

Git 配下では common Git directory の親を管理 root とし、全 worktree で `.axon/axon.db` を共有する。Git 外ではカレントから祖先へ最も近い DB を探す。Issue と Group は同じ `<prefix>-<ランダム 6 文字>` namespace を使い、完全 ID または一意な suffix で解決する。
