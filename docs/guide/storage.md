# backendとworktree

axonにはSQLiteとfileの二つのbackend（保存方式）があります。SQLite backendは手元でタスクと計画を管理するための基本形です。その管理データもGitに置き、分岐した変更を統合したい場合にfile backendを使います。一つのrepositoryでは一つのbackendを使い、Issue・Groupの操作は共通です。

| backend | データの場所 | worktree間の扱い |
| --- | --- | --- |
| SQLite（既定） | Git common directoryの親の`.axon/axon.db` | 同じDBを参照・更新する |
| file | 現在のworktree rootの`.axon/state.jsonl` | Gitで取り込んだデータを参照・更新する |

Git外ではどちらも管理rootの`.axon/`に保存します。通常の操作は最寄りの管理rootを探します。Git repositoryが探索の境界です。詳しい探索規則は[保存契約](../reference/file-storage.md)を参照してください。

## SQLite backendで計画を共有する

管理したいrepositoryまたはディレクトリで`axon init`を実行します。SQLiteの初期化はGitのignore設定を変えません。DBを追跡したくない場合は`.axon/`を`.gitignore`や`.git/info/exclude`のignore対象にしてください。

メインリポジトリで登録した計画は、そのrepositoryのlinked worktreeからも参照・更新できます。worktreeごとの初期化やデータのコピーは不要です。同じDBを使うので、別のworktreeで取得したclaimも共有されます。

## file backendでIssueをGit管理する

file backendは、Issueや計画をGitで管理し、branchや別PCで分岐した変更を統合するためのbackendです。変更をコードと同じcommitやPRに含められ、統合にはaxonのmerge driverを使います。自動で解決できない競合は、利用者が確認して解決します。

SQLiteのDBもGitで保存・受け渡しはできますが、現在のaxonにはDB同士の変更を統合する仕組みはありません。file backendでは、Issueや履歴・関係を読み取って統合結果を検証します。

新しく管理を始めるrepositoryで、次を実行します。

```sh
axon init --backend file
```

初期化は`.axon/state.jsonl`を作り、`.axon/.gitignore`とrootの`.gitattributes`を用意します。`.gitattributes`にはこのデータにaxonのmerge driverを使う指定が入ります。

`axon`がPATH上にある環境で、Gitのmerge driverを登録します。以下は現在のrepositoryだけに設定する例です。全repositoryで共通に使う場合は、各`git config`に`--global`を付けてください。

```sh
git config merge.axon.name 'Axon validated snapshot merge'
git config merge.axon.driver 'axon merge driver %O %A %B'
git config merge.axon.recursive binary
```

Gitで追跡する対象は`.axon/state.jsonl`、`.axon/.gitignore`、rootの`.gitattributes`です。既存のignore設定が`.axon/`全体を隠している場合は、追跡できるように利用者が調整します。stage・commitは自分やagentが行い、axonが自動で実行することはありません。

保存ファイルには、入力した本文だけでなく操作主体・作業場所の絶対パス・時刻も履歴として残ります。公開前に[共有・公開される情報](../reference/file-storage.md#git-で共有公開される情報)を確認してください。

### 別のworktreeやcloneから使う

既存のIssueを使うworktreeには、保存ファイルをGitで取り込みます。データのないbranchで`init`し直すと別の管理データになるため、既存の計画を共有したいときは、その計画を含むcommitを使ってください。repository単位のGit driver設定はcloneに引き継がれません。clone先でも、repositoryの設定または`--global`の共通設定でdriverを利用できるようにしてください。

file backendでは各worktreeのデータが独立しています。別のworktreeのclaimは即座には共有されず、同じIssueに別々に着手することもできます。作業分担は自分やagentの協業方針で決め、変更はGitで取り込みます。

### mergeで競合したとき

driverは三方向のデータを検証して統合します。自動で解決できない場合は入力と診断を残します。[手動解決の手順](../reference/file-storage.md#cli-workspace-と-git)に沿って結果を確認し、stageやmerge・rebaseの続行を行ってください。

同じworktreeでは、Gitのcheckout・mergeや保存ファイルの直接編集と、axonの書き込みを同時に行わないでください。

Git repository内での利用には`git`が必要です。外部コマンドを再浮上条件にする場合は`/bin/sh`も使います。WSL2ではrepositoryとaxonのデータを`/mnt/c`などではなくLinux filesystem側に置くことを推奨します。

## backendを切り替える

SQLiteで使っていた計画をGitでも管理したくなったら、`migrate`でfile backendへ変換できます。変換元として扱えるのは現行schemaのSQLiteです。

変換結果は別のディレクトリに出力され、元のデータは自動で切り替わりません。結果を確認して配置し、Gitの設定を整えるまでの手順は[backend変換](../reference/migration.md#backend-変換)を参照してください。`init`は新規作成専用なので、切替には使いません。
