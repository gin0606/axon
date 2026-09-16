# 保存操作と再試行

## 初回実行前

対象root・binary・引数・payloadを固定し、保存情報を読む。Entityは `axon show ID --details`、判断理由はlog・必要なNote、構造変更やterminal化は親・子孫・直接dependentまで調べる。別writerやGit/editor操作との未調整競合があれば先に直列化または分離する。

各mutationは単独のshell呼出しにし、その終了コードを個別確認する。後続commandの成功で失敗を隠さない。複数段階の状態・本文・関係変更は一つのtransactionではない。順序、各段階のpostconditionと適用済み範囲を保持する。

本文の `-m/--description` と `-F/--file`、Noteの `-m/--message` と `-F/--file` はそれぞれ排他で、`-F -` はstdin。file/stdinは初回mutation前に正確なUTF-8 bytesを独立snapshotへ保存しdigestを記録する。結果不明の間は保持し、元fileの後の編集を再試行へ混入させない。コマンドに先頭hyphenを含むoption値は `--message='--text'` のように渡す。shellの補間で内容を変えない。

## 保存先と権限

Git内のSQLiteはcommon Git directoryの親の `.axon/axon.db` をworktree間で共有する。fileは現在worktreeの `.axon/state.jsonl` で、他worktreeの未統合データは観測できない。Git外は最寄りの管理root。別worktreeはSQLiteの独立fixtureではない。混在・破損・unknown schema・`axon init`途中から別保存先へfallbackしない。

意図した保存先への当該mutationだけがsandboxに拒否された場合は、そのcommandだけをホストの許可機構へ渡す。無関係な読み取りやprogram、別binary、backend切替まで許可範囲を広げない。変更fileをstage/commitする権限は呼び出し側が別に与える。

## 成功とno-op

終了0と完全IDの確認文を読み、`axon show`・`axon show --details`・log・個別Noteで要求した作用を検証する。`axon write`の同値や同じparent/dependency/conditionの値は `No changes` の成功で履歴を増やさない。同値lifecycleは拒否で、開始済みを新たな`axon start`成功と扱わない。作成とNote追記は非冪等で、再実行すると別IDになる。

## 失敗・部分適用・結果不明

`Applied:` または `storage applied; output failed` は保存済み。出力失敗を未適用と解釈して作成・Noteを繰り返さない。`Not applied:` は示された保存段階の未適用で、前段の成功まで否定しない。SQLite commit失敗、file置換後の同期失敗など `Result unknown:` は成功でも未適用でもない。

元processの終了を確認し、同じbackend/rootで現在値・記録を再読する。fileは正本の完全な検査も行う。現在値を同じ効果へ収束させる操作は、現在状態と反復契約が合う場合に限り原因を修正して再試行できる。lifecycleは対象状態とlogを照合し、別状態へ進んでいれば再送しない。追加操作の照合は [作成](creation.md) または `axon-kit:add-note` の事前集合・固定payloadの手順に従う。

競合・一致候補複数・欠損した事前証拠などで結論できなければ不明として返し、payloadと観測を保持する。補償遷移、取消、Note削除、rollback、別保存先へ作成で「修復」しない。既に保存確認したNoteは後続状態操作の失敗後も再追加しない。

fileのlockはOSがwriter終了時に解放する。lock fileの削除は別writerとの相互排他を壊すため行わない。同一worktreeでGit/editor書込とAxon書込を並行しない。詳細は [保存先と復旧](storage.md)。
