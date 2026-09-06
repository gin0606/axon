# Worktree ごとの file backend と三方向 merge

この文書は 2026-09-06 の計画・仕様検討の成果である。未実装の新仕様を示す。
現行 CLI の規範を置き換えるものではない。実装時は、ここで定めた意味を検証してから
担当する reference 文書と形式モデルへ反映する。
作業単位と依存関係は `axon show axon-x7j4we` で参照する。

## 目的と合意した境界

- file backend のすべての読み書きは、実行した worktree 内で完結する。
  start、claim、Note を含む変更でメインや別 worktree の working tree に差分を作らない。
- 別 worktree への即時伝搬・排他は提供しない。同じ Issue をそれぞれで start できる。
  Git merge 等でファイルを取り込んだ時点で状態が伝搬する。
- 初版から三方向 merge と競合解決を提供する。機械的に確定する部分はツールが処理し、
  意味の判断は人間またはエージェントが補助する。候補の検証と安全な保存はツールが担う。
- Note、Revision、履歴は生成時の安定 ID で参照する。番号を表示しても永続参照にしない。
- 旧 CLI・旧保存形式の後方互換は必須にしない。既存 SQLite の情報を失わず、手動で
  新形式へ移行できることを必須にする。
- SQLite と file の選択肢は維持する。新モデルは共通にし、保存方式ごとに状態操作を複製しない。
  file backend に永続 SQLite cache や同期元 DB を置かない。

旧 Group `axon-j81sx8` は再開しない。旧成果 `8856b7a` / `8a80b87` は設計資料であり、
現在の自動 migration、Manual / Command や、この文書の merge 契約より優先しない。

## 先行導入する範囲

安定 ID 化と状態操作の分離を先に完成させ、線形履歴の SQLite として利用できる区切りを作る。
以降の分岐・merge は最終仕様であり、先行二段階の完了条件へ混ぜない。

1. **安定 ID 化**: Note / Revision / typed history の識別と参照を安定 ID に変える。
   保存順は従来どおり stream 内の線形順序として保持する。ID と順序を分け、
   Revision の再利用・通常の状態遷移・履歴生成の意味は変えない。
   現行 SQLite v11 からこの段階の SQLite への全情報保持の手動移行と旧番号対応表を含める。
   SQLite 内の既存更新処理を使ってよく、core 分離をこの段階の前提にしない。
2. **状態操作と SQLite の分離**: 安定 ID と線形履歴のモデルを使い、状態操作・履歴生成・
   検証を SQL に依存しない境界へ移す。SQLite adapter へ接続し、同じ DB で全 CLI が使える
   状態を維持する。分離だけのために保存形式や root 解決を変更しない。
3. **分岐する記録モデルと codec**: 因果関係、MergeRecord、current / last Revision の系統、
   canonical JSONL を追加し、共通 core と SQLite も対応させる。先行段階で発行した ID と
   本文・記録を維持して移せる手動変換を含める。
4. **file と merge の提供**: 三方向 merge と file 保存を成立させ、競合解決 CLI / Git 接続、
   file への移行を接続する。

前半では因果 DAG、merge による現在値選択、JSONL、backend 設定・保存先の変更を導入しない。
SQLite の内部順序キーを残すことは安定 ID と矛盾しない。そのキーを公開参照にせず、
後続の分岐対応でも安定 ID を作り直さないことが境界となる。前半だけで利用可能であることと、
file backend なしでも必須の設計改善であることは区別する。

## 現在値と、分岐した記録を分ける

一つの公開 snapshot は、一つの Entity 集合とその現在値、および保持している全記録を持つ。
現在の Progress、Disposition、Resurface condition、claim、declaration、関係は一つに定まる。
過去の記録は分岐できる。snapshot は、その分岐を一本の架空の操作列へ変換しない。

例えば同じ NotStarted から、L で start→done、R で start→release が起きた場合、
四つの実操作記録を保持する。merge で L の Ended を採用しても、R の操作を削除せず、
R の release が L の done の後に行われたとは表示しない。

