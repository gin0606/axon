# file 保存と Git 統合

[保存と統合の契約](../reference/storage.md) を `src/file.rs`、`src/location.rs`、`src/file_merge.rs` が実装する。通常操作は [CLIと保存の接続](lifecycle-cli.md) と同じ `Snapshot` の操作・記録・全体検査を通す。未知のformatは自動変換しない。試用・検証は独立fixtureで行う。

## 初期化と探索

`axon init demo` は現在の管理rootに正本 `.axon/state.jsonl` だけを新規作成する。Git内では現在のworktree root、Git外では現在directoryを対象とし、Git外で既存管理root内への入れ子の`axon init`は拒否する。

`axon init` が保存先として作るのは `.axon/state.jsonl` だけで、ほかに残すのはOS lock用のfileに限り、既存のfileは編集しない。repositoryの `.gitignore`、`.gitattributes`、Git configを作成も変更もせず、実効merge属性も検査せず、stageもcommitもしない。Git内では初期化した正本がuntrackedに見えることと、無視する運用・追跡する運用それぞれの手順を表示する。無視する運用は、利用者が `.git/info/exclude` やglobalのignore fileに `.axon/` の行を書いて選ぶ。追跡する運用は、`axon init` が表示する次の手順を、利用者がrepository rootで行って選ぶ。

1. 正本だけを追跡対象にする `.axon/.gitignore` を作る。

    ```gitignore
    # .axon/.gitignore
    *
    !.gitignore
    !state.jsonl
    ```

2. repository rootの `.gitattributes` にmerge driverを宣言する。

    ```gitattributes
    # repository root の .gitattributes
    /.axon/state.jsonl merge=axon
    ```

