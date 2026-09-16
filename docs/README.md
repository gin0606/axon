# ドキュメント

単一 lifecycle の再構築の正本は [literate spec](../spec/lifecycle_proposal.md)、新しい実装の入口は [共通コア](development/lifecycle-core.md) です。利用ガイドは新lifecycleに対応しています。旧三軸のreference/design・モデルは過去資料として区別します。継続する入出力契約は [現行CLI契約](reference/lifecycle-cli.md) に明示し、過去資料の一括廃止から契約の廃止を推論しません。新 binary と `tests/smoke.rs` は [SQLite CLI](development/lifecycle-sqlite.md) の新仕様を対象にします。`archive/three-axis/src` の旧 module と `archive/three-axis/tests` は過去の三軸実装・検証資料です。

新binaryの利用は [使い始める](guide/getting-started.md) から確認してください。

## 利用者向け

| 文書 | 読む目的 |
| --- | --- |
| [使い始める](guide/getting-started.md) | 新binary選択、独立SQLite試用、手動持込み、同梱skill |
| [日常の操作](guide/usage.md) | 候補、状態変更、Group最終確認、記録参照、計画の一括登録・編集 |
| [状態と用語](guide/concepts.md) | 単一lifecycleと関係 |
| [保存先とworktree](guide/storage.md) | SQLite共有、fileのworktree隔離、探索・初期化の境界 |
| [Agentからのアクセス](guide/codex.md) | ホスト権限と保存先 |
| [記録者連携](development/lifecycle-recorder.md) | 自動取得と詳細参照 |
| [CLI入出力契約](reference/lifecycle-cli.md) | ID・引数・詳細参照・英語表示・装飾・保存結果 |

## 開発者向け

| 文書 | 定義する範囲 |
| --- | --- |
| [正本spec](../spec/lifecycle_proposal.md) | lifecycle・構造・候補・情報・表示・保存・統合・一括declarationの契約とモデル |
| [Declaration](development/lifecycle-declaration.md) | strict YAML、`axon export`・`axon import`、共通コアとfile書戻しの境界 |
| [共通コア](development/lifecycle-core.md) | 通常操作、記録、codec、三者比較 |
| [SQLite CLI](development/lifecycle-sqlite.md) | 公開操作とSQLite adapter |
| [file保存とGit統合](development/lifecycle-file.md) | writer、worktree、`axon merge` CLI・driver |
| [専用移行ツール](../tools/lifecycle-migration/README.md) | 旧file/SQLiteから単一lifecycleへのbackup・変換・検証・明示適用 |
| [候補と外部条件](development/lifecycle-candidates.md) | `axon proposals|tasks`とprocess評価 |
| [記録者連携](development/lifecycle-recorder.md) | 自動取得と保存済み詳細 |
| [検証方針](development/verification.md) | CI、declarationを含む独立fixture、モデル検証の分担 |

保存schemaは [SQLite adapter](../src/sqlite.rs) と [file adapter](../src/file.rs)、Usageは [Clap定義](../src/main.rs) を確認します。

### 設計検討資料

