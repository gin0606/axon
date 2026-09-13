# Agentからの保存先アクセス

SQLiteはGit worktree間で共有され、current worktree外の `.axon/axon.db` への書込みになる場合があります。選択したbinaryと実保存先を確認し、ホストの権限機構が拒否した操作だけに必要なアクセスを与えてください。別の保存先に切り替えて成功扱いにしません。

利用者が許可した範囲とホストの権限制約は継続します。記録者のactor/sessionは権限やlockを与えません。試用は [初回手順](getting-started.md) の独立保存先で行えます。