統合は新しい種類の記録 `MergeRecord` で表す。親となる記録の先端、統合前の候補値、
統合結果、解決方法を記録する。通常の Started / Done / Disposition 判断を捏造しない。
統合後の通常操作は統合記録の後へ追記する。現在値の根拠は、選択された履歴の先端、
または最新の統合記録となる。「全履歴を時刻で並べた最後の判断」とはしない。

これは全操作を再生して現在値を求める event sourcing ではない。現在値は snapshot に
直接保存し、履歴は参照と因果順を保持する。Undecided 中の本文編集をすべて履歴化する
機能も追加しない。分岐後の draft の競合では、三入力の現在値を比較する。

## 論理保存モデル

| 構成 | 内容 |
| --- | --- |
| Metadata | 論理 schema、format version、store ID、Entity ID prefix |
| Entity | ID、kind、作成・更新時刻、現在 declaration、Control state、current / last Revision ID、履歴先端 |
| Revision | 安定 ID、owner、declaration 全文、生成時刻、origin、先行 Revision 参照 |
| Note | 安定 ID、owner、本文、actor、時刻、先行 Note の参照 |
| 判断・進行記録 | 安定 ID、owner、種類、元の typed payload、因果上の先行記録 |
| MergeRecord | 安定 ID、owner、統合した先端、入力 identity、候補値と結果、解決方法 |
| Migration baseline | 安定 ID、元 schema、元記録の識別情報、既知の順序、移行時の現在値 |

`ready`、`blocked`、`orphaned`、active scope、Command の実行結果などの導出値は保存しない。
prefix が同じことは同じ store の証拠にしない。clone / worktree は store ID を継承し、
独立 init した store 同士は merge しない。

### ID と順序

- 新しい Note / Revision / typed history は種類付きのランダム 128-bit ID を生成する。
  同じ本文・actor・時刻でも、独立した追加には別 ID を付ける。
- 同じ ID と同じ payload は同じ記録。同じ ID で payload が異なる入力は拒否する。
  Entity ID の衝突も自動で別 ID に変換しない。
- Revision の参照と current Revision、Note の本文からの参照は ID に固定する。
  CLI の `note show` / `revision show|diff` は安定 ID を受け、短縮は一意の場合だけ許す。
- 因果関係を先行 ID で保持する。同一 stream の逐次追加は先行記録の後、独立した
  branch の追加同士は並行であり、wall-clock から前後関係を作らない。
- 表示と canonical 出力は因果順を守り、並行な要素の整列には ID を使う。
  表示では分岐・統合を識別できるようにし、整列順を保存の先後とは説明しない。
- 取り込んだ Note、Revision、typed history の ID・payload・既知の順序を変えない。
  merge で current Revision を選ぶことと、他方の Revision を削除することを分ける。

Undecided への変更は current Revision を解除するが、採用した系統の last Revision は
保持する。再判断時の再利用判定はこの last Revision と比較し、全 branch の Revision を
時刻や ID で並べた最後とは比較しない。新しい Revision は採用系統の last Revision を
先行参照に持つ。分岐した Note の後に追加する Note は、その時点の全 Note 先端を先行参照に持つ。

新しい共通モデルでは両 backend に同じ ID 契約を適用する。旧番号互換のための
backend 別分岐を core に残さない。移行前の本文中の自由記述は書き換えず、旧番号から
新 ID への対応表を移行成果に含める。

## 三方向 merge

入力は base / ours / theirs の完全な snapshot とし、各入力の bytes と digest を固定する。
store ID と schema の一致、記録・参照の integrity を確認する。base にあった Entity や
不変記録を sides が削除・改変した場合は、通常の分岐として自動修復しない。
古い履歴全体への耐改ざん性は保証せず、与えられた三入力に対して検査する。

### 初版の統合単位

現在値については Entity の次の bundle を一体で扱う。

`declaration + outgoing dependencies + Disposition + current/last Revision + Progress + claim + Resurface`

