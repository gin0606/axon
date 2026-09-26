# 保存先と復旧

## 探索と初期化

保存形式は一つで、保存先は管理rootの `.axon/` にある不変な記録の集合。記録1件が `.axon/records/` の下の1 fileで、file名は内容のhashである記録ID。header `.axon/header.json` が保存先の目印になる。現在値は読取のたびに記録から導出される。Git内は現在のrepositoryを探索境界とし、現在のworktree rootの `.axon`、次にmain worktreeの `.axon` の順に選ぶ。2段目はlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ使い、bare repositoryに付けたworktreeとsubmoduleでは使わない。各段はheaderがあれば確定し、headerがなく記録やその他のfile（以前の形式のfileやOSが作るfileを含む）があれば停止する。中断した初期化の残骸（lock、`.tmp` で終わるfile、空の記録のdirectory、CRLFをLFと読んで `axon init` と同じ内容の `.axon/.gitignore`・`.axon/.gitattributes`）だけでは確定せず次へ進む。Git外では最寄りのheaderを持つ祖先が管理rootで、残骸だけの`.axon`は飛ばし、headerがなく記録やその他のfileがある`.axon`では停止する。破損・読取不能のとき別の保存先へfallbackしない。

Git内での使い方は無視する運用と追跡する運用の二つで、利用者のGitの運用だけで決まる。Axonは二つを区別せず、一つのrepositoryではどちらか一つに揃える。無視する運用では利用者が `.git/info/exclude` やglobalのignore fileで `.axon/` を無視させ、linked worktreeはmain worktreeの保存先を共有する。追跡する運用では利用者が `git add .axon` で記録、header、`.axon/.gitignore`、`.axon/.gitattributes` をstage・commitし、各worktreeがcheckoutした自分の保存先を持つ。ほかのGit設定は要らない。追跡する運用で保存先を持たないbranchのlinked worktreeから操作すると、2段目によってmain worktreeの追跡対象の保存先に記録を書く。

Gitはuntrackedなfileをcheckout・mergeの上書きから保護するが、無視されているfileは保護しない。二つの運用を混ぜたrepositoryでは、`.axon/` を追跡しているcommitのcheckout・mergeが、無視されている保存先を警告なしに置き換える。Axonはこの混在を検出しない。追跡する運用でもstageしていない新しい記録fileはuntrackedなので、checkout・reset・stashの後も残り、移った先の記録と並んで読まれる。別のcommitへ移る前に記録をcommitするか、`.axon/records/` のuntracked fileを消すかは呼出し側が決める。

`axon init [PREFIX]` は `.axon/records/`、header、lockと一時fileだけを除外する `.axon/.gitignore`、`* -text` の1行でGitの改行変換を止める `.axon/.gitattributes` を新規作成する。配置や形式を選ぶoptionはない。prefixはASCII小文字・数字・ハイフンだけを許し、省略時は管理rootのdirectory名を小文字化した値を使う。規則に合わなければ保存先を作らずに失敗するので、`axon init PREFIX` で明示する。既存保存先内の入れ子の`axon init`と、中断した初期化の残骸以外のfileがある `.axon` への`axon init`は拒否される。残骸の `.axon/.gitignore`・`.axon/.gitattributes` のうちCRLFを含むものは、`axon init`が書く内容で置き換える。探索の2段目が働くlinked worktreeでの`axon init`は、main worktreeに保存先があれば拒否される。追跡する運用で保存先を持たないbranchをcheckoutしたmain worktreeは未初期化に見えるが、そこで`axon init`すると後で統合できない別のstoreになるため、既存の保存先はGitで取り込む。

`axon init` は初期化のlock（Git内ではcommon Git directoryに残る）を除いて `.axon/` の外に書かず、repositoryの `.gitignore`、`.gitattributes`、Git configを作成も編集もせず、stage・commitもしない。初期化直後の保存先はGitからuntrackedに見え、運用を選ぶまでは `git add -A` でcommitされる。ignore fileの編集、stage・commitは別の権限で行う。

`axon init`はOS lock下で記録のdirectory、`.axon/.gitignore`、`.axon/.gitattributes` を作り、最後にheaderを一時fileからrenameで公開する。header公開前に中断した保存先は未初期化のままで、再実行で作り直せる。失敗時はwriterの終了を確認して `.axon/` を保全し、headerの有無で適用範囲を調べる。header・記録の削除や上書きで修復しない。

## 通常writerと検査

writerは確定した保存先の `.axon/write.lock` のOS lock取得後に記録の集合を読み、前提を検査して新しい記録を一つ作る（`axon import apply` だけは一つのlockの下でEntityごとに一つずつ作り、途中のrename失敗はresult unknownになる）。`<記録ID>.tmp` に書いて同期し、記録IDへrenameしてdirectoryを同期する。既存の記録fileは書き直さず、置き換えず、削除しない。rename前の失敗はnot applied、rename後の同期失敗はresult unknown。同値操作は記録を作らない。記録fileとheaderを手で作成・編集・削除しない。

OS lock、Git indexで `.axon/` の下のpathがunmergedでないことの検査、管理directoryがsymlinkでないことの検査は、確定した保存先とそれを含むworktreeに対して行う。Git内では保存先の確定より前にも、headerのない段ごとに（`.axon` がない段を含む）そのworktreeのindexを検査し、unmergedなら破損・未初期化の判定や次の段へ進まずに停止する。統合が未解決なままheaderが作業treeから消えた保存先を、headerの欠落や未初期化と取り違えず、次の段の保存先へ書かないためである。無視する運用では複数のworktreeが同じ保存先と同じlockを使うため、並行mutationは直列化される。

Git/editorはOS lockに従わないため、同じworktreeでcheckout/merge/editor保存とAxon書込を並行しない。

## 統合と検査

Git統合時にAxonは呼ばれず、両側の記録は別fileとして残る。同じEntityへの両側の操作は衝突（headが複数）、構造の制約に反する組合せは違反、cherry-pick・revert・未commitの記録を残したcheckoutなどで親記録が欠けた記録はgap、`.axon/records/` の下の記録以外のfile（`.tmp` で終わる一時fileを除く）、名前とhashが一致しないfile、headerの欠落・読めないheader・未知のformatは破損として、次の読取と `axon storage check [ROOT]` が報告する。衝突中のEntityがあれば `axon resolve` と `axon note add` 以外の変更が拒否され、破損があれば読取を含む全操作が止まる。違反とgapは通常操作を止めない。通常操作は各前提に加えて違反を増やさないことを要し、違反に含まれるEntityには修復のために一部の固定が免除される。`axon import prepare|check|apply` は違反のある保存先でも拒否される（途中で止まった反映の再試行は `axon-kit:declaration`）。Git indexで `.axon/` の下のpathがunmergedなら内容が有効でも全操作が拒否され、そのpathとindexを持つworktreeを示す診断だけが出る。headerが作業treeになくても破損としては報告されず、そのworktreeでのGitでの解決とstageが要る。追跡する運用のCIは保存先に `axon storage check` を実行する。

衝突と違反の解決は `axon-kit:resolve` を使い、呼び出し側がその範囲を与えていなければ拒否と理由を返す。通常操作の権限から採るheadの選択や修復へ広げない。破損は利用者がfileを直すまで解けず、改行変換が疑われる場合は診断の `Hint:` と `axon docs` の手順を呼出し側へ返す。Axonの状態の取り消しはlifecycle操作で行い、Gitのrevertに頼らない。revertは記録fileを消すだけで、後続の記録があればgapと偽の衝突になる。
