# SQLite v11から安定IDへの手動移行

v12は保存先とworktree間のSQLite共有を維持し、Note、Revision、判断・進行履歴に安定IDを導入する。
通常openは旧DBを変更せず拒否する。v9/v10は旧版Axonでv11へ移行してから以下を行う。
旧バイナリを新DBへ向けたり、`user_version`だけを変更して利用してはいけない。

1. 旧版と候補版のバイナリを別pathに実体コピーし、版の由来とdigestを残す。
   複数repositoryで同じPATH上のバイナリを使う場合、その全DBを切替対象として棚卸しする。
2. コピーで移行を試し、出力manifestの対応表と全情報保持を確認する。
3. 本切替ではすべてのwriterを止め、停止後の最新DBを入力にする。linked worktreeは同じDBを共有する。
   Axon自身のタスクDBを移す間は、実施台帳をDB外へ置き、旧DBへNote等を追加しない。
4. 固定した新版の絶対pathを使い、未使用directoryへ変換する。

   ```sh
   /absolute/path/to/new-axon migrate --source /root/.axon/axon.db --output /backup/new-conversion
   ```

   出力は`source-v11.db`（WALを含む整合した旧DB）、`axon.db`（v12）、`manifest.yaml`。
   manifestはstore ID、入力の論理digest、ファイルのdigest、旧番号→安定ID対応、件数と検査結果を持つ。
   元DB・本文・legacy reasonを変更しない。既存出力先は拒否し、途中成果も削除しない。
5. 成功終了とmanifest、出力のdigestを確認する。別のGit外の検証rootの`.axon/axon.db`へ
   新DBをコピーし、一覧・Note・Revision・履歴・更新を検証する。Command条件を実行する検査は
   実行内容を確認して行う。宣言exportは全情報backupの代用にならない。
6. writer停止を維持したまま共有バイナリを反映し、ビルド成功と実行ファイルを確認する。
   旧DBとそのWAL/SHMを一組で退避し、新DBを同じ`.axon/axon.db`へ配置する。
   通常openに元のWALを新DBへ適用させない。配置ファイルとdirectoryを同期する。
7. 各利用directoryからPATH上の新版が意図したrootを参照していることを確認し、全DBが
   互換な状態になってから通常利用を再開する。バックアップと旧バイナリを保管する。

失敗・中断では、rootごとに未適用・適用済み・結果不明を記録し、結果不明の変換や切替を
無条件に再実行しない。manifestのない途中成果を正本として使わない。復旧はバイナリとDBと
設定の組を合わせて行う。新DBへの書込再開後に古いbackupへ戻す場合は、新規変更の保全が先になる。
同じ固定入力からの変換は同じIDになるが、別時点の旧DBを独立変換したものは共通起点ではない。
後続の形式移行では、この段階で付けたIDを維持する。
