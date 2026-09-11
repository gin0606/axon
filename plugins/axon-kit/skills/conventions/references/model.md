# 状態と情報

EntityはIssueまたはGroupで、lifecycleは Undecided / NotStarted / InProgress / Completed / Cancelled の一つ。CompletedとCancelledは終了。依存が満たされるのは依存先がCompletedの場合だけ。作業者情報は任意の付随情報で、排他・操作権限・生存確認には使わない。

`capture` はUndecided、`plan` はNotStartedを作る。`accept` はUndecided→NotStarted、`withdraw` はNotStarted→Undecided、`start` はNotStarted→InProgress、`release` はInProgress→NotStarted、`done` はInProgress→Completed。`cancel` は未終了→Cancelled、`reconsider` はCancelled→Undecided。Completedは再開しない。各操作の親・依存・子孫・固定構成のguardはCLIが検査する。

子のstartには親GroupがInProgressであることが必要。Groupのstartは子を開始しない。GroupのreleaseはInProgressの子孫がいない場合だけ。Groupのdoneは全子孫が終了し、計画全体を最終確認してから明示する。終了した構成や包含・依存の循環を迂回しない。

再浮上条件は未設定または外部コマンド。`triage` は自身と祖先が浮上したUndecided、`tasks` は浮上したNotStarted（親・依存待ちも含む）と全InProgress。条件の未成立は明示操作の拒否理由にはしない。`list` は全件、`show` は本文と直接の待ち理由で、外部条件を実行しない。

本文は現在の計画、Noteはimmutableな補足、logは状態変更・統合の経緯。`write` は状態を変えずタイトル・本文を更新する。再浮上は `when set ID --command ...` / `when clear ID`。古い記録や同内容Noteも独立に保持する。記録者は自動取得できた範囲だけ添えられ、詳細は `log ID --recorder-details` と `note list ID --recorder-details` で読む。

既存Entityの変更前にshowと判断に関係するlog・Noteを読み、必要な祖先を個別にshowする。終了状態や構造の変更ではlistから関係先を調べ、その本文から直接dependent・祖先・子孫への影響を確認する。候補一覧だけで不存在や着手可能性を決めない。
