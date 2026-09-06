# アーキテクチャ

[状態モデル](../reference/state-model.md) と [情報モデル](../reference/information-model.md)
を実装へ落とす際の型境界、保存構造、原子性を定める。
設計の比較と導入経緯は [設計判断](../design/decisions.md) を参照する。

## 型境界

Rust の enum、網羅的な match、EntityId newtype を使い、状態の値と ID の取り違えを型で防ぐ。
DB から読んだ Entity の値を型に変換する境界は `RawEntity::into_entity` に集約し、
別の読み出し経路に変換・検査を分散させない。この境界を通った内部の値は型を信頼する。

## 保存先と管理 root

| | 内容 |
| --- | --- |
| 用途 | **個人のタスク分解・管理**に絞る。プロダクト全体の ITS としては使わない (別ツールにする) |
| git | **管理しない** (ignore) |
| 保存形式 | **SQLite 単体**。git 用のエクスポートを持たない (二重持ちをしない) |
| 配置 | 管理 root ごとに `.axon/axon.db` を 1 つ置く。Git リポジトリでは全 worktree から同じ DB を共有する |
| Git 配下での解決 | `git rev-parse --git-common-dir` で共通ディレクトリを求め、その親を管理 root にする |
| Git 外での解決 | `init` はカレントディレクトリを管理 root にする。通常操作はカレントから祖先へ最寄りの `.axon/axon.db` を探す |
| 同期 | 不要 (1 マシン・1 DB) |

状態モデルは Git に依存しないため、Git 外でも同じ DB と操作を使える。Git 外ではサブディレクトリから祖先を探索し、候補が複数あれば最も近い管理 root を選ぶ。ただし通常の `init` は既存の管理 root 配下に暗黙の入れ子を作らず、その root を示して失敗する。

Git リポジトリ内では common root を管理境界として常に優先する。外側に Git 外の axon DB があってもフォールバックしないため、Git リポジトリの issue が別の管理単位へ紛れ込まない。

## スキーマ

正は [src/db.rs](../../src/db.rs)。ここには構成と、そこから読み取りにくい意図だけ書く。

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
- **Revision と判断は同じ transaction で確定する**。直前と同じ declaration は Revision を再利用し、違う場合だけ 新しい安定IDと内部順序キーを追加する
- **Note の追加は immediate transaction で安定IDと保存順を確定する**。入力時刻ではなくこの保存順を正にし、空白だけの本文は Store の書き込み境界で拒否する。DB の `NOT NULL` / `CHECK` 制約も NULL、空文字、U+0020 だけの本文を拒否する
- **複数テーブルの詳細表示は一つの read transaction から作る**。現在 Entity、関係、履歴、Revision、Note、件数を異なる時点から混ぜない
- **スキーマは `user_version` と既知 DDL の両方で識別する**。v12の通常openは旧版を変換せず、手動変換はv11を入力とし、未知版・未知構造を変更しない

## 状態更新と履歴

状態更新と履歴の記録判定は `Store::apply` に集約し、同じ transaction で行う。
判断履歴は Disposition / Resurface condition の変更について、時刻、actor、対象軸、old / new、
任意の reason を保存する。Progress の操作は別の型付き進行履歴へ保存する。
進行履歴の順序は入力時刻ではなく、同じ transaction で保存された順序を正とする。
契約変更前の done reason は過去の事実として保持し、show で読み続けるため reason 列を残す。

start は対象を明示して ready の検査と claim の取得を同じ write transaction で行う。
候補選択と着手をまとめる操作は持たない。並行して先に着手された場合は更新が失敗し、
競合後の扱いは呼び出し側の workflow が決める。

## 外部条件の評価 context

Store が保持する評価 context を View と StoreSnapshot に共有し、同じ Entity の評価結果と
失敗を 1 invocation 内で再利用する。DB の読み取り transaction が別でも context は共有する。
この結果はメモリ内だけに置き、条件のシェル文字列と区別する。

CLIが `--trace-conditions` を受け取った場合だけ、Storeは評価contextへstderr writerを渡す。
実processの終了0/1を取得した直後、結果をmemoizeする前に一つのtrace blockを書き込むため、
評価順を保ち、memoized参照を再表示しない。異常終了は既存の評価errorだけを生成する。
trace書き込み失敗も評価errorにし、状態変更のtransactionを確定しない。

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

v12の通常openは版と既知DDLを検査し、旧版を暗黙に更新しない。
`axon migrate --source <v11-db> --output <未使用directory>` はSQLite backup APIでWALを含む
一貫した入力を出力directory内に固定し、新しいSQLiteと対応表を作る。元DBのpathは切り替えない。
v9/v10は旧版でv11にしてから手動変換する。

論理digestは既知tableの列名・SQLite値の型・値を決定的に符号化して計算する。
そのdigestと旧table/keyからstoreとrecordのIDを生成し、元の番号を内部保存順として残す。
元の全field、claim、日時、legacy reason、baseline、metadataと関係を保持する。
出力の全rowと参照・schema・整合性を検査し、manifestを最後に同期して成功を返す。
出力先を上書きせず、失敗時も調査用の途中成果を残す。manifestがない途中成果は利用しない。
具体的な切替は[手動移行](../reference/migration.md)に従う。
