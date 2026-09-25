# モデルと参照

EntityはIssueまたはGroup。同じID namespaceを使い、`Start`・`Release`がIssueだけである点と、`Complete`・`Cancel`の遷移元がIssueとGroupで異なる点（下の表）を除いて同じ操作を使う。lifecycleは `Undecided` / `NotStarted` / `InProgress` / `Completed` / `Cancelled` の一つ。`Completed`と`Cancelled`はterminalだが、明示dependencyを満たすのは`Completed`だけ。IDはprefix＋ランダム6文字で、完全IDまたは一意なsuffixを参照できる。保存済みの長いIDも有効。内部のNote・状態記録IDは別の安定識別子で、順序や優先度を表さない。

lifecycleには保存値と実効値がある。Issueの実効値は保存値と同じ。Groupの保存値は`InProgress`にならず、実効値は「保存値が`NotStarted`で、直属の子に実効値が`InProgress`か`Completed`のものがあれば`InProgress`、それ以外は保存値」と導出される。子Groupを通じて上へ伝わるため、孫のIssueの`Start`で祖父のGroupも実効`InProgress`になる。`Cancelled`の子だけでは`InProgress`にならない。以下で「採用済み」は保存値が`NotStarted`であることを指す。

| 入力 | 遷移 |
| --- | --- |
| `axon capture` | `Undecided`を新規作成。種別は `--kind` で選び、省略時はIssue |
| `axon capture --accept` | 採用済みの`NotStarted`を新規作成 |
| `axon accept` / `axon withdraw` | `Undecided`→`NotStarted` / `NotStarted`→`Undecided` |
| `axon start` / `axon release` | Issueだけ。`NotStarted`→`InProgress` / `InProgress`→`NotStarted` |
| `axon complete` | Issueは`InProgress`→`Completed`、Groupは保存値`NotStarted`（実効`InProgress`を含む）→`Completed` |
| `axon cancel` / `axon reconsider` | 未終了（Groupは保存値`Undecided`・`NotStarted`）→`Cancelled` / `Cancelled`→`Undecided` |
| `axon reopen` | `Completed`→`NotStarted` |

同値のlifecycle遷移は拒否される。親は一つのGroup。Groupは`Start`・`Release`を持たず、Groupへの`axon start`・`axon release`は拒否される。Groupの実効値は配下から導出され、配下のlifecycle操作や、着手・完了したEntityの移動で変わる。それによってGroup自身の保存値と履歴は変わらない。

Issueの`Start`には全祖先が採用済みで、自身と全祖先の直接dependencyが全員`Completed`であることが必要。親の実効値が`InProgress`であることは要求しない。`Complete`には全祖先が採用済みで、自身の直接dependencyが全員`Completed`であることが必要。Groupの`Complete`・`Cancel`は直属の子が全員終了していること（直属の子がないGroupと全子`Cancelled`のGroupも満たす）を要し、Groupの`Complete`は子のterminal化に加え計画全体の最終確認を表す。Groupの`Withdraw`は実効値が`NotStarted`のときだけ、着手・完了した子孫を持つGroupの`Accept`は全祖先が採用済みのときだけ行える。実効値が`InProgress`か`Completed`のEntityの移動先は、所属なしか、移動先を含む全祖先が採用済みのGroupに限る。

`Completed`から抜ける経路は`Reopen`だけで、全祖先が採用済みであり、自身を依存先に持つ`Completed`のEntityがないことを要する。`Reopen`は対象だけを`NotStarted`へ戻し、子や依存元を変えない。`Completed`の依存元があれば、依存元から順に`Reopen`する。終了したGroupの配下は構成もlifecycleも固定され、`Reopen`・`Reconsider`を含むlifecycle操作と所属変更は、そのGroupを戻すまで拒否される。条件の編集、`Cancelled`のEntityのdependency編集、Noteの追記は固定されない。終了したGroupの構成を変える正規の手順は、そのGroupを`Reopen`・`Reconsider`で戻すことで、それ以外の状態の往復や別保存経路で循環・構成固定などのguardを迂回しない。取消が子を自動取消することもない。

## 読む目的から入口を選ぶ

`axon list` は非浮上・terminalを含む保存済み全件で、`--kind issue|group`、`--lifecycle not-started` など、`--terminal=false`、`--search='text'` でAND絞り込みできる。`--lifecycle` はGroupの実効値で絞り込むため、保存値`NotStarted`で配下の仕事が始まったGroupは `in-progress` に当たる。検索は現在のtitle・本文だけのcase-sensitiveなliteral一致で、不一致は意味上の重複の不在を示さない。

`axon proposals` は自身と全祖先が浮上した`Undecided`、`axon tasks` は同条件の保存値`NotStarted`と、浮上を問わない`InProgress`のIssue・実効`InProgress`のGroupを平らな一覧で示す。祖先の採用待ちや依存待ちも`axon tasks`に入る。候補一覧はinventoryではなく、不在は削除・登録失敗・未着手を立証しない。`--kind`・`--search` は対象候補を先に絞り、残る候補の祖先は通常どおり評価する。ただし`axon tasks`は、残ったGroupの行の状況を導出するため、絞り込みで除外された子孫の着手可能なIssueとその間のGroupの条件も評価する。実効`InProgress`のGroupも保存値は`NotStarted`なので自身の条件を評価し、判定失敗は一覧全体を失敗させる。

