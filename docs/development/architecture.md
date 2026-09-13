# アーキテクチャ

この文書は置換前の三軸 CLI の資料です。単一 lifecycle の再構築は [正本 spec](../../spec/lifecycle_proposal.md) と [新しい共通コア](../development/lifecycle-core.md) を参照してください。

[状態モデル](../reference/state-model.md) と [情報モデル](../reference/information-model.md)
を実装へ落とす際の型境界、保存構造、原子性を定める。
設計の比較と導入経緯は [設計判断](../design/decisions.md) を参照する。

## 型境界

Rust の enum、網羅的な match、EntityId newtype を使い、状態の値と ID の取り違えを型で防ぐ。
DB から読んだ Entity の値を型に変換する境界は `RawEntity::into_entity` に集約し、
別の読み出し経路に変換・検査を分散させない。この境界を通った内部の値は型を信頼する。

## 保存先と管理 root

backend 設定は持たない。Git の file は現在の worktree root の `.axon/state.jsonl`、
SQLite は common directory の親の `.axon/axon.db`。Git 外は最寄りの正本または
init.pending がある管理 root の同じ二つの名前を使う。両方なら混在、どちらもなければ
未初期化。破損・途中生成で fallback しない。一 repository 一 backend を利用契約とし、
見えない他 worktree の混在を検出するための selector や走査は持たない。

file writer は stable sidecar `.axon/write.lock` の OS lock 下で読取、core 操作、
temporary の sync、元 bytes・backend・identity 再確認、atomic replace、directory sync を行う。
no-op は bytes を保持する。replace 前の失敗は未適用、replace 後の同期失敗は結果不明。
init は backend 共通の排他、pending marker、完成正本の新規公開、補助 file 作成、
marker 削除の順に進む。既存正本の再 init は拒否し、自動修復・rollback はしない。
SQLite は ignore を変更しない。file は Git 内外とも ignore と attribute を生成・補完する。
詳細と理由は [保存先の設計](../design/storage-discovery.md) と
[保存契約](../reference/file-storage.md) を参照する。

Git index の正本に unmerged entry があれば通常操作を拒否する。
Git/editor は lock に従わないため、同じ worktree の checkout/merge と Axon write を
同時実行しない。分散 lock、network filesystem の保証、全 worktree scan は提供しない。
状態・Group・情報所有・因果履歴の意味は維持する。filesystem の障害と並行性は
既存 Quint model の対象外なので、Rust の fault/process/CLI tests で検証する。

## スキーマ

正は [src/db.rs](../../archive/three-axis/src/db.rs)。ここには構成と、そこから読み取りにくい意図だけ書く。

| テーブル | 役割 |
| --- | --- |
| `meta` | ID の接頭辞などの設定 |
| `entities` | kind、現在の plan declaration、A / B / C、claim、current Revision。Issue / Group の共有状態を持つ |
| `entity_deps` | Entity 間の依存。両端は kind の全組み合わせを許す |
| `declaration_revisions` | Accepted / Rejected と判断された plan declaration の不変な全文 Revision |
| `revision_dependencies` | 各 Declaration Revision が所有した outgoing dependency |
| `entity_notes` | 状態と独立した追記専用 Note。安定ID、内部の線形順序キー、本文、actor、保存時刻を持つ |
| `entity_events` | B (`disposition`) / C (`resurface_condition`) の Entity 単位の判断ログ。B の判断は対象 Revision を参照する |
| `entity_progress_events` | A (進行) の Entity 単位の状態遷移履歴 |

設計上の要点:

