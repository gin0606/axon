# 保存先とworktree

保存形式は一つです。`axon init [PREFIX]` が管理rootに記録のdirectory `.axon/records/`、header `.axon/header.json`、lockと一時fileだけを除外する `.axon/.gitignore` を新規作成します。作るのはこの三つだけで、配置や形式を選ぶoptionはありません。PREFIXを省略すると管理rootのdirectory名から導出します。既存・破損の保存先を修復するコマンドではありません。prefixの規則は [CLIと表示の契約](../reference/cli.md#識別子と入力) を参照してください。

保存先は不変な記録の集合で、操作のたびに記録が1 file増えます。現在の状態は読取のたびに作業treeにある記録から導出されます。追跡する運用では、commitした記録がそのcommitの状態を表し、`git checkout` で別のcommitに移ればその時点の状態が読めます。ただし未commitの新しい記録fileはuntrackedなので、`git checkout`・`git reset --hard`・`git stash` の後も残り、移った先の記録と並んで読まれます（親の欠けた記録として偽の衝突になることがあります）。別のcommitへ移る前に記録をcommitするか、捨てるなら `git clean` で `.axon/records/` のuntracked fileも消してください。

## 無視する運用と追跡する運用

Gitが保存先をどう扱うかにAxonは関与しません。`axon init` の直後、headerと `.axon/.gitignore` はGitからuntrackedに見えるため、運用を選ぶまでは `git add -A` で保存先もcommitされます。Git内での使い方は二つあり、どちらになるかは利用者のGitの運用だけで決まります。Axonは二つを区別せず、設定にも持ちません。一つのrepositoryでは、どちらか一つに揃えてください。

無視する運用は、`.git/info/exclude`（そのrepositoryだけ）やglobalのignore fileに `.axon/` の行を書いて、利用者がGitに無視させて選びます。linked worktreeには `.axon` が現れないため、下の探索順によって全worktreeがmain worktreeの保存先を共有します。worktreeごとに `axon init` を繰り返す必要はありません。

追跡する運用は、`git add .axon` で記録とheaderをstageしてcommitして選びます。`.axon/.gitignore` がlockと一時fileを除くので、ほかに設定は要りません。各worktreeはcheckoutした自分の保存先を持ち、変更はGitで取り込むまで他のworktreeから見えないため、同じIssueに別々に着手できます。両側が記録を追加したbranchは、記録が別fileなのでGitの属性や設定なしにそのまま統合できます。Axonが契約として扱うのはローカルのGit操作（merge・rebase・cherry-pick・revert・squash）で、ホスティングサービスのweb上のmergeは契約の外ですが、両側が記録を追加したPRをGitHub上でそのままmergeできることは確認しています（2026-09-25、[設計判断](../design/decisions.md#記録-1-件-1-file-にした理由)）。同じEntityへの両側の操作はGit上では衝突せず、次の `axon` の読取で衝突として見えます。統合後は `axon storage check` で衝突・違反・記録の欠けを確認し、`axon resolve` と通常操作で直します。手順は [file保存とGit統合](../development/lifecycle-file.md#git-統合と検査) にあります。

Axonの状態の取り消しは `axon reopen`・`axon release`・`axon reconsider` などのlifecycle操作で行い、Gitのrevertに頼らないでください。revertは記録fileを消すだけで、その後に記録が続いていれば親の欠けた記録が残り、状態は戻らずに偽の衝突として見えます。

Gitはuntrackedなfileをcheckout・mergeの上書きから保護しますが、無視されているfileは保護しません。無視されている保存先がある作業directoryで、`.axon/` を追跡しているcommitをcheckout・mergeすると、Gitは警告なしにfileを置き換え、記録が失われます。二つの運用を一つのrepositoryで混ぜなければ起きませんが、Axonはこの混在を検出しません。

`axon init` は手順を表示するだけで、repositoryの `.gitignore`、`.gitattributes`、Git configを作成も編集もせず、stage・commitもしません。

## どの保存先が選ばれるか

Git内では現在のrepositoryの中だけを探し、次の順で保存先を選びます。

1. 現在のworktree rootの `.axon`。
2. main worktreeの `.axon`。現在のworktreeがlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ探します。bare repositoryに付けたworktreeとsubmoduleでは探しません。

各段では `header.json` があればそこに決まり、中断した初期化の残骸しかない `.axon` では次へ進みます。headerがないのに記録や以前の形式のfileがあれば停止します。決まった保存先が破損・読取不能であれば停止し、別の保存先へは切り替えません。Git外では最寄りの保存先を祖先から探します。

追跡する運用では、保存先を持たないbranch（`axon init` より前に分岐したbranchなど）のlinked worktreeから操作すると、2によってmain worktreeの追跡対象の保存先に記録を書きます。変更はmain worktreeの差分として見え、記録は失われないため、この副作用は許容しています。

linked worktreeでの `axon init` は、main worktreeに保存先が既にあれば拒否します。無視する運用では手前に作られた保存先へ読み書きが気づかないまま切り替わり、追跡する運用では別のstoreができて後から統合できなくなるためです。main worktreeに保存先がなければ作成し、他のworktreeからは見えないことを表示します。

試用には既存の保存先の外にある独立directoryを使ってください。初期化・writerの失敗境界・統合の検査と解決の手順は [file保存とGit統合](../development/lifecycle-file.md)、保存先の判別と統合の契約は [保存と統合の契約](../reference/storage.md) を参照してください。

試用手順は [使い始める](getting-started.md) を参照してください。
