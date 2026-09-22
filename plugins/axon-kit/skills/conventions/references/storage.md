# 保存先と復旧

## 探索と初期化

保存形式は一つで、正本は管理rootの `.axon/state.jsonl`。Git内は現在のrepositoryを探索境界とし、現在のworktree rootの `.axon`、次にmain worktreeの `.axon` の順に選ぶ。2段目はlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ使い、bare repositoryに付けたworktreeとsubmoduleでは使わない。各段は正本または `.axon/init.pending` があれば確定し、空の `.axon` とlockだけでは確定せず次へ進む。Git外では最寄りの正本または `.axon/init.pending` を持つ祖先が管理root。破損・読取不能・初期化途中のとき別の保存先へfallbackしない。

Git内での使い方は無視する運用と追跡する運用の二つで、利用者のGitの運用だけで決まる。Axonは二つを区別せず、一つのrepositoryではどちらか一つに揃える。無視する運用では利用者が `.git/info/exclude` やglobalのignore fileで `.axon/` を無視させ、linked worktreeはmain worktreeの保存先を共有する。追跡する運用では各worktreeがcheckoutした自分の正本を持つ。追跡する運用で正本を持たないbranchのlinked worktreeから操作すると、2段目によってmain worktreeの追跡対象の正本を書き換える。

Gitはuntrackedな正本をcheckout・mergeの上書きから保護するが、無視されている正本は保護しない。二つの運用を混ぜたrepositoryでは、`.axon/state.jsonl` を追跡しているcommitのcheckout・mergeが、無視されている正本を警告なしに置き換える。Axonはこの混在を検出しない。

`axon init [PREFIX]` は正本 `.axon/state.jsonl` だけを新規作成する。配置や形式を選ぶoptionはない。prefixはASCII小文字・数字・ハイフンだけを許し、省略時は管理rootのdirectory名を小文字化した値を使う。規則に合わなければ保存先を作らずに失敗するので、`axon init PREFIX` で明示する。既存保存先内の入れ子の`axon init`や既存artifactへの`axon init`の再実行は拒否される。linked worktreeでの`axon init`は、main worktreeに正本または初期化途中のmarkerがあれば拒否される。

`axon init` が作るのは正本と、Git外で `.axon` に残すOS lock用の `axon-init.lock` だけで、repositoryの `.gitignore`、`.gitattributes`、Git configを作成も編集もせず、実効merge属性も検査せず、stage・commitもしない。初期化直後の正本はGitからuntrackedに見え、運用を選ぶまでは `git add -A` でcommitされる。無視する運用は利用者が `.axon/` を無視させて選ぶ。追跡する運用は、`axon init` が表示する手順（正本だけを追跡対象にする `.axon/.gitignore` の作成、root `.gitattributes` への `/.axon/state.jsonl merge=axon` の追加、merge driverの登録、stage・commit）を利用者が実行して選ぶ。ignore fileの編集、driver設定、stage・commitは別の権限で行う。

`axon init`はOS lock下で存在を検査し、pending marker・同期済みtemporary・正本を段階的に保存し、最後にmarkerを取り除く。途中失敗では成果が残る。writerを停止・確認して表示された正本、temporary、pending markerを保全し、どこまで適用されたか調べる。`axon init`の再実行・marker削除・既存正本上書きで修復しない。必要な復旧操作が既存権限を超える場合だけ具体的なartifactと案を返す。

## 通常writerと検査

writerは確定した保存先の `.axon/state.lock` のOS lock取得後に最新の正本を読む。通常操作と全体検査後、temporaryへの書込みと同期、管理directory・正本の存在・Git index・元bytesの再照合、atomic replace、directory syncの順で保存する。置換前の失敗はnot applied、置換後同期の失敗はresult unknown。同値操作は元bytesを保持する。

OS lock、Git indexのunmerged検査、管理directoryがsymlinkでないことの検査は、確定した保存先とそれを含むworktreeに対して行う。無視する運用では複数のworktreeが同じ正本と同じlockを使うため、並行mutationは直列化される。

`axon storage check SNAPSHOT` は明示fileの完全なsnapshotを読取検査し、条件評価・正本更新・Git index解決をしない。通常読取も完全なsnapshotを検査する。Git indexがunmergedなら内容がvalidでも通常操作を拒否する。解決を検証し呼び出し側の権限でstageした後、通常操作へ戻る。merge driverの登録が漏れてGitがtextとして統合した正本もこの全体検査を受け、conflict markerや不整合を含めば拒否される。離れたEntityへの変更どうしのtext統合は検査を通るため、設定漏れは衝突するまで表に出ない。

Git/editorはOS lockに従わないため、同じworktreeでcheckout/merge/editor保存とAxon書込を並行しない。最終再照合直後の非協調書込を透過的に保護する保証はない。

## 統合と結果不明

分岐した正本の統合は `axon-kit:merge-snapshot` とその配布内referenceを使う。通常操作から統合時の選択や保存先の切替へ権限を広げない。公開結果が不明ならwriter終了後に正本、候補、記録を照合する。