片側だけの変更は採用する。両側の bundle が同一なら同じ現在値を採用し、異なる ID の
記録は両方保持する。両側が異なる bundle に変更した場合は、初版では field ごとの
推測で合成せず Entity の競合として提示する。Note の追加は bundle の変更ではないので、
別 Note 同士、Note と状態変更は自動統合できる。

`created_at` と identity は不変。`updated_at` は採用した変更の情報を保持し、同じ現在値に
収束する場合の表示上の最大時刻を採用しても、時刻を状態の勝者の決定に使わない。
Note だけの追加で Entity の updated_at を変えることはしない。

記録は ID で集合統合し、因果関係を保つ。分岐した履歴の先端をまとめる場合には
MergeRecord を追加する。自動統合記録の ID は、version、owner、順序を正規化した
親先端、結果と解決内容から決定的に生成する。時刻や ours / theirs の呼び名を
identity に混ぜず、同じ統合を別々に計算しても同じ記録にする。
記録に保存する選択元も ours / theirs という相対名から入力 digest / record ID へ正規化する。
自動統合記録に新しい wall-clock や実行者を付けて決定性を壊さない。人間による解決と
通常の修正操作の actor / 時刻は作業 artifact 内で一度だけ固定する。

すでに片側が他方の記録をすべて含み、現在値も一致する場合は新しい統合記録を作らない。
同じ解決 artifact の再適用は bytes を変えない no-op とする。内容が似た別の
ユーザー操作を同じものと判断してはならない。

### 全体検証と意味の競合

| 例 | 結果 |
| --- | --- |
| 別 Issue の独立した変更 | 両方を候補へ取り込み、全体を検査 |
| 同じ Issue へ独立に同じ本文の Note を追加 | 異なる ID の二件を保持 |
| 双方で同じ番号相当の Revision を追加 | 別 ID のまま保持。各判断の参照を維持 |
| 双方が同じ Issue を異なる claim で start | bundle の競合。claim だけを合成しない |
| 一方が done、他方が release | 現在値を解決し、両側の履歴を保持 |
| 一方が draft を編集、他方が旧 draft を Accepted にする | bundle の競合。未判断の draft を Accepted にしない |
| 一方が A→B、他方が B→A を追加 | 循環を示す構造競合 |
| 一方が Group を done、他方が子を追加・移動 | 終了した Group の構造に関する競合 |
| 同一 ID の不変記録を別内容に変更 | 入力異常として拒否 |

候補全体で、一親 tree、二つの wait graph、参照先、claim と Progress、Disposition と
current Revision、および決定済み declaration の一致を検査する。Ended Group の
構造固定は、結果だけでなく双方の入力との比較も行う。結果で全員が terminal に見える
だけでは、Ended Group に新しい子を追加してよい根拠にならない。解決後も Ended を採用する
Group は、明示した採用元の親・依存・subtree 構造を保持する。異なる構造のまま双方で
Ended になった Group も、同じ bundle というだけで自動統合しない。

### 人間・エージェントによる解決

競合一覧には stable conflict ID、種類、対象 Entity、base / ours / theirs、関連する
Revision・履歴・辺を含める。解決 artifact で現在値の採用側を明示する。全体競合では
関連する複数 Entity の選択を変更でき、通常の Axon 操作による修正案も記述できる。
Ended と未終了の競合で後者を明示採用することは merge の現在値選択であり、通常の
Ended→NotStarted 操作を追加することではない。完了宣言が現在値に採用されなかったことは
競合表示と MergeRecord に残す。選択によって Entity や過去の完了記録を削除しない。

修正操作は候補に対し共通 core の guard と履歴生成を通して適用する。
決定済み declaration を編集するなら Undecided 化と再判断を明示する。
不変記録の編集・削除、claim だけの差し替え、履歴にない通常状態遷移の捏造は許さない。
通常の辺除去による repair と、復元不能な入力の拒否を区別する。

修正途中の候補が全体の構造検査を通らなくても正本にはしない。入力保存、個別解決、
通常操作の検証、最後の全体検証を作業 artifact 内で完結させ、完全な結果だけを publish する。
自動で解けない不変条件を満たすために履歴を捨てず、解決不能なら入力と診断を保持して停止する。