Groupの行の状況は、保存値が`Undecided`・`Completed`・`Cancelled`ならその状態名、保存値が`NotStarted`なら配下から導出され、上から最初に当たるものを示す。`Empty`（直属の子がない。次の一手は計画を書くこと。完了できる場合も`Empty`）、`Confirmable`（最終確認が通れば`axon complete`できる）、`Ready`（着手候補のIssueを子孫に持つ）、`InProgress`（実効値が`InProgress`。完了済みの子孫だけで着手中の子孫がない場合も含む）、`Blocked`（残り）。Group自身は着手対象にならず、`Ready`のGroupでも着手するのは子孫のIssue。`Ready`は、`axon tasks`と既定の`axon show`では再浮上条件を評価した着手候補の子孫から、条件を評価しない`axon list`・`axon show --skip-conditions`では着手可能な子孫から決まる。浮上していない着手可能なIssueだけを配下に持つGroupは、`axon tasks`・`axon show`では`Blocked`、`axon list`・`axon show --skip-conditions`では`Ready`になる。

`axon show ID` は保存本文、Note件数、所属、直接の未充足前提、Groupの全子孫ツリーと終了件数を読む。状況は`axon tasks`の行と同じ範囲（祖先、対象自身、Groupなら配下の着手可能なIssueとその間のGroup）の条件を評価して導出し、条件で浮上していない`NotStarted`のIssueは`Ready`・`Blocked`の代わりに`Unsurfaced`と示す。未充足の前提は、採用済みでない祖先、自身の未完了の依存先、Issueの着手では祖先の未完了の依存先を示す。同じ`Required to start`の節に、条件が未成立の祖先を`Unsurfaced ancestor:`として示すが、これは候補にならない理由であって`Start`の前提ではなく、ID を明示した`axon start`は浮上に関係なく行える。詰まっている保存値`NotStarted`のGroupには`Stalled`の節で、未完了のdependency（自身・祖先・子孫）、`Undecided`の子、未終了の子Group、浮上していない着手可能な子孫（`Unsurfaced candidate:`）、自身の条件が未成立（`Own condition unsatisfied:`、行は当のGroup自身）、`Undecided`の祖先、条件が未成立の祖先（`Unsurfaced ancestor:`）を示す。判定失敗は表示全体の失敗で、診断が示す`axon show ID --skip-conditions`で保存情報を読み、`axon condition set|unset`で修復する。`axon show ID --details` は条件、親・全直接dependency・直接dependentも取得できる。Issueは状況と異なる場合だけ保存lifecycleを`Lifecycle:`に別記する。Groupの`Lifecycle:`は常に実効値で、保存値と異なれば `InProgress (stored NotStarted)` のように保存値を併記する。充足済みの依存を待ち理由の欠如から消えたと判断しない。祖先は親IDを辿り、子孫はツリーから対象IDを選んで必要な本文・Note・logを取得する。`--details`付きでも条件は評価され、条件を実行しないのは`--skip-conditions`を付けたときだけ。操作前の現在値確認や本文の取得には`axon show ID --details --skip-conditions`、浮上していない候補や祖先を含む状況の診断には既定の`axon show ID`を使う。

`axon note search 語句` は終了Entityを含む全Note本文を条件実行なしで横断検索する。case-sensitiveなliteral部分一致でtrim・Unicode正規化はせず、空文字は構文エラー、非一致はstdout空・stderr案内で正常終了する。完全Entity ID・安定Note ID・日時・最初の一致の抜粋を1 Note＝1行で示し、`axon list`と同じ順序とNoteの因果順を保つ。抜粋の省略側は…、改行・バックスラッシュ・制御文字は可視化する。原文は`axon note show ID NOTE_ID`で読む。`axon list`/`axon tasks`/`axon proposals`の検索にはNote本文を含めない。

`axon note list ID` は全Noteの本文・安定ID・日時・actor、`axon note show ID NOTE_ID` は個別Noteを読む。`axon log ID` は状態変更・統合の経緯。分岐の記録を時刻で一本の操作列へ並べ直さない。`--recorder-details` で保存済みdataを取得する。`axon actor` は現在環境の任意actorを表示するだけで、過去の記録者・所有者・今のwriterの終了を立証しない。

## 情報を混同しない

title・本文は現在の定義で、未終了の間は`axon write`で編集できる。Noteはimmutableな補足でterminal後も追記でき、同内容の別Noteも独立に保持する。logのreasonは状態変更の理由。記録者は環境から取得できた場合だけ付随し、欠如は保存失敗ではない。

再浮上条件は未設定またはshell文字列。未設定は常に成立。`axon condition set|unset`はterminalにも使え、状態・履歴を変えず条件を評価しない。条件未成立は明示状態操作のguardではない。壊れた条件も保存情報を読み`axon condition set|unset`で修復できる。

条件を評価する前にleaf helpを読む。`/bin/sh -c`、stdin閉鎖、現在のGit worktree root（Git外は管理root）、継承環境で実行する。終了0=成立、1=未成立、他・timeout・signalは一覧全体の失敗。既定30s、`--condition-timeout 500ms|30s|2m|1h`。`--trace-conditions` は実際の評価をstderrへ出す。副作用のあるcommandを単なる読取と扱わない。
