# 保存先とworktree

`axon` はSQLiteとfileを選べます。既定はSQLite、fileは `axon init --backend file` で新規作成します。`axon init [PREFIX]` は新規作成専用で、PREFIXを省略すると管理rootのdirectory名から導出します。既存・混在・破損・初期化途中の保存先を修復するコマンドではありません。prefixの規則は [CLIと表示の契約](../reference/cli.md#識別子と入力) を参照してください。

Git内ではcommon Git directoryの親の `.axon/axon.db` を使い、linked worktree間で共有します。別worktreeを作るだけでは試用データを分離できません。Git外では最寄りの保存先を祖先から探します。試用には既存保存先の外の独立directoryを使ってください。

保存先の混在や未知schemaで別の場所へfallbackしません。初期化途中のartifactは保全し、writerを止めて内容を確認します。SQLiteの詳細は [SQLite CLI](../development/lifecycle-sqlite.md)。fileは現在worktreeの `.axon/state.jsonl` を使い、Gitで取り込むまで他worktreeへ変更を伝えません。初期化・writerの失敗境界・`axon merge`の手順は [file保存とGit統合](../development/lifecycle-file.md)、保存先の判別と統合の契約は [保存と統合の契約](../reference/storage.md) を参照してください。

試用手順は [使い始める](getting-started.md) を参照してください。backend変換や保存先の自動切替はしません。
