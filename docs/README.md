# ドキュメント

Axon の振る舞いの契約は `reference/` の各文書、状態と遷移のモデルは [spec](../spec/README.md)、実装を読む入口は [層構造の地図](development/architecture.md) です。

利用は [使い始める](guide/getting-started.md) から確認してください。

## 利用者向け

| 文書 | 読む目的 |
| --- | --- |
| [使い始める](guide/getting-started.md) | 試用手順、保存先の作り方、同梱skill |
| [日常の操作](guide/usage.md) | 候補、状態変更、label、Group最終確認、記録参照、計画の一括登録・編集 |
| [状態と用語](guide/concepts.md) | lifecycle、label、関係 |
| [保存先とworktree](guide/storage.md) | 無視する運用と追跡する運用、探索・初期化の境界 |
| [Agentからのアクセス](guide/codex.md) | ホスト権限と保存先 |
| [記録者連携](development/lifecycle-recorder.md) | 自動取得と詳細参照 |

## 契約

| 文書 | 定義する範囲 |
| --- | --- |
| [lifecycle](reference/lifecycle.md) | 状態と遷移、Groupの実効lifecycle、再浮上条件、計画と包含、種類の変換、dependency、候補集合、label、文面・Note・状態変更履歴・記録者 |
| [候補と外部条件](reference/candidates.md) | `axon tasks`の行と状況、`axon proposals|tasks|show`の評価と、条件コマンドの実行 |
| [CLI入出力](reference/cli.md) | ID・引数・一覧と詳細・英語表示・装飾・保存結果 |
| [保存と統合](reference/storage.md) | 記録と現在値の導出、衝突・違反・gap、保存先の探索と初期化、Git統合の範囲 |
| [一括declaration](reference/declaration.md) | 計画全体の取得と一括編集のfile形式、`axon export`・`axon import` |

## 開発者向け

| 文書 | 定義する範囲 |
| --- | --- |
| [モデル](../spec/README.md) | Quintモデルの対象範囲、検証する性質、再現手順 |
| [層構造の地図](development/architecture.md) | crate・module の配置、依存方向、各層のテスト入口 |
| [共通コア](development/lifecycle-core.md) | 通常操作、記録の集合と導出、衝突と解決、codec |
| [Declaration](development/lifecycle-declaration.md) | strict YAML、`axon export`・`axon import`、共通コアとfile書戻しの境界 |
| [CLIと保存の接続](development/lifecycle-cli.md) | 公開操作と保存adapterの接続 |
| [file保存とGit統合](development/lifecycle-file.md) | 初期化と探索、記録fileとcodec、writer、worktree、`axon storage check`・`axon resolve` |
| [記録者連携](development/lifecycle-recorder.md) | 自動取得と保存済み詳細 |
| [検証方針](development/verification.md) | CI、declarationを含む独立fixture、モデル検証の分担 |
| [設計判断](design/decisions.md) | 現在の契約がその形になっている理由と、採らなかった案 |

保存形式は [file adapter](../src/file.rs)、保存先の探索と初期化は [location](../src/location.rs)、Usageは [Clap定義](../src/cli/args.rs) を確認します。

## 更新するとき

- 現在のルールは担当する契約文書で定義する。別の文書で説明するときは要約とリンクにする。
- 契約は現在形で書く。理解に必要な短い理由は契約の近くに残し、長い比較や判断の背景は design に置く。
- 検証方法は development、検証結果には実施時点と対象・条件を記す。
- 未決事項の採否や作業状況は Axon で管理し、docs に現況一覧を複製しない。
- 文書を移動・分割したら、README、AGENTS.md、モデル冒頭などの参照元も更新する。
- 利用者向け文書は日本語で書き、CLIの識別子は実際の表記を併記する。段落内には手動改行を入れず、表示幅による折り返しに任せる。

### Axonの操作・遷移・状態の表記