merge 自体の検証は Command 条件の shell を実行しない。利用者が選んだ修正操作が
ready を必要とする場合だけ、通常 CLI と同じ明示的な評価 context で実行し、失敗を伝える。

## CLI と Git の境界

以下は新しい command 群の責務を固定する。flag の細部は Clap 実装時に整える。

| 操作 | 入力・効果 |
| --- | --- |
| `axon storage check <snapshot>` | 保存情報・参照・構造を読み取り専用で検査 |
| `axon merge prepare` | 三入力を凍結し、候補と競合一覧を作業 directory に作る。正本を変えない |
| `axon merge check <workspace>` | 解決案から候補を検証。未解決と drift を区別して報告 |
| `axon merge apply <workspace>` | 検証済み候補だけを指定先へ原子的に保存 |
| `axon merge driver` | Git から渡された三入力へ同じ engine を適用する薄い接続 |
| `axon migrate` | 明示した旧 SQLite から未使用の移行先を作る。元 DB を変更しない |

prepare は base / ours / theirs の原本と digest、保存先の prepare 時 bytes の digest、
解決案、固定した修正操作 context、候補を保存する。保存先が conflict marker を含む場合も、
その bytes は競合検出用の preimage として保持し、状態入力には使わない。
apply は lock 下で保存先の一致を再確認する。入力原本・candidate・修正操作の drift は拒否する。
成功済み candidate と保存先が完全一致する再試行は no-op。結果不明では推測して再実行しない。
解決案そのものは編集対象であり、編集後に check で候補を再計算して検査結果を更新する。
apply が固定するのは最後に検査した解決案・context・candidate の組であり、prepare 時の
未解決案を永久に凍結する意味ではない。check は入力原本と利用者の解決案を上書きしない。
active root へ保存する場合は設定の identity と digest も固定し、backend の変更を見逃さない。

Git の標準テキスト merge の成功だけで正しい状態とは判断しない。通常 open も保存情報の
検査を行う。driver がない環境では明示的な prepare / check / apply で解決できる。
Git index 上で設定または正本が unmerged の間は、通常の Axon 操作を拒否する。
marker がない片側の有効ファイルだけが残っていても、解決済みとはみなさない。
driver の登録は明示的な setup とし、通常 init や読み書きで Git 設定を黙って変更しない。
未解決なら非 0 を返して conflict を残し、部分的に valid に見える snapshot を返さない。
Git が渡す一時入力は失われ得るため、手動解決用の原本は別の作業 directory へ保持する。
driver は通常の backend discovery を呼ばず、受け取ったファイルだけを処理する。

複数 merge base の内部統合、add/add、delete/modify も driver の検証対象とする。
祖先を安全に構成できない場合は conflict として明示解決へ渡し、空の base を推測しない。
Git が driver を呼ばない fast-forward 等の経路を、driver が検証したとは説明しない。
Axon は stage、commit、pull、push、rebase の継続を実行しない。

## 保存形式と root

file の正本は active root の `.axon/state.jsonl` 一つとする。UTF-8、LF、末尾 LF、
一行一 record。header を先頭に、Entity、Revision、Note、typed history、MergeRecord、
baseline を種類と ID の決定的な順に配置する。record 内の field 順、日時の UTC 表記、
null と空集合の扱いを codec に集約する。未知 field / version、重複 ID、欠けた参照を拒否する。
集合としての ID 配列は sort し、意味のある順序は並べ替えない。
reader は無害な record 順・空白の違いを受け入れ、writer が canonical bytes を生成する。
no-op は正規化のためだけにファイルを書き換えない。

backend の選択は active root の `.axon/config.json` に置き、Git ではその worktree に
属する設定とする。設定は backend、設定 schema、接続する store ID を持ち、正本の
store ID との一致を検査する。prefix や Entity の内容は設定に複写しない。
file と設定は Git で追跡し、lock と merge workspace は追跡しない。

