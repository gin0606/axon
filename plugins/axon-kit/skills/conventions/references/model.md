# モデルと参照

EntityはIssueまたはGroup。同じID namespaceと操作を使う。lifecycleは Undecided / NotStarted / InProgress / Completed / Cancelled の一つ。CompletedとCancelledはterminalだが、明示dependencyを満たすのはCompletedだけ。IDはprefix＋ランダム6文字で、完全IDまたは一意なsuffixを参照できる。保存済みの長いIDも有効。内部のNote・状態記録IDは別の安定識別子で、順序や優先度を表さない。

| 入力 | 遷移 |
| --- | --- |
| capture / group capture | Undecidedを新規作成 |
| plan / group plan | 採用済みのNotStartedを新規作成 |
| accept / withdraw | Undecided→NotStarted / NotStarted→Undecided |
| start / release | NotStarted→InProgress / InProgress→NotStarted |
| done | InProgress→Completed |
| cancel / reconsider | 未終了→Cancelled / Cancelled→Undecided |

同値のlifecycle遷移は拒否される。Completedの再開はない。親は一つのGroupで、子のstartには親がInProgressであることが必要。start/doneには直接dependencyが全員Completedであることが必要。Groupのstartは子を開始せず、releaseはInProgressの子孫がいない場合だけ。Groupのdoneは子のterminal化に加え計画全体の最終確認を表す。循環・終了Groupの構成固定などのguardを状態の往復や別保存経路で迂回しない。取消が子を自動取消することもない。

## 読む目的から入口を選ぶ

`list` は非浮上・terminalを含む保存済み全件で、`--kind issue|group`、`--lifecycle not-started` など、`--terminal=false`、`--search='text'` でAND絞り込みできる。検索はtitle・本文・全Noteのcase-sensitiveなliteral一致。単一検索の不一致だけで意味上の重複なしと断定しない。

`triage` は自身と全祖先が浮上したUndecided、`tasks` は同条件のNotStartedと浮上を問わない全InProgressを示す。親や依存待ちもtasksに入る。候補一覧はinventoryではなく、不在は削除・登録失敗・未着手を立証しない。`--kind`・`--search` は対象候補を先に絞り、残る候補の祖先は通常どおり評価する。

`show ID` は保存本文、Note件数、所属、直接の未充足前提、Groupの直属の子を読む。`show ID --details` は保存lifecycle・条件、親・全直接dependency・直接dependentも取得できる。充足済みの依存を待ち理由の欠如から消えたと判断しない。祖先は親IDを、子孫は直属の子Groupを順に辿り、必要な範囲を取得する。通常showもdetailsも条件を実行しない。

`note list ID` は全Noteの本文・安定ID・日時・actor、`note show ID NOTE_ID` は個別Noteを読む。`log ID` は状態変更・統合の経緯。分岐の記録を時刻で一本の操作列へ並べ直さない。`--recorder-details` で保存済みdataを取得する。`actor` は現在環境の任意actorを表示するだけで、過去の記録者・所有者・今のwriterの終了を立証しない。

## 情報を混同しない

title・本文は現在の定義で、未終了の間はwriteできる。Noteはimmutableな補足でterminal後も追記でき、同内容の別Noteも独立に保持する。logのreasonは状態変更の理由。記録者は環境から取得できた場合だけ付随し、欠如は保存失敗ではない。旧Revision・claim・Disposition別軸は使わない。

再浮上条件は未設定またはshell文字列。未設定は常に成立。when set/clearはterminalにも使え、状態・履歴を変えず条件を評価しない。条件未成立は明示状態操作のguardではない。壊れた条件も保存情報を読みset/clearで修復できる。

条件を評価する前にleaf helpを読む。`/bin/sh -c`、stdin閉鎖、現在のGit worktree root（Git外は管理root）、継承環境で実行する。終了0=成立、1=未成立、他・timeout・signalは一覧全体の失敗。既定30s、`--condition-timeout 500ms|30s|2m|1h`。`--trace-conditions` は実際の評価をstderrへ出す。副作用のあるcommandを単なる読取と扱わない。
