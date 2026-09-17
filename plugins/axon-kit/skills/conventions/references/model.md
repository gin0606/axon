# モデルと参照

EntityはIssueまたはGroup。同じID namespaceと操作を使う。lifecycleは `Undecided` / `NotStarted` / `InProgress` / `Completed` / `Cancelled` の一つ。`Completed`と`Cancelled`はterminalだが、明示dependencyを満たすのは`Completed`だけ。IDはprefix＋ランダム6文字で、完全IDまたは一意なsuffixを参照できる。保存済みの長いIDも有効。内部のNote・状態記録IDは別の安定識別子で、順序や優先度を表さない。

| 入力 | 遷移 |
| --- | --- |
| `axon capture` | `Undecided`を新規作成。種別は `--kind` で選び、省略時はIssue |
| `axon capture --accept` | 採用済みの`NotStarted`を新規作成 |
| `axon accept` / `axon withdraw` | `Undecided`→`NotStarted` / `NotStarted`→`Undecided` |
| `axon start` / `axon release` | `NotStarted`→`InProgress` / `InProgress`→`NotStarted` |
| `axon complete` | `InProgress`→`Completed` |
| `axon cancel` / `axon reconsider` | 未終了→`Cancelled` / `Cancelled`→`Undecided` |

同値のlifecycle遷移は拒否される。`Completed`の再開はない。親は一つのGroupで、子の`Start`には親が`InProgress`であることが必要。`Start`/`Complete`には直接dependencyが全員`Completed`であることが必要。Groupの`Start`は子を開始せず、`Release`は`InProgress`の子孫がいない場合だけ。Groupの`Complete`は子のterminal化に加え計画全体の最終確認を表す。循環・終了Groupの構成固定などのguardを状態の往復や別保存経路で迂回しない。取消が子を自動取消することもない。

## 読む目的から入口を選ぶ

`axon list` は非浮上・terminalを含む保存済み全件で、`--kind issue|group`、`--lifecycle not-started` など、`--terminal=false`、`--search='text'` でAND絞り込みできる。検索は現在のtitle・本文だけのcase-sensitiveなliteral一致で、不一致は意味上の重複の不在を示さない。

`axon proposals` は自身と全祖先が浮上した`Undecided`、`axon tasks` は同条件の`NotStarted`と浮上を問わない全`InProgress`を示す。親や依存待ちも`axon tasks`に入る。候補一覧はinventoryではなく、不在は削除・登録失敗・未着手を立証しない。`--kind`・`--search` は対象候補を先に絞り、残る候補の祖先は通常どおり評価する。

`axon show ID` は保存本文、Note件数、所属、直接の未充足前提、Groupの全子孫ツリーと終了件数を読む。`axon show ID --details` は保存lifecycle・条件、親・全直接dependency・直接dependentも取得できる。充足済みの依存を待ち理由の欠如から消えたと判断しない。祖先は親IDを辿り、子孫はツリーから対象IDを選んで必要な本文・Note・logを取得する。通常の`axon show`も`--details`付きも条件を実行しない。

`axon note search 語句` は終了Entityを含む全Note本文を条件実行なしで横断検索する。case-sensitiveなliteral部分一致でtrim・Unicode正規化はせず、空文字は構文エラー、非一致はstdout空・stderr案内で正常終了する。完全Entity ID・安定Note ID・日時・最初の一致の抜粋を1 Note＝1行で示し、`axon list`と同じ順序とNoteの因果順を保つ。抜粋の省略側は…、改行・バックスラッシュ・制御文字は可視化する。原文は`axon note show ID NOTE_ID`で読む。`axon list`/`axon tasks`/`axon proposals`の検索にはNote本文を含めない。

`axon note list ID` は全Noteの本文・安定ID・日時・actor、`axon note show ID NOTE_ID` は個別Noteを読む。`axon log ID` は状態変更・統合の経緯。分岐の記録を時刻で一本の操作列へ並べ直さない。`--recorder-details` で保存済みdataを取得する。`axon actor` は現在環境の任意actorを表示するだけで、過去の記録者・所有者・今のwriterの終了を立証しない。

## 情報を混同しない

title・本文は現在の定義で、未終了の間は`axon write`で編集できる。Noteはimmutableな補足でterminal後も追記でき、同内容の別Noteも独立に保持する。logのreasonは状態変更の理由。記録者は環境から取得できた場合だけ付随し、欠如は保存失敗ではない。

再浮上条件は未設定またはshell文字列。未設定は常に成立。`axon condition set|unset`はterminalにも使え、状態・履歴を変えず条件を評価しない。条件未成立は明示状態操作のguardではない。壊れた条件も保存情報を読み`axon condition set|unset`で修復できる。

条件を評価する前にleaf helpを読む。`/bin/sh -c`、stdin閉鎖、現在のGit worktree root（Git外は管理root）、継承環境で実行する。終了0=成立、1=未成立、他・timeout・signalは一覧全体の失敗。既定30s、`--condition-timeout 500ms|30s|2m|1h`。`--trace-conditions` は実際の評価をstderrへ出す。副作用のあるcommandを単なる読取と扱わない。