このリポジトリが書く日本語の文書・skillでは、散文でAxonの操作・遷移・状態を指す語を必ずcode表記にする。code表記でない英単語はAxonの操作を指さない。この読み分けを契約とし、Axonの操作を指さない語は規約の対象外とする。英語のCLI出力・help・内蔵文書には適用しない。

1. コマンドの実行を指すときは、`axon start`・`axon show ID --details`・`axon import prepare …`のように、引用するcode spanを`axon`から始める。同じ列挙では`axon accept|withdraw|cancel|reconsider`のようにまとめてよい。
2. コマンドの一部を単独で指す場合は、段階名の`prepare`・`check`・`apply`、フラグの`--details`・`--version`、引数名の`parent`・`needs`のように、その部分だけをcode表記にする。複数のコマンドに共通し、namespaceを特定できない段階名もこの形にする。
3. コマンドという手段ではなくlifecycleの遷移概念を指すときは、specの遷移名`Accept`・`Withdraw`・`Start`・`Release`・`Complete`・`Cancel`・`Reconsider`・`Reopen`を使う。これらはQuintの型構築子でもあるが、他ツールの識別子としての除外よりこの規則を優先する。
4. 状態は`Undecided`・`NotStarted`・`InProgress`・`Completed`・`Cancelled`のようにcode表記にする。遷移の動詞形`Complete`と、状態の過去分詞形`Completed`を区別する。
5. 一般的な意味での作業の中断・完了・統合・リリースは日本語で書き、Axonのコマンド名・遷移名・状態名と同じ英単語を裸で散文に使わない。ただし、次の対象外に該当する場合を除く。
6. Axon固有の名詞（Issue、Group、Entity、Note、lifecycle、dependency、declaration）、情報モデルのfield名や記録の名詞（actor、log、reason、parent、condition、label）、declarationのfield名（parent、needs、key、base、label）、リポジトリ内のpath・ディレクトリ名、Git・YAML・JSONL・Quint・Rustなど他ツール・他仕様の識別子、一般技術語は対象外。他ツールの識別子は節・表の冒頭または近くの文でツールを示す。その語を主語に操作の挙動を述べる文はコマンド側とみなす。たとえば「`axon condition`は現在の条件だけを編集する」はコマンドの説明、「logのreason」は記録の説明となる。helpも、`axon help`の実行とhelpの出力内容を区別する。
7. skillのdescriptionは冒頭でAxon対象と分かるようにし、Axonの操作を指す場合は上記のコマンド表記にする。
8. 記録の種類名を種類として指すとき（`created`・`transition`・`label`・`import`・`resolve`・`note` など）は、コマンド名と重なるため常にcode表記にする。6のfield名・記録の名詞としての用法（parent、condition、label）はそのまま対象外とする。

declarationはYAMLの一括宣言を指す。タイトルはtitle、本文はdescriptionを指し、両方を指すときは「タイトルと本文」と書く。文面は一般語として使えるが、項目を厳密に指定するときは項目名を列挙する。

一般の作業計画には「計画」やplanを使える。Groupの構成・範囲を指すときは「Groupと全子孫」など対象を明示し、計画の作成とGroup全体の成果の最終確認を区別する。「採用」も一般語として使えるが、方針とEntityの判断を併せて述べるときは「方針の決定」「Issueの採用」のように対象を示す。方針への賛同だけでEntityの採用や実装の権限を得たと解釈しない。

確認時は、`axon --help`の全subcommand、`axon import`・`axon storage`・`axon note`の下位subcommand、状態5語、遷移8語を対象に、fenced code・inline code span・frontmatterのname行を除いた散文をcase-sensitiveかつ単語境界`[A-Za-z-]`で検索する。残存箇所を全て分類し、Axonの操作・遷移・状態を裸で指す箇所がないことを確認する。対象語で始まるcode spanも列挙し、コマンド引用が`axon`から始まることを確認する。検索は発見の補助であり、意味の判定や文脈の確認を置き換えない。
