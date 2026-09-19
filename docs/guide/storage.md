# 保存先とworktree

保存形式は一つです。`axon init [PREFIX]` が管理rootに正本 `.axon/state.jsonl` を新規作成します。作るのはこのfileだけで、配置や形式を選ぶoptionはありません。PREFIXを省略すると管理rootのdirectory名から導出します。既存・破損・初期化途中の保存先を修復するコマンドではありません。prefixの規則は [CLIと表示の契約](../reference/cli.md#識別子と入力) を参照してください。

## 無視する運用と追跡する運用

Gitが保存先をどう扱うかにAxonは関与しません。`axon init` の直後、正本はGitからuntrackedに見えるため、運用を選ぶまでは `git add -A` で保存先もcommitされます。Git内での使い方は二つあり、どちらになるかは利用者のGitの運用だけで決まります。Axonは二つを区別せず、設定にも持ちません。一つのrepositoryでは、どちらか一つに揃えてください。

無視する運用は、`.git/info/exclude`（そのrepositoryだけ）やglobalのignore fileに `.axon/` の行を書いて、利用者がGitに無視させて選びます。linked worktreeには `.axon` が現れないため、下の探索順によって全worktreeがmain worktreeの保存先を共有します。worktreeごとに `axon init` を繰り返す必要はありません。

追跡する運用は、`axon init` が表示する手順を利用者が実行して選びます。`*`・`!.gitignore`・`!state.jsonl` の3行を持つ `.axon/.gitignore` を作って正本だけを追跡対象にし、repository rootの `.gitattributes` に `/.axon/state.jsonl merge=axon` を書き、Git merge driverを登録し、これらをstage・commitします。各worktreeはcheckoutした自分の正本を持ち、変更はGitで取り込むまで他のworktreeから見えないため、同じIssueに別々に着手できます。分岐した正本の統合手順は [file保存とGit統合](../development/lifecycle-file.md#明示的な統合) にあります。

Gitはuntrackedなfileをcheckout・mergeの上書きから保護しますが、無視されているfileは保護しません。無視されている正本がある作業directoryで、`.axon/state.jsonl` を追跡しているcommitをcheckout・mergeすると、Gitは警告なしに正本を置き換え、記録が失われます。二つの運用を一つのrepositoryで混ぜなければ起きませんが、Axonはこの混在を検出しません。

`axon init` は手順を表示するだけで、repositoryの `.gitignore`、`.gitattributes`、Git configを作成も編集もせず、実効merge属性も検査せず、stage・commitもしません。merge driverの登録が漏れるとGitは正本をtextとして統合します。その結果も次の読み取りで全体検査を受けるので、conflict markerや不整合を含む正本がそのまま使われることはありません。ただし離れたEntityへの変更どうしはtextとして統合でき検査も通るため、設定漏れは同じEntityを両側で変更して衝突するまで気づけません。追跡する運用を始めるときは `git check-attr merge -- .axon/state.jsonl` と `git config merge.axon.driver` で設定を確認してください。

## どの保存先が選ばれるか

Git内では現在のrepositoryの中だけを探し、次の順で保存先を選びます。

1. 現在のworktree rootの `.axon`。
2. main worktreeの `.axon`。現在のworktreeがlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ探します。bare repositoryに付けたworktreeとsubmoduleでは探しません。

各段では正本または初期化途中のmarkerがあればそこに決まり、空の `.axon` やlockだけでは次へ進みます。決まった保存先が破損・読取不能・初期化途中であれば停止し、別の保存先へは切り替えません。Git外では最寄りの保存先を祖先から探します。

追跡する運用では、正本を持たないbranch（`axon init` より前に分岐したbranchなど）のlinked worktreeから操作すると、2によってmain worktreeの追跡対象の正本を書き換えます。変更はmain worktreeの差分として見え、記録は失われないため、この副作用は許容しています。

linked worktreeでの `axon init` は、main worktreeに保存先または初期化途中のmarkerが既にあれば拒否します。無視する運用では手前に作られた保存先へ読み書きが気づかないまま切り替わり、追跡する運用では別のstoreができて後から統合できなくなるためです。main worktreeに保存先がなければ作成し、他のworktreeからは見えないことを表示します。

試用には既存の保存先の外にある独立directoryを使ってください。初期化途中のartifactは保全し、writerを止めて内容を確認します。初期化・writerの失敗境界・`axon merge`の手順は [file保存とGit統合](../development/lifecycle-file.md)、保存先の判別と統合の契約は [保存と統合の契約](../reference/storage.md) を参照してください。

試用手順は [使い始める](getting-started.md) を参照してください。