[一括declarationの再設計資料](development/declaration-design-notes.md) は、[正本spec](../spec/lifecycle_proposal.md#計画全体の取得と一括編集) の「計画全体の取得と一括編集」を設計したときの目的・保存境界・協業方針と、旧契約との対応・採らなかった案を残した資料です。契約の定義や通常操作の手順には使いません。

## 過去資料

`reference/` のlifecycle-cli.md以外、`design/`、`development/architecture.md`・`branch-history.md` は置換前の三軸CLIの契約・設計です。`development/audits/` は各文書に記した対象・時点に限定した調査です。現行操作の手順には使いません。旧コード・テストは [archive/three-axis](../archive/three-axis/README.md) に隔離し、Cargo・CIの対象から外しています。旧Quintモデル `axon.qnt`・`group_plan.qnt`・`information_model.qnt`・`branch_history.qnt` も過去資料です。新モデルは正本specから生成します。

## 更新するとき

- 現在のルールは担当する契約文書で定義する。別の文書で説明するときは要約とリンクにする。
- 契約は現在形で書く。理解に必要な短い理由は契約の近くに残し、長い比較や過去の経緯は design に置く。
- 検証方法は development、検証結果には実施時点と対象・条件を記す。
- 未決事項の採否や作業状況は Axon で管理し、docs に現況一覧を複製しない。
- 文書を移動・分割したら、README、AGENTS.md、モデル冒頭などの参照元も更新する。
- 利用者向け文書は日本語で書き、CLIの識別子は実際の表記を併記する。段落内には手動改行を入れず、表示幅による折り返しに任せる。

### Axonの操作・遷移・状態の表記

このリポジトリが書く日本語の文書・skillでは、散文でAxonの操作・遷移・状態を指す語を必ずcode表記にする。code表記でない英単語はAxonの操作を指さない。この読み分けを契約とし、Axonの操作を指さない語は規約の対象外とする。英語のCLI出力・help・内蔵文書には適用しない。

1. コマンドの実行を指すときは、`axon start`・`axon show ID --details`・`axon merge prepare …`のように、引用するcode spanを`axon`から始める。同じ列挙では`axon accept|withdraw|cancel|reconsider`のようにまとめてよい。
2. コマンドの一部を単独で指す場合は、段階名の`prepare`・`check`・`apply`、フラグの`--details`・`--version`、引数名の`parent`・`needs`のように、その部分だけをcode表記にする。複数のコマンドに共通し、namespaceを特定できない段階名もこの形にする。
3. コマンドという手段ではなくlifecycleの遷移概念を指すときは、specの遷移名`Accept`・`Withdraw`・`Start`・`Release`・`Complete`・`Cancel`・`Reconsider`を使う。これらはQuintの型構築子でもあるが、他ツールの識別子としての除外よりこの規則を優先する。
4. 状態は`Undecided`・`NotStarted`・`InProgress`・`Completed`・`Cancelled`のようにcode表記にする。遷移の動詞形`Complete`と、状態の過去分詞形`Completed`を区別する。
5. 一般的な意味での作業の中断・完了・統合・リリースは日本語で書き、Axonのコマンド名・遷移名・状態名と同じ英単語を裸で散文に使わない。ただし、次の対象外に該当する場合を除く。
6. Axon固有の名詞（Issue、Group、Entity、Note、lifecycle、dependency、declaration）、情報モデルのfield名や記録の名詞（actor、log、reason、parent、condition）、declarationや統合のrepairsのoperation種別名（write、parent、dependency、condition）、リポジトリ内のpath・ディレクトリ名、Git・YAML・SQLite・Quint・Rustなど他ツール・他仕様の識別子、一般技術語は対象外。他ツールの識別子は節・表の冒頭または近くの文でツールを示す。その語を主語に操作の挙動を述べる文はコマンド側とみなす。たとえば「`axon condition`は現在の条件だけを編集する」はコマンドの説明、「logのreason」は記録の説明となる。helpも、`axon help`の実行とhelpの出力内容を区別する。
7. skillのdescriptionは冒頭でAxon対象と分かるようにし、Axonの操作を指す場合は上記のコマンド表記にする。

確認時は、`axon --help`の全subcommand、`axon import`・`axon merge`の下位subcommand、状態5語、遷移7語を対象に、fenced code・inline code span・frontmatterのname行を除いた散文をcase-sensitiveかつ単語境界`[A-Za-z-]`で検索する。残存箇所を全て分類し、Axonの操作・遷移・状態を裸で指す箇所がないことを確認する。対象語で始まるcode spanも列挙し、コマンド引用が`axon`から始まることを確認する。検索は発見の補助であり、意味の判定や文脈の確認を置き換えない。

候補の `axon proposals|tasks` と外部コマンドの実行契約は [候補一覧と外部条件](development/lifecycle-candidates.md) を参照する。

[file保存とGit統合](development/lifecycle-file.md) は新lifecycleのwriter・`axon merge` CLI・Git driverの入口です。
