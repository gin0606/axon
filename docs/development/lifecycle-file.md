# 単一 lifecycle の file 保存と Git 統合

[正本spec](../../spec/lifecycle_proposal.md) の保存契約を `src/file.rs`、`src/location.rs`、`src/file_merge.rs` が実装する。通常操作は [SQLite CLI](lifecycle-sqlite.md) と共通で、同じ `Snapshot` の操作・記録・全体検査を通す。旧schemaは自動移行しない。試用・検証は独立fixtureで行う。

## 初期化と探索

`axon init demo --backend file` は現在の管理rootに `.axon/state.jsonl` を新規作成する。Gitでは現在のworktree、Git外では現在directoryを対象とする。Git外で既存管理root内への入れ子initは拒否する。通常探索はGit repositoryで止まり、Git外では最寄りの正本または `init.pending` を持つ祖先で止まる。空の `.axon` とlockだけでは止まらない。

既定backendはSQLite。Git common directoryの親にあるSQLiteと現在worktreeのfileが両方あれば拒否する。他worktreeは走査せず、別worktreeだけにあるfileとの混在は検出を保証しない。一repositoryでbackendを混在させない。破損・未知形式・読取不能・pendingから別保存先へfallbackしない。file正本のないbranchは未初期化であり、既存storeを使うには正本をGitで取り込む。そこでinitすると別のstoreになる。

file initは無関係な行を保持して次を補完する。直接対象を指定する競合設定と、通常fileではない編集先は拒否する。Git内では補完後の実効merge属性も確認し、`.axon/.gitattributes` や Git `info/attributes` で上書きされていれば初期化失敗としてartifactを保持する。親/global ignoreは変更しない。

```gitignore
# .axon/.gitignore
*
!.gitignore
!state.jsonl
```

```gitattributes
# repository root の .gitattributes
/.axon/state.jsonl merge=axon
```

initはbackend共通のOS lock下で存在を確認し、pending markerと同期済みtemporaryを作り、既存正本を上書きせず公開して補助fileを整え、最後にmarkerを除く。途中失敗はartifactを保持する。writerを止め、エラーに表示された正本・temporary・marker・補助fileを保全して確認する。再initによる修復・自動rollbackは行わない。SQLite initはGit補助fileを変更しない。

## 保存の保証

fileの先頭行は `{"format":"axon-file/v1","prefix":"demo"}`。続く行は既存の `axon-lifecycle/v1` 共通codecそのものであり、store ID・状態・全記録を保持する。旧JSONLの互換読取や暗黙変換はしない。

writerは `.axon/state.lock` のOS lockを取得してから最新の正本を読み、通常操作と全体検査を行う。temporaryの書込・sync後、backend・Git index・元bytesを再照合し、atomic replaceとdirectory syncを終えて成功する。lock fileは置換・削除しない。process終了時はOSがlockを解放する。同値の操作では非canonicalな空白も含め元bytesを保持する。

置換前の失敗は `not applied`、置換後のsync失敗は `result unknown` と区別する。結果不明ならprocess終了を確認し、保存済みEntityと記録を照合する。Noteや作成を推測で再実行しない。出力失敗は `storage applied; output failed` で区別する。

通常読取も完全なsnapshotを検査し、Git indexで正本がunmergedなら内容がvalidでも拒否する。Gitがdriverを呼ばないfast-forwardなどでも壊れたsnapshotは受理しない。解決した内容を検査・stageした後に通常操作へ戻る。GitやeditorはOS lockに従わないため、同じworktreeでcheckout/merge/editor保存とAxon書込を並行しない。最終再照合直後の非協調書込やnetwork filesystemの透過的な保証は対象外。

## 明示的な統合

```sh
axon merge prepare --base /tmp/base.jsonl --ours /tmp/ours.jsonl \
  --theirs /tmp/theirs.jsonl --output .axon/state.jsonl --workspace .axon/review
axon merge check .axon/review
axon merge apply .axon/review
axon storage check .axon/state.jsonl
```

