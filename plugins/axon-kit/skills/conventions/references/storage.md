# 保存先と復旧

## 探索と初期化

Git repositoryの境界で探索を止める。fileは現在worktreeの `.axon/state.jsonl`、SQLiteはcommon Git directoryの親の `.axon/axon.db`。Git外では最寄りの正本または `.axon/init.pending` を持つ祖先が管理root。空の `.axon` とlockだけでは境界にならない。SQLiteとfileが両方存在すれば混在エラー。unknown schema・破損・読取不能・pendingのとき別rootへfallbackしない。他worktreeは全走査されないため一repositoryでbackendを混在させない。

`axon init [PREFIX]` はSQLiteを、`axon init [PREFIX] --backend file` はfileを新規作成する。prefix省略時は管理root名を使う。既存保存先内の入れ子の`axon init`や既存artifactへの`axon init`の再実行は拒否される。file正本のないbranchで`axon init`すると別storeになる。既存storeを使う意図ならその正本をGitで取り込む必要がある。

file `axon init`は無関係な行を保持して `.axon/.gitignore` に `*`、`!.gitignore`、`!state.jsonl` を、root `.gitattributes` に `/.axon/state.jsonl merge=axon` を補完する。競合する設定、非通常file、補完後の実効merge属性の上書きは拒否される。親/global ignoreは変更しない。SQLite `axon init`はGit補助fileを編集しない。driver設定・stage・commitは別の権限で行う。

`axon init`はbackend共通lock下で存在を検査し、pending marker・同期済みtemporary・正本・補助fileを段階的に保存し、最後にmarkerを取り除く。途中失敗では成果が残る。writerを停止・確認して表示された正本、temporary、pending marker、補助fileを保全し、どこまで適用されたか調べる。`axon init`の再実行・marker削除・既存正本上書きで修復しない。必要な復旧操作が既存権限を超える場合だけ具体的なartifactと案を返す。

## 通常writerと検査

file writerは `.axon/state.lock` のOS lock取得後に最新の正本を読む。通常操作と全体検査後、temporaryへの書込みと同期、backend/index/元bytesの再照合、atomic replace、directory syncの順で保存する。置換前の失敗はnot applied、置換後同期の失敗はresult unknown。lock fileを削除しない。同値操作は元bytesを保持する。

SQLiteはwrite transactionのlock取得後にsnapshotを読み、コアの操作と全体検査後にcommitする。commit errorは結果不明として保存済み状態を照合する。linked worktreeは同じDBを共有する。

`axon storage check SNAPSHOT` は明示fileの完全なsnapshotを読取検査し、条件評価・正本更新・Git index解決をしない。通常読取も完全なsnapshotを検査する。Git indexがunmergedなら内容がvalidでも通常操作を拒否する。解決を検証し呼び出し側の権限でstageした後、通常操作へ戻る。

Git/editorはOS lockに従わないため、同じworktreeでcheckout/merge/editor保存とAxon書込を並行しない。最終再照合直後の非協調書込を透過的に保護する保証はない。

## 統合と結果不明

file統合は `axon-kit:merge-snapshot` とその配布内referenceを使う。通常操作から統合時の選択、backend切替、全data移行へ権限を広げない。公開結果が不明ならwriter終了後に正本、候補、記録を照合する。結果不明のままNote追加や状態変更を繰り返さない。