- **導出値をテーブルに持たない**。ready / blocked / orphaned / Group の進捗集計はすべて計算する。Entity 自身の Progress は保存する状態であり、集計とは区別する。比較した代替案は [設計判断](../design/decisions.md#永続化の設計判断) に記す
- **claim は `progress = 'in_progress'` のときだけ存在する**。CHECK 制約で縛り、読み出し時も `Progress::from_db` が食い違いを弾く
- **claim は actor / worktree / at を持つ**。worktree は新しい `start` では必ず保存し、移行前から存在する claim だけは `unknown` として保持する
- **C の直和型は CHECK 制約で整合性を保つ**。`resurface_kind` が取る値ごとに、どの列が埋まっているべきかを縛る
- **待機関係は DAG に保つ**。dependency、`AfterEntity`、包含を activation / completion の 2 つの wait graph に射影し、両方を同じ write transaction で検査する。group を待機元にした辺は全子孫へ展開する
- **決定済み declaration と Revision の参照を DB 制約でも一致させる**。Undecided は `current_revision IS NULL`、Accepted / Rejected は同じ Entity の Revision を必ず参照する
- **Revision と判断は同じ transaction で確定する**。採用系統のlast Revisionと同じ declaration は Revision を再利用し、違う場合だけ 新しい安定IDと内部順序キーを追加する
- **Note の追加は immediate transaction で安定IDと保存順を確定する**。入力時刻ではなくこの保存順を正にし、空白だけの本文は Store の書き込み境界で拒否する。DB の `NOT NULL` / `CHECK` 制約も NULL、空文字、U+0020 だけの本文を拒否する
- **複数テーブルの詳細表示は一つの read transaction から作る**。現在 Entity、関係、履歴、Revision、Note、件数を異なる時点から混ぜない
- **スキーマは `user_version` と既知 DDL の両方で識別する**。通常openは対応する更新経路を実行し、backend変換は現行schemaのみを入力とする。退役版・未来版・未知構造を変更しない

## 状態更新と履歴

状態更新と履歴の記録判定は SQL に依存しない `core::StateSnapshot::execute` に集約する。
`StateSnapshot` は Entity、dependency、metadata と、所有 Entity ごとの Revision、Note、
判断履歴、進行履歴を持つ。因果参照を正とし、履歴のvectorは因果順（並行時はID順）の表示用の並びとして、SQLite の row ID や順序番号を
コアへ渡さない。`StoreSnapshot` は宣言編集・表示に使う Entity と dependency の射影である。

操作 context は時刻、actor、reason、ID 生成関数、Command の評価 context を明示する。
Start の入力には作業場所と取得時刻を含む claim を渡す。コアは入力 snapshot を変更せず、
ガードを検査し、成功した操作だけを `ValidatedChange` として返す。生成 ID の種別と重複も検査する。
採用系統のlast Revisionとの比較、履歴を追加する条件、設定の no-op と遷移の反復失敗、
既存の不整合を修復する辺の削除は、コアで決定する。

SQLite adapter は immediate transaction 内で完全な snapshot を読み、コアを呼び出し、
変更された現在値と新しい履歴だけを保存して commit する。途中の失敗は全体を rollback する。
宣言 import も、同じ transaction 内の読み取り・宣言組み立て・コア検査・保存を通る。
保存順の整数キーは adapter が割り当て、既存の番号の欠番や日時の文字列表現を上書きしない。
RecordId の SQL 変換も adapter 内に置く。

この境界はSQLite単独でも使う。v13はv12の全tableを維持し、`history_lineage`、`causal_links`、
`history_baselines`、`history_merges`を追加する。保存済みの因果情報は維持する。
v14 は `AtDate` をオフセット付き RFC 3339 instant にし、v13 の日付は UTC 午前 0 時に変換して現在値・履歴・因果 payload を保持する。
新しい因果関係とcurrent/lastの検証は[分岐履歴](branch-history.md)に定める。
通常操作の情報所有モデルと分岐履歴モデルを検証し、共通操作契約をメモリとSQLiteで検証する。
CLI・Command・migrationテストで実装の境界を確認する。
判断履歴は Disposition / Resurface condition の変更について、時刻、actor、対象軸、old / new、
任意の reason を保存する。Progress の操作は別の型付き進行履歴へ保存する。
進行履歴は先行IDで因果順を保持する。同じtransactionでの逐次保存は先行関係になり、
並行するbranchの記録を入力時刻から直列化しない。
契約変更前の done reason は過去の事実として保持し、show で読み続けるため reason 列を残す。

start は対象を明示して ready の検査と claim の取得を同じ write transaction で行う。
候補選択と着手をまとめる操作は持たない。並行して先に着手された場合は更新が失敗し、
競合後の扱いは呼び出し側の workflow が決める。

## 三方向統合の境界

merge engine は三入力の bytes を所有し、digest に束縛した Entity 選択と共通 core の
通常修正操作だけを受け付ける。入力の削除・不変記録改変・identity 衝突は入力異常、
現在値の不一致と候補全体の不整合は解決可能な競合として区別する。
各 Entity の bundle を選択した後、記録集合を ID で統合し、因果先端を決定的な
MergeRecord で束ねる。時刻は勝者決定に使わない。Ended Group は選択元の
親・出力依存・子孫集合とその構造を比較し、現在の全子孫が terminal かも検査する。
通常修正操作と最終検証が成功するまで候補を公開可能な結果として返さない。
merge の比較・構造検証は Command 条件を評価しない。

自動選択の真理値表、swap 対称性、記録保持、再統合、および構造競合は独立した
決定的 Rust テストで直接検査する。既存 Quint の通常操作・明示選択の意味は変更せず、
storage 総合モデルは拡張しない。Git 接続とファイル公開は engine の外側が所有する。

## 外部条件の評価 context

Store が保持する評価 context を View と StoreSnapshot に共有し、同じ Entity の評価結果と
失敗を 1 invocation 内で再利用する。DB の読み取り transaction が別でも context は共有する。
この結果はメモリ内だけに置き、条件のシェル文字列と区別する。

CLIはleaf commandの `--condition-timeout` を呼び出し単位の評価policyとしてStoreへ渡し、
未指定時はCommand 1件ごとに30秒を適用する。CLIが `--trace-conditions` を受け取った場合だけ、
Storeは評価contextへstderr writerを渡す。
実processの終了0/1を取得した直後、結果をmemoizeする前に一つのtrace blockを書き込むため、
評価順を保ち、memoized参照を再表示しない。異常終了は既存の評価errorだけを生成する。
trace書き込み失敗も評価errorにし、状態変更のtransactionを確定しない。

評価runnerはCommandごとに専用process groupを作り、2本のpipeを並行して読みながら各streamの
先頭32 KiBと末尾32 KiBだけを保持する。timeoutまたはCtrl-CではgroupへTERMを送り、1秒の
有限な猶予後にKILLする。このprocess監督と出力保持は保存状態ではなく評価contextの責務とする。

条件の置換では付随列をまとめて更新し、旧条件の日付・参照・シェル文字列を残さない。
SQL 制約と RawEntity の型変換境界で、選択した条件に不要な付随値を拒否する。
外部プロセスの実行契約は [CLI 契約](../reference/cli.md#外部条件の評価) に置く。

## 公開 ID の生成

ID は `<prefix>-<ランダム 6 文字>`。ランダム部分は Crockford Base32
(`0-9A-Z` から `I` / `L` / `O` / `U` を除く) を使う。
prefix は初期化時に管理 root 名から決め、引数で変更できる。
Issue と Group は同じ namespace を共有し、kind や作成順を ID に埋め込まない。

## actor と作業場所

優先順位:

```
1. AXON_ACTOR          — 明示指定 (上書き手段として残す)
2. 個別の環境変数を検出  — サポート対象は簡潔な名前を出す (下表)
3. AI_AGENT            — サポート外のエージェントの自己申告
4. $USER@<作業ディレクトリ名>  — 人間が直接使う場合のフォールバック
```

個別検出を `AI_AGENT` より先に見るのは、`AI_AGENT` の値が `claude-code_2-1-251_agent` のように冗長なことがあるため。サポート対象は `claude-code` / `codex` と短く表示する。

**サポート対象は Claude Code と Codex のみ**とする (他は必要になってから足す)。

| エージェント | 検出に使う変数 |
| --- | --- |
| Claude Code | `CLAUDECODE` / `CLAUDE_CODE` |
| Codex | `CODEX_SANDBOX` / `CODEX_THREAD_ID` |

actor は一覧と調査の手掛かりであり、排他制御や `release` の事前条件には使わない。claim には actor に加えて、`git rev-parse --show-toplevel` で取得した worktree と `start` の時刻を保存する。Git 外では作業場所を失わないためカレントディレクトリを保存する。

## schema 切り替えの境界

通常操作のSQLite更新は `src/db/upgrade.rs`、file更新は `src/file_upgrade.rs` が担当する。現行の基点から次のschemaへ進む際に、入力の検証と全情報保持の変換を追加する。現行版では更新経路は空で、通常読取と完全検証を行う。過去の移行だけのためにv9〜v12の変換経路を保持しない。
SQLiteは排他取得・版再確認・backup・transaction更新、fileはwriter lock・backup・入力再照合・atomic replaceを使う。失敗時の保全、版とbackupの通知、通常操作の続行は[移行契約](../reference/migration.md)に従う。
`src/db/migration.rs`のbackend変換はこの自動更新を呼ばず、現行schemaだけを読み取り専用で変換する。完全snapshotの往復、最終backendの再読取、digestを検証し、manifestを最後に公開する。元の保存先を切り替えない。

## Merge workspace

CLI adapter は独立 directory に原本、保存先 preimage、active root と入力 store ID、固定 context を凍結する。
`resolution.json` の選択と通常操作だけが編集対象で、check は候補と report、検査済みの
manifest/resolution/candidate digest を更新する。apply は workspace と保存先の sidecar
lock 下で全 digest、store identity、preimage を再照合して atomic replace する。
入力・context drift、未解決、domain error を区別する。障害は Rust の fixture と fault
テストで検査し、既存 Quint の状態・履歴意味は変更しない。
Git driver は backend discovery を使わず三つの raw 入力を保存する。空・欠落・不正な
祖先を合成せず診断を残し、recursive driver は binary として曖昧な仮想祖先を拒否する。

## ビルド時のソース由来

`--version` / `-V` は package version を先頭に、ビルド時の完全 commit hash と
`source clean|modified|unknown` を補足する。build script が文字列を埋め込み、実行時は
Git・環境変数・DB を参照しない。canonical build も通常の Cargo build と同じ仕組みを使う。

配布工程では `AXON_BUILD_COMMIT`（40 桁または 64 桁の ASCII 十六進数、または `unknown`）と
`AXON_BUILD_SOURCE_STATE`（`clean`、`modified`、`unknown`）を指定できる。
どちらか一方でも存在すれば明示指定モードとし、省略した項目は `unknown` にする。
Git 自動取得とは混ぜない。不正値・空文字・非 Unicode 値は警告して当該項目を `unknown` とする。
たとえば上流 commit に配布側パッチを当てる場合は、その commit と `modified` を明示する。

指定がなければ Cargo package root 自身が Git worktree root の場合だけ自動取得する。
linked worktree にも対応する。親 repository 内へ展開した source archive は親の HEAD を採用しない。
Git を見つけられない場合や各照会が失敗した場合は、取得不能な項目を `unknown` にする。
Git の repository/index/pathspec を切り替える環境変数は照会から除外する。

modified は staged / unstaged の tracked file と Git が無視しない untracked file を対象にする。
package root の `.axon`（管理データ）と `target`（標準 build artifact）は tracked でも除外する。
その他の生成物は `.gitignore` 等で除外する。無視されたソース、Git の assume-unchanged /
稼働中の並行編集などまで検知する保証はない。Git 照会は build 開始時の観測である。
commit のみ、変更の有無のみ、metadata のみの変化でも情報を更新するため、build script は
毎回の Cargo build で再実行する（由来の照会と axon crate 再コンパイルのコストがある）。

これは完全なソース内容や binary の同一性の証明ではない。正式 release では version が通常の
識別の主となり、commit は補助情報である。commit と modified だけでは配布側のパッチ、
ビルド設定、未 commit 差分の内容を識別できない。厳密な監査では binary hash と package 情報も記録する。