workspaceの親directoryを先に用意し、workspace自身は未使用の名前を指定する。outputは現在のfile正本。prepareは入力を保全し、衝突や不正があれば非0で終了するが正本は変更しない。`base.jsonl / ours.jsonl / theirs.jsonl`、出力の元bytesである `preimage`、絶対path・digest・固定記録contextの `manifest.json` は編集しない。

`choices.json` に自動選択と衝突の候補を示す。`resolution.json` の `choices` はEntity IDから `Left`（ours）または `Right`（theirs）へのmapで、現在値の全項目を選ぶ。衝突する全Entityを明示選択する。循環などの全体不整合には、自動選択したEntityも上書き選択できる。Noteと状態記録は両側を保持し、採用結果を通常遷移とは別の統合記録へ残す。

```json
{
  "choices": {"demo-ENTITY_ID": "Right"},
  "reason": "残作業のある分岐を採用する",
  "repairs": []
}
```

`repairs` は選択後のvalidな候補へ順に適用する通常編集。`operation` は `write`（id/title/description）、`parent`（id/parent）、`dependency`（id/needs/present）、`condition`（id/command）で、通常コアの制約に従う。終了構成やCompletedの編集制限を免除しない。構造的に不正な選択をrepairsで救済することはせず、まず選択自体を整える。条件コマンドは実行しない。

checkは全体を検証し、`candidate.jsonl` と入力・解決案・候補を結び付ける `checked.json` を作る。`report.json` にはvalidまたはエラーを残す。これらは編集しない。失敗した再checkは前のcheckedを無効化する。checkを繰り返すと統合記録IDを再生成しうるため、検査済み候補を確認してからapplyする。

applyはworkspaceと正本のlockを取り、元入力・保全コピー・解決案・候補・保存先・backendの変更を拒否する。validな正本はレビュー対象のours/theirsいずれかと一致する必要があり、別storeや入力に含まれない追加作業を上書きしない。Git conflict markerのある正本にも、prepare時の元bytesが変わっていなければ適用できる。Git indexは変更しない。apply後の再実行は保存先の変更として拒否するので、結果不明時は記録を照合する。

## Git driver

利用するbinaryの絶対pathを選び、利用者が設定する。

```sh
git config merge.axon.driver "'/absolute/path/to/axon' merge driver %O %A %B"
git add .axon/state.jsonl .axon/.gitignore .gitattributes
git commit -m 'Track task snapshot'
```

Axon自身はGit config・stage・commitを行わない。driverも同じ `MergePlan` のEntity単位比較と全体検査を使う。成功時だけGitのours temporaryへ公開する。衝突時は非0でoursを保持し、Git indexの三入力を取り出して上記のprepare/check/applyで解決する。検証後に利用者が `git add .axon/state.jsonl` する。fast-forwardでも通常読取の検査は省略しない。

file snapshotには本文に加え取得できた記録者情報が入る。Gitで追跡するとこれらも共有される。記録者の保存項目は [記録者連携](lifecycle-recorder.md) を参照する。

検証入口は `cargo test --lib --bin axon --test smoke`。fileの並行writer、置換前後障害、drift、初期化、実worktreeとdriver、index guard、両backendの同一snapshot保持を独立fixtureで扱う。保存とCLI接続でlifecycleの意味は変更せず、Quintの状態を増やさない。

## Declarationの一括反映

`import apply FILE` は `src/declaration_file.rs::apply` から通常の `Store::update` を使い、OS lock取得後にdeclarationを読み、全件の検証と一回の正本atomic replaceを行う。保存成功後のdeclaration rewriteは同じpublish実装を使う別の保存境界であり、元bytesの再照合、rename、directory syncを行う。保存先Appliedとdeclaration未更新・結果不明を別々に表示する。再試行は全編集Entityの最終値一致なら正本bytesを保持し、保存したsnapshotからbaseと外部参照を更新する。
