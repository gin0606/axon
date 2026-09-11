# 保存操作

各mutationは単独のshell呼出しにし、その終了コードを個別確認する。後続commandの成功で隠さない。指定対象とpayloadを固定し、操作後にshow・log・note listなどでpostconditionを確認する。

新CLIのSQLiteはGit common directoryの親の `.axon/axon.db` をworktree間で共有する。Git外は最寄りの保存先を使う。別worktreeは独立fixtureではない。混在・破損・未知schema・init途中を別保存先へのfallbackで回避しない。権限拒否は意図した保存先への当該commandだけをホストの許可機構で扱う。

file/stdinの入力は初回mutation前に正確なbytesをsnapshotとして保存しdigestを記録する。結果不明の間は保持する。通常の失敗は原因を修正してから、現在値と操作の反復契約で再試行の安全性を確認する。

出力に `storage applied; output failed` があれば保存は適用済み。元processの終了を確認し保存結果を再読する。結果が不明なら成功扱いにせず、重複の可能性がある登録・Noteを繰り返さない。現在値や記録の照合で未適用と確定できた場合だけ安全な再試行へ進む。

新旧binary・実保存先の切替、一括取り込み、stage・commitはこの契約から許可されない。初期化は新規作成のみ。旧データは旧binaryで保全・参照し、新保存先へ必要な計画を手動で登録する。

fileは現在worktreeの `.axon/state.jsonl` を変更し、Gitで取り込むまで他worktreeへ反映しない。未解決indexは通常操作を拒否する。`not applied` と置換後の `result unknown` を区別し、後者はwriter終了後に正本と記録を照合する。lockを削除して回避せず、同一worktreeでGit/editor保存とAxon書込を並行しない。詳細は [file保存](../../../../../docs/development/lifecycle-file.md)。
