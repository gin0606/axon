# 保存先とworktree

新CLIの初回運用はSQLiteです。`init [prefix]` は新規作成専用で、既定prefixはaxon。既存・混在・破損・初期化途中の保存先を修復するコマンドではありません。

Git内ではcommon Git directoryの親の `.axon/axon.db` を使い、linked worktree間で共有します。別worktreeを作るだけでは試用データを分離できません。Git外では最寄りの保存先を祖先から探します。試用には既存保存先の外の独立directoryを使ってください。

保存先の混在や未知schemaで別の場所へfallbackしません。初期化途中のartifactは保全し、writerを止めて内容を確認します。SQLiteの詳細は [SQLite CLI](../development/lifecycle-sqlite.md)。file保存とGit統合は [正本spec](../../spec/lifecycle_proposal.md) に設計がありますが、このSQLite入口ではまだ利用できません。

新binaryの選択と旧データの手動持込みは [使い始める](getting-started.md) を参照してください。backend変換や実保存先の自動切替はしません。