- Git 配下では `--show-toplevel` の root を使う。祖先や別 worktree に fallback しない。
- Git 外では最寄りの `.axon/config.json` を持つ祖先が root。壊れた設定で探索を続けない。
- file 設定ならその root の state.jsonl だけを開く。欠落時は空 state や SQLite に fallback しない。
- SQLite 設定なら Git では common directory 配下の `axon/state.db`、Git 外では
  active root の `.axon/state.db` を使う。SQLite の共有は保存先の性質であり、file に持ち込まない。
- 設定なしは未初期化。旧 `.axon/axon.db` があれば移行を案内し、通常 open で暗黙変換しない。
- backend はコマンド単位の flag で変更しない。異なる backend のデータへ設定だけを
  書き換えて転用する経路は migration として認めない。
- 他 worktree の file と共有 SQLite の共存は、それだけでは異常としない。active root の
  明示設定が一つの正本を選ぶ。全 worktree の保存物探索や共有 file binding を導入しない。

`init` は明示指定、または省略時 SQLite を選ぶ。既存データを上書きせず、同じ有効な
設定・正本が揃っていれば no-op。新規生成では complete data を先に、設定を最後に
publish し、通常 open は設定と正本が両方 valid の場合だけ成功する。中断で一方だけが
ある場合はその path と状況を示し、空データの自動再生成はしない。既存の valid な共有
SQLite を別 worktree に登録するときは、その DB を再初期化せず設定だけを作る。
clone で取得した file 設定と正本は追加の shared binding なしで使える。

## 更新の成功境界

file の writer は正本とは別の stable sidecar に OS lock を取り、lock 下で読み、
core の検証と操作を行い、同じ filesystem の一時ファイルへ完全に書いて同期する。
publish 前に元 bytes を再照合し、原子的な置換と directory 同期を完了して成功を返す。
sidecar を通常書込で replace / unlink しない。process 終了時は OS が lock を解放する。

replace 前の未適用、正常に適用済み、replace 後に耐久性を確定できない結果不明を分ける。
失敗/no-op で一部の Entity や履歴だけを保存しない。SQLite も同じ変更を一つの
write transaction で公開する。CLI は保存成功より先に変更成功を表示しない。

Git や editor は sidecar lock に従わない。再照合後の非協調 write を防げるとは主張しない。
同じ worktree の Git checkout / merge と Axon 書込は同時に行わない利用契約とする。
複数 host の分散排他、network filesystem の透過的な保証、耐改ざん性は対象外。

## 実装境界

新しい共通 StateSnapshot と Command → validated change の境界を置く。
core は現在値、record integrity、状態操作、Revision 再利用、記録生成、構造検証を所有し、
SQLite / file は一貫した read と原子的な保存を所有する。既存の derived と状態 guard は
再利用し、操作群ごとに core と SQLite を同時に接続して常に動作確認できる単位で移す。
この分離はまず安定 ID と線形履歴の段階で完成させ、因果関係と merge の意味は後続で
共通モデルへ追加する。後続の field を予め実装したことを分離の完了条件にしない。

メモリ SQLite への全量往復を正式な file 実装にはしない。現行 schema の番号・線形履歴へ
分岐を投影する変換を加えるより、新しい記録モデルを共通に扱う。SQLite の新物理 schema は
adapter の内部設計とし、core に SQL transaction 型や物理 row ID を漏らさない。
汎用 plugin API や第三者 backend のための抽象化は追加しない。

## 手動 migration

最低対応入力は現行 SQLite schema v11。v9 / v10 は既存の対応版 Axon で v11 にした後、
この変換を使える。移行 tool は未知版・未知構造を変更せず拒否する。

移行経路は `v11 → 安定ID・線形履歴SQLite → 分岐対応SQLite → file` と段階化する。
安定 ID 化の成果に最初の変換、分岐モデルの成果に次の変換を含め、それぞれの段階で利用を
開始できるようにする。状態操作の分離そのものでは DB の再変換を要求しない。
最終の migration 作業は既存の変換を再実装せず、経路の接続と file 出力・切替手順を所有する。
v11 から最終形式へ直接出力する入口も同じ経路を順に適用する。

