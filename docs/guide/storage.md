# 保存先とworktree

保存形式は一つです。`axon init [PREFIX]` が管理rootに記録のdirectory `.axon/records/`、header `.axon/header.json`、lockと一時fileだけを除外する `.axon/.gitignore`、Gitの改行変換を止める `.axon/.gitattributes` を新規作成します。作るのはこの四つだけで、配置や形式を選ぶoptionはありません。PREFIXを省略すると管理rootのdirectory名から導出します。既存・破損の保存先を修復するコマンドではありません。prefixの規則は [CLIと表示の契約](../reference/cli.md#識別子と入力) を参照してください。

保存先は不変な記録の集合で、操作のたびに記録が1 file増えます。現在の状態は読取のたびに作業treeにある記録から導出されます。追跡する運用では、commitした記録がそのcommitの状態を表し、`git checkout` で別のcommitに移ればその時点の状態が読めます。ただし未commitの新しい記録fileはuntrackedなので、`git checkout`・`git reset --hard`・`git stash` の後も残り、移った先の記録と並んで読まれます（親の欠けた記録として偽の衝突になることがあります）。別のcommitへ移る前に記録をcommitするか、捨てるなら `git clean` で `.axon/records/` のuntracked fileも消してください。

## 無視する運用と追跡する運用

Gitが保存先を無視するか追跡するかにAxonは関与しません。`axon init` の直後、headerと `.axon/.gitignore`・`.axon/.gitattributes` はGitからuntrackedに見えるため、運用を選ぶまでは `git add -A` で保存先もcommitされます。Git内での使い方は二つあり、どちらになるかは利用者のGitの運用だけで決まります。Axonは二つを区別せず、設定にも持ちません。一つのrepositoryでは、どちらか一つに揃えてください。

無視する運用は、`.git/info/exclude`（そのrepositoryだけ）やglobalのignore fileに `.axon/` の行を書いて、利用者がGitに無視させて選びます。linked worktreeには `.axon` が現れないため、下の探索順によって全worktreeがmain worktreeの保存先を共有します。worktreeごとに `axon init` を繰り返す必要はありません。

追跡する運用は、`git add .axon` で記録、header、`.axon/.gitignore`、`.axon/.gitattributes` をstageしてcommitして選びます。`.axon/.gitignore` がlockと一時fileを除き、`.axon/.gitattributes` が記録fileを改行変換から外すので、ほかに設定は要りません。各worktreeはcheckoutした自分の保存先を持ち、変更はGitで取り込むまで他のworktreeから見えないため、同じIssueに別々に着手できます。両側が記録を追加したbranchは、記録が別fileなのでmergeの属性やGitの設定なしにそのまま統合できます。Axonが契約として扱うのはローカルのGit操作（merge・rebase・cherry-pick・revert・squash）で、ホスティングサービスのweb上のmergeは契約の外です（GitHub上で確かめた結果は [設計判断](../design/decisions.md#記録-1-件-1-file-にした理由)）。同じEntityへの両側の操作はGit上では衝突せず、次の `axon` の読取で衝突として見えます。統合後は `axon storage check` で衝突・違反・記録の欠けを確認し、`axon resolve` と通常操作で直します。検査と解決の契約は [保存と統合の契約](../reference/storage.md#検査と解決の入口) にあります。

Axonの状態の取り消しは `axon reopen`・`axon release`・`axon reconsider` などのlifecycle操作で行い、Gitのrevertに頼らないでください。revertは記録fileを消すだけで、その後に記録が続いていれば親の欠けた記録が残り、状態は戻らずに偽の衝突として見えます。

Gitはuntrackedなfileをcheckout・mergeの上書きから保護しますが、無視されているfileは保護しません。無視されている保存先がある作業directoryで、`.axon/` を追跡しているcommitをcheckout・mergeすると、Gitは警告なしにfileを置き換え、記録が失われます。二つの運用を一つのrepositoryで混ぜなければ起きませんが、Axonはこの混在を検出しません。

`axon init` は `.axon/` の外については手順を表示するだけで、repository rootの `.gitignore`、`.gitattributes`、Git configを作成も編集もせず、stage・commitもしません。

## 改行変換と `.axon/.gitattributes`

記録IDは記録fileのbytes全体のhashなので、Gitの改行変換（repository rootの `* text=auto eol=crlf`、Git for Windowsのsystemの設定にある `core.autocrlf=true` など）がcheckout時にLFをCRLFへ変えると、保存先の破損として読取と全操作が止まります。`axon init` が書く `.axon/.gitattributes` の `* -text` がこの変換を止めます。この属性のない保存先が変換されたときは、読取と `axon storage check` が破損の報告に、改行を戻せば名前と一致するfileであることと、改行変換の可能性とこの節への案内を添えます。この属性は改行変換を止めるだけで、統合には関わりません。防げない設定（repositoryごとの `info/attributes` での `text` 属性の指定、`filter`・`ident`・`working-tree-encoding` の属性など）を含む規則は [保存と統合の契約](../reference/storage.md#保存先と初期化) にあります。

`axon init` は既存の保存先に `.axon/.gitattributes` を足しません。このfileのない保存先を追跡している既存のrepositoryでは、管理rootで次のように足してcommitします。このfileのない保存先を無視する運用から追跡する運用へ移すときは、`.axon/` を無視する設定（`.git/info/exclude` などの行）を外し、`* -text` の1行の `.axon/.gitattributes` を書いてから `git add .axon` で保存先全体をcommitします。ほかのcloneやworktreeも同じく無視する設定を外してから取り込んでください（二つの運用を混ぜた場合の上書きは上に書いたとおりです）。無視していた保存先はGitが取り出していないので、下の手順は要りません。コマンドはPOSIXのshell（WindowsではGit Bash）で実行します。`git check-attr` が `text: unset` を示せば属性は効いています。

```sh
printf '* -text\n' > .axon/.gitattributes
git check-attr text -- .axon/header.json
git add .axon/.gitattributes
git commit -m 'Stop line-ending conversion of the Axon store' -- .axon/.gitattributes
```

最後の行はpathを指定して、このcommitに `.axon/.gitattributes` だけを含めます。変換されたfileは属性を足すと変更として見えることがありますが、`-text` の下では `git add` が改行を戻さないので、`git add .axon`・`git add -A`・`git commit -a` などでstageするとCRLFのままcommitされ（`git stash` やautostashで退避しても変換されたbytesが戻ります）、全cloneで破損になります。

属性を足しても、すでにCRLFで取り出されたfileは書き換わらず、`git status` に変更として見えないこともあります。属性をcommitしたworktreeと、そのcommitを取り込んだ各cloneと各worktreeで、その保存先を使うAxonの操作（保存先を共有するlinked worktreeからの操作を含む）を止めてから管理rootに移り、追跡された `.axon/` のfileを消してindexから取り出し直してください。消してから取り出し直すまでの間は保存先のheaderがなく、Axonは保存先を読めないか、linked worktreeではmain worktreeの保存先を読み書きします。

```sh
git ls-files -z -- .axon | xargs -0 rm --
git checkout -- .axon
```

1行目は追跡されたfile（記録、header、`.axon/.gitignore`、`.axon/.gitattributes`）だけを消し、indexは変えません。2行目がindexの内容で書き直します。untrackedの記録fileは対象にならず、そのまま残ります。取り出し直した後、`git ls-files --eol -- .axon` の各行が `i/lf` と `w/lf` を示すことを確かめます。`i/crlf` があれば変換されたfileがstageされているので、`git restore --staged -- .axon` でindexをcommitの内容に戻してから（`.axon/` のほかのstageした変更も外れます）手順を繰り返します。`w/crlf` が残るなら `.axon/.gitattributes` より優先される設定があるので、契約にある防げない設定を確認します。それでも `i/crlf` が残るなら変換されたfileがcommitされています。そのfileの行末のCRを取り除いて（例: `git ls-files -z -- .axon | xargs -0 perl -pi -e 's/\r\n/\n/'`）`git add .axon` してcommitし、その後で各cloneが同じ確認をします。checkoutしたcommitが `.axon/` を追跡していないworktree（main worktreeの保存先を読むlinked worktree）では不要です。属性のcommitより前に分岐したbranchをcheckoutした場合は、そのbranchに属性のcommitを取り込んでから同じ手順を繰り返します。属性のcommitより前のcommitそのものを読むcloneでは、`git rev-parse --git-path info/attributes` が示すfileに `/.axon/** -text` の行を足すと、そのcloneのcheckoutでは常に変換が止まります。

## どの保存先が選ばれるか

Git内では現在のrepositoryの中だけを探し、次の順で保存先を選びます。

1. 現在のworktree rootの `.axon`。
2. main worktreeの `.axon`。現在のworktreeがlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ探します。bare repositoryに付けたworktreeとsubmoduleでは探しません。

各段では `header.json` があればそこに決まります。Git内でheaderのない段では、まずそのworktreeのGit indexを調べ、`.axon/` の下にunmergedなpathがあれば次へ進まずに停止してunmergedを示します。そうでなければ、中断した初期化の残骸しかない `.axon` では次へ進み、headerがないのに記録やその他のfileがあれば停止します。決まった保存先が破損・読取不能であれば停止し、別の保存先へは切り替えません。Git外では最寄りの保存先を祖先から探します。

追跡する運用では、保存先を持たないbranch（`axon init` より前に分岐したbranchなど）のlinked worktreeから操作すると、2によってmain worktreeの追跡対象の保存先に記録を書きます。変更はmain worktreeの差分として見え、記録は失われないため、この副作用は許容しています。

linked worktreeでの `axon init` は、main worktreeに保存先が既にあれば拒否します。無視する運用では手前に作られた保存先へ読み書きが気づかないまま切り替わり、追跡する運用では別のstoreができて後から統合できなくなるためです。main worktreeに保存先がなければ作成し、他のworktreeからは見えないことを表示します。ただしmain worktreeのGit indexで `.axon/` の下がunmergedなら拒否します。

試用には既存の保存先の外にある独立directoryを使ってください。初期化・writerの失敗境界・保存先の判別と統合の契約は [保存と統合の契約](../reference/storage.md) を参照してください。

試用手順は [使い始める](getting-started.md) を参照してください。
