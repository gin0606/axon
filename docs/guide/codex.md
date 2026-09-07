# Codexでのアクセス設定に必要な情報

axonの操作がsandboxに阻まれる場合は、このページをCodexに読んでもらい、使っている環境に合う設定を相談してください。ここでは、設定方法を判断するためのaxon側の情報をまとめます。

## 実際の保存先

SQLite backendでは、Git common directoryの親にある`.axon/axon.db`をlinked worktree間で共有します。作業中のworktreeの外に保存先があることが、アクセスを拒否される原因になり得ます。

Git common directoryの場所は、作業中のrepositoryで次のコマンドから確認できます。

```sh
git rev-parse --path-format=absolute --git-common-dir
```

返されたディレクトリの親にある`.axon/`が共有データの保存先です。DB本体に加えて、ロックやSQLiteのjournal・WAL、保存形式の更新時のバックアップなども扱うため、必要な書き込み先をDBファイル一つだけと考えないでください。

file backendでは、現在のworktree rootの`.axon/state.jsonl`を使います。書き込み時には`.axon/`内のロックや一時ファイルも扱います。初期化時には`.axon/.gitignore`とrootの`.gitattributes`も作成・更新します。

初期化時は、Git common directory内の`axon-init.lock`も使います。

Git外では、どちらも管理rootの`.axon/`を使います。詳しい保存先の規則は[backendとworktree](storage.md)と[保存契約](../reference/file-storage.md)を参照してください。

## 実行するもの

Codexの実行環境から`axon`を呼び出せる必要があります。Git repository内の操作では`git`も使います。axon自体はGitのstage・commit・pushを行いません。

コマンドを再浮上条件に設定している場合は、一覧などの読み取り操作でも、その外部コマンドを実行することがあります。`/bin/sh -c`を使い、現在のworktree root（Git外では管理root）で、axonの起動元の環境変数を引き継いで実行します。スクリプトや認証、ネットワーク、cacheexecのキャッシュ先など、必要なアクセスは設定したコマンドによって異なります。

外部コマンドを実行せずに保存状態を確認したい場合は、`axon show <id> --skip-command-evaluation`や`axon list --skip-command-evaluation`を使えます。ただし、対応する保存形式の自動更新は通常の読み取りコマンドでも起こり得ます。詳細は[CLI契約](../reference/cli.md#外部条件の評価)と[保存形式の更新](../reference/migration.md#通常コマンドによる-schema-更新)を参照してください。

## Codexに伝えること

使っているbackend、作業中のworktree、実際の保存先、失敗したコマンドとエラーを伝えると、必要なアクセスを確認できます。コマンド条件を使っている場合は、その内容も併せて伝えてください。