1. 利用者が writer を止め、移行元と未使用の出力先、出力 backend を明示する。
2. SQLite backup API により WAL を含む一貫した入力を専用 staging に固定する。
3. Entity、関係、全 Note・Revision・判断履歴・進行履歴、claim、prefix、日時、legacy reason、
   baseline を全量変換する。原本の DB・WAL・設定は変換処理で変更しない。
4. source snapshot の正規化した論理内容の digest と元の table / key から決定的な新 ID を作り、番号・参照を
   対応付ける。既知の stream 内順序だけを保ち、別 table 間の時系列を作らない。
   Migration baseline で移行時現在値へ接続する。古い事実を新規操作として再生成しない。
5. 新 SQLite または file と、入力 snapshot、schema・件数・ID 対応表・検査結果の manifest を
   新しい移行先へ出力する。既存の出力先を上書きしない。
6. 全情報の比較と新 backend での読み取りを確認後、利用者が旧版の書込を止めたまま
   設定・保存先を明示的に切り替える。tool が既存 root を自動で切り替えることはしない。

同じ固定入力から別々に変換しても同じ store ID と記録 ID を得る。同じ DB の異なる
時点から独立変換したものを merge 可能な共通起点とは扱わず、原則一度変換した成果を
各 branch / worktree へ配布する。失敗時は staging を保全し、元 DB を引き続き調査できる。
安定 ID 導入後の DB からは、既存 store / record ID をそのまま運び、source digest の変化で
付け直さない。source からの ID 生成は、ID をまだ持たない v11 からの変換に限定する。
ID 用の論理 digest と、保全した backup file の bytes digest は区別する。未使用 page や
backup の物理配置の違いを、同じ旧記録の別 identity にしない。

## 実装前と実装中の検証

現行の core 状態軸は維持する。情報モデルでは、番号の単調増加を ID と因果関係へ置き換え、
分岐の記録保持、統合後の現在値の根拠、追記専用性を検証する。Group model は現在値の
操作 guard を維持し、merge の双方比較と全体構造は小さな独立した検査で扱う。
一つの巨大な storage model に init、Git、cache、migration を詰め込まない。

受け入れケースは上の表に加え、次を含める。

- 同一三入力の決定性、取り込み済み branch の再 merge、同じ解決案の再 apply。
- 未解決・入力 drift・書込失敗で正本を部分変更しない。
- 別 Issue の更新からできる循環、Ended Group の子集合変化、決定済み declaration の混成を拒否。
- 安定 ID が merge 後も同じ記録を指し、独立した同内容 Note を失わない。
- worktree A の全変更で main / B の bytes が変わらず、明示 merge 後だけ伝搬する。
- 両 backend に同じ時刻・ID・外部評価を与え、状態・記録・no-op・失敗の同等性を比較。
- migration の全旧 field / row / 参照対応と、失敗時の元 DB 保持を合成 fixture で比較。
- ready / show / merge を代表的な Entity・履歴量で計測し、永続 cache は初版に含めない。

## 調査根拠

現行実装では `src/db.rs` の StoreSnapshot は Entity と依存を持つが、全履歴の保存形式ではない。
Note / Revision は Entity 内連番、判断履歴は Revision 番号を参照する。
`src/declaration.rs` の import は plan declaration の操作であり、Control / history merge の代用ではない。

2026-09-06、Git 2.55.0 の `git merge-file -p` で合成 JSONL の三入力を試した。
別 Entity の A→G と G→A の追加は exit 0、隣接 Entity の別編集と同じ Note 番号への
異なる追加は exit 1、同じ Note payload の双方追加は exit 0 で一行に統合された。
これは文字列 merge の観測であり、Axon merge engine の検証結果ではない。

Git の driver は file-level merge が必要な場合に三入力を受け、結果と終了値を返す。
仕様の根拠は [gitattributes](https://git-scm.com/docs/gitattributes#_performing_a_three_way_merge) と
[git-merge-file](https://git-scm.com/docs/git-merge-file)。この設計文書の記録 DAG、整合性検査、
安全な保存を Git が提供するという意味ではない。