3. [Git driver](#git-driver) の `git config` でdriverを登録する。
4. `.axon/state.jsonl`・`.axon/.gitignore`・`.gitattributes` をstage・commitする。

3の登録が漏れるとGitは正本をtextとして統合する。その結果も次の読み取りの全体検査を受け、conflict markerや不整合を含めば拒否される。離れたEntityへの変更どうしはtextとして統合でき検査も通るため、設定漏れは同じEntityの衝突まで表に出ない。

二つの運用は一つのrepositoryでは混ぜない。無視されている正本はGitのcheckout・mergeの上書き保護を受けず、警告なしに置き換わる。Axonはこの混在を検出しない。理由と境界は [保存と統合の契約](../reference/storage.md#無視する運用と追跡する運用) にある。

探索はGit内では現在のworktree rootの `.axon`、次にmain worktreeの `.axon` の順に見る。2段目はlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ使い、bare repositoryに付けたworktreeとsubmoduleでは使わない。各段は正本または `init.pending` があれば確定し、空の `.axon` とlockだけでは確定せず次へ進む。確定した保存先が破損・読取不能・初期化途中なら停止し、別の保存先へfallbackしない。Git外では最寄りの正本または `init.pending` を持つ祖先で止まる。段の意味と、追跡する運用で正本を持たないbranchのlinked worktreeがmain worktreeの正本を書き換える副作用は [保存と統合の契約](../reference/storage.md#探索) に定める。

`axon init` は2段目を使わず、Git内では現在のworktree rootだけを対象にする。2段目が働く構成のlinked worktreeでの`axon init`は、main worktreeに正本または初期化途中のmarkerがあればそのpathを示して拒否し、なければ作成したうえで他のworktreeからは見えないことを表示する。main worktreeの判定は、common directoryの親で `git rev-parse --is-bare-repository` を含む一回のGit呼出しで行う。bare repositoryなら2段目を使わず、それ以外の理由でGitが失敗した場合は未初期化として扱わずにエラーとする。

OS lock、Git indexのunmerged検査、管理directoryが通常のdirectoryであること（symlinkの拒否）の検査は、確定した保存先とそれを含むworktreeに対して行う。通常writerのlockは確定した保存先の `.axon/state.lock`、`axon init` のlockはGit内ではcommon Git directoryの `axon-init.lock`、Git外では管理directoryの `.axon/axon-init.lock` で、worktreeをまたぐ並行初期化も直列化する。

`axon init`はそのOS lock下で既存の正本を確認し、`init.pending` markerと同期済みtemporaryを作り、既存正本を上書きせず公開してdirectoryを同期し、最後にmarkerを除く。途中失敗はartifactを保持する。writerを止め、エラーに表示された正本・temporary・markerを保全して確認する。`axon init`の再実行による修復・自動rollbackは行わない。

## 保存の保証

fileの先頭行は `{"format":"axon-file/v1","prefix":"demo"}`。続く行は既存の `axon-lifecycle/v1` 共通codecそのものであり、store ID・状態・全記録を保持する。未知formatの読取や暗黙変換はしない。

writerは `.axon/state.lock` のOS lockを取得してから最新の正本を読み、通常操作と全体検査を行う。temporaryの書込・sync後、管理directoryが通常のdirectoryであること・正本が初期化途中でなく存在すること・Git index・元bytesを再照合し、atomic replaceとdirectory syncを終えて成功する。lock fileは置換・削除しない。process終了時はOSがlockを解放する。同値の操作では非canonicalな空白も含め元bytesを保持する。通常の読み取り・書き込みでは保存先の発見はCLI実行ごとに一回で、以後の再照合はこの管理directory・正本・Git index・元bytesを対象とする。実行の途中でGitのtoplevelやcommon directoryが変わったことは検出しない。

置換前の失敗は `not applied`、置換後のsync失敗は `result unknown` と区別する。結果不明ならprocess終了を確認し、保存済みEntityと記録を照合する。Noteや作成を推測で再実行しない。出力失敗は `storage applied; output failed` で区別する。

通常読取も完全なsnapshotを検査し、Git indexで正本がunmergedなら内容がvalidでも拒否する。Gitがdriverを呼ばないfast-forwardなどでも壊れたsnapshotは受理しない。解決した内容を検査・stageした後に通常操作へ戻る。改行を含むpathに置かれたGit worktreeは保存先の発見でエラーにする。GitやeditorはOS lockに従わないため、同じworktreeでcheckout/merge/editor保存とAxon書込を並行しない。最終再照合直後の非協調書込やnetwork filesystemの透過的な保証は対象外。

## 明示的な統合

```sh
axon merge prepare --base /tmp/base.jsonl --ours /tmp/ours.jsonl \
  --theirs /tmp/theirs.jsonl --output .axon/state.jsonl --workspace .axon/review
axon merge check .axon/review
axon merge apply .axon/review
axon storage check .axon/state.jsonl
```

workspaceの親directoryを先に用意し、workspace自身は未使用の名前を指定する。outputは現在のfile正本。`axon merge prepare`は入力を保全し、衝突や不正があれば非0で終了するが正本は変更しない。`base.jsonl / ours.jsonl / theirs.jsonl`、出力の元bytesである `preimage`、絶対path・digest・固定記録contextの `manifest.json` は編集しない。

`choices.json` に自動選択と衝突の候補を示す。`resolution.json` の `choices` はEntity IDから `Left`（ours）または `Right`（theirs）へのmapで、現在値の全項目を選ぶ。衝突する全Entityを明示選択する。循環などの全体不整合には、自動選択したEntityも上書き選択できる。Noteと状態記録は両側を保持し、採用結果を通常遷移とは別の統合記録へ残す。

```json
{
  "choices": {"demo-ENTITY_ID": "Right"},
  "reason": "残作業のある分岐を採用する",
  "repairs": []
}
```

`repairs` は選択後のvalidな候補へ順に適用する通常編集。`operation` は `write`（id/title/description）、`parent`（id/parent）、`dependency`（id/needs/present）、`condition`（id/command）で、通常コアの制約に従う。終了構成や`Completed`の編集制限を免除しない。構造的に不正な選択をrepairsで救済することはせず、まず選択自体を整える。条件コマンドは実行しない。

`axon merge check`は全体を検証し、`candidate.jsonl` と入力・解決案・候補を結び付ける `checked.json` を作る。`report.json` にはvalidまたはエラーを残す。これらは編集しない。失敗した再`axon merge check`は前のcheckedを無効化する。統合記録のIDは記録の内容から決まるため、入力と解決案が同じなら`axon merge check`を繰り返しても同じ候補になる。

`axon merge apply`はworkspaceと正本のlockを取り、元入力・保全コピー・解決案・候補・保存先の変更を拒否する。validな正本はレビュー対象のours/theirsいずれかと一致する必要があり、別storeや入力に含まれない追加作業を上書きしない。Git conflict markerのある正本にも、`axon merge prepare`時の元bytesが変わっていなければ適用できる。Git indexは変更しない。`axon merge apply`後の再実行は保存先の変更として拒否するので、結果不明時は記録を照合する。

## Git driver

driverが働くのは追跡する運用だけで、[初期化と探索](#初期化と探索) の4手順を利用者が行って初めて成立する。`.axon/.gitignore` の作成と `.gitattributes` の1行がなければ正本はGitの追跡対象にならず、driverも呼ばれない。利用するbinaryの絶対pathを選び、利用者が設定する。

```sh
git config merge.axon.driver "'/absolute/path/to/axon' merge driver %O %A %B"
git add .axon/state.jsonl .axon/.gitignore .gitattributes
git commit -m 'Track task snapshot'
```

Axon自身はGit config・stage・commitを行わない。`axon merge driver`も同じ `MergePlan` のEntity単位比較と全体検査を使う。成功時だけGitのours temporaryへ公開する。衝突時は非0でoursを保持し、Git indexの三入力を取り出して上記の`axon merge prepare|check|apply`で解決する。検証後に利用者が `git add .axon/state.jsonl` する。fast-forwardでも通常読取の検査は省略しない。

file snapshotには本文に加え取得できた記録者情報が入る。Gitで追跡するとこれらも共有される。記録者の保存項目は [記録者連携](lifecycle-recorder.md) を参照する。

検証入口は `cargo test --workspace --lib --bin axon --test smoke`。並行writer、置換前後の障害、drift、初期化と探索、実worktreeとdriver、index guardを独立fixtureで扱う。保存とCLI接続でlifecycleの意味は変更せず、Quintの状態を増やさない。

## Declarationの一括反映

`axon import apply FILE` は `src/declaration_file.rs::apply` から通常の `Store::update` を使い、OS lock取得後にdeclarationを読み、全件の検証と一回の正本atomic replaceを行う。保存成功後のdeclaration rewriteは同じpublish実装を使う別の保存境界であり、元bytesの再照合、rename、directory syncを行う。保存先Appliedとdeclaration未更新・結果不明を別々に表示する。再試行は全編集Entityの最終値一致なら正本bytesを保持し、保存したsnapshotからbaseと外部参照を更新する。
