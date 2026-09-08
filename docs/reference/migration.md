# Schema 更新と backend 変換

通常コマンドは、対応する schema 更新が必要ならバックアップを保存して更新し、成功後に要求された処理を続ける。利用者は backend 変換を実行する必要がない。
現行の基点は SQLite schema v14、file format 1 / schema 14。v13 の日付型 `AtDate` は通常起動で UTC 午前 0 時の instant へ更新する。移行済みの v9〜v12 の変換経路は保持しない。未知・未来版・退役した形式は変更せず拒否し、自動 downgrade は行わない。

## 通常コマンドによる schema 更新

SQLite は書込排他後に版を再確認し、WAL の確定情報を含む backup を `.axon/migration-backups/` に保存する。schema 更新は一つの transaction で行い、commit 後に元の操作へ進む。途中の失敗では更新を rollback し、backup は保持する。commit 自体の失敗は結果不明として診断する。
file は通常 writer と同じ lock を使い、最新入力から変換・検証した候補と元 bytes の backup を作る。入力・backend・Git index を再照合し、atomic replace と directory sync 後に通常操作へ進む。置換前の失敗は未適用、置換後の同期失敗は結果不明となる。
更新成功の版と backup 先は stderr へ通知する。通常操作が後から失敗しても、成功済み更新は取り消さない。現行形式なら backup や更新通知を追加しない。help、docs、version、completion は保存先を開かない。

各更新経路は入力形式の完全検証、全保存情報の保持、出力形式の完全検証を担当する。file の schema 更新は明示 snapshot 操作の codec へ混ぜず、通常起動だけで実行する。退役形式の復旧が必要なら対応する旧 binary と保全データで一度限りの変換を行う。schema 番号だけの書換えで検査を迂回しない。

Git の過去 commit や branch にある v13 snapshot は書き換えない。merge base、ours、theirs のいずれかが v13 の merge 入力は、混在したまま変換せず拒否する。branch tip を v14 にしても Git が `%O` として渡す共通祖先は v13 のままなので、通常の Git merge は引き続き成立しない。旧 binary で各 snapshot を保全・確認したうえで、隔離した file root で抽出済みの base、ours、theirs をそれぞれ通常起動により v14 へ移行し、その 3 入力を `axon merge prepare --base ... --ours ... --theirs ... --output ... --workspace <unused-dir>` に渡して `check`、`apply` する。完全な操作列と保存契約は [CLI workspace と Git](file-storage.md#cli-workspace-と-git) に従う。代替として履歴を書き換える場合は共同利用者と調整し、全 writer を停止して保全してから行う。`axon-plan/v2` の declaration もメモリ内変換せず拒否するため、現行 store の移行後に `axon export` で v3 を作り直す。

## Backend 変換

```sh
axon migrate --source /path/to/axon.db --output /backup/conversion --backend file
```

入力は現行 schema の SQLite のみ、出力は `sqlite` または `file`。通常の root 探索や自動 schema 更新は行わず、schema 不一致はエラーとする。元 DB を変更せず、出力先も自動で正本へ切り替えない。file 入力は扱わない。

最終切替前に全 writer を止め、元 DB と WAL/SHM、利用 binary を保全する。SQLite backup API で一貫した入力を固定し、未使用 directory に以下を出力する。

| artifact | 内容 |
| --- | --- |
| `source-v14.db` | WAL の確定情報を含む単独で整合した元入力 backup |
| `axon.db` または `state.jsonl` | 指定 backend の正本候補 |
| `snapshot.jsonl` | 全保存情報を持つ canonical snapshot |
| `manifest.yaml` | source/target schema、store ID、件数、digest、検証結果を持つ format 2 manifest |

store/record ID、Entity、関係、Revision、Note、typed history、因果情報と metadata を保持する。番号変換や履歴再生成は行わず、manifest の mappings は空。schema 変換用の staging は作らない。canonical 往復と最終 backend 再読取を検証し、manifest を最後に公開・同期して成功を返す。

成功終了と manifest の digest を照合し、`axon storage check <output>/snapshot.jsonl` でも確認する。隔離コピーで通常の読み取り・更新を検証してよいが、更新したコピーを本番に配置しない。export は計画編集用で全情報 backup の代用ではない。

## 切替と復旧

writer 停止中に元データを退避し、検証済み・未更新の成果を配置する。SQLite は Git common directory の親の `.axon/axon.db`（Git 外は管理 root）、file は現在 worktree の `.axon/state.jsonl`。backend 設定は不要で、競合する正本を同時に置かない。全利用先を確認して writer を再開する。init で空の正本を作り直さない。
file の ignore・attribute・Git driver は[保存契約](file-storage.md)に従って別途整える。migrate は設定・stage・commit を行わない。

出力先は再利用・上書きしない。失敗しても backup と途中成果を保持する。manifest 公開前の失敗は未適用、公開開始後の同期失敗は結果不明。manifest の欠落や digest 不一致がある成果を採用しない。再試行は保全入力と別の未使用 directory を使う。
切替を戻す場合は writer を停止し、現在の全情報を保全してから旧 binary と整合する backup を戻す。新旧 DB の WAL/SHM を混ぜない。書込再開後の新情報を古い backup で失わないよう、引継ぎを決めてから復元する。
