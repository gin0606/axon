# 保存操作と再試行

## 初回実行前

対象root・binary・引数・payloadを固定し、保存情報を読む。Entityは `axon show ID --details --skip-conditions`、判断理由はlog・必要なNote、構造変更やterminal化は親・子孫・直接dependentまで調べる。別writerやGit/editor操作との未調整競合があれば先に直列化または分離する。

各mutationは単独のshell呼出しにし、その終了コードを個別確認する。後続commandの成功で失敗を隠さない。複数段階の状態・本文・関係変更は一つのtransactionではない。順序、各段階のpostconditionと適用済み範囲を保持する。

本文の `-m/--description` と `-F/--file`、Noteの `-m/--message` と `-F/--file` はそれぞれ排他で、`-F -` はstdin。file/stdinは初回mutation前に正確なUTF-8 bytesを独立snapshotへ保存しdigestを記録する。結果不明の間は保持し、元fileの後の編集を再試行へ混入させない。コマンドに先頭hyphenを含むoption値は `--message='--text'` のように渡す。shellの補間で内容を変えない。

## reasonを渡す

`axon accept|withdraw|start|release|complete|cancel|reconsider|reopen`、`axon write`、`axon label set`、`axon parent set|unset`、`axon dep add|rm`、`axon condition set|unset`、`axon convert`、`axon resolve ID --head RECORD_ID`は任意の`-r/--reason`を受け付ける。呼び出し側から渡されたreasonを対応する操作に添え、渡されていなければ理由なしで操作する。kitでは理由を作ったり入力を要求したりしない。

reasonは同値判定より先に検証される。一行で、空白だけの値・改行・制御文字・Unicodeの文字単位で500文字を超える値は拒否される。登録（採用済みを含む）とNote追加にはreason入力がなく、必要な情報はそれぞれの本文へ渡す。`axon import apply`とdeclarationも理由入力の対象外であり、reasonのために別の変更操作を追加しない。

## 保存先と権限

保存先は探索で確定した管理rootの `.axon/`（記録のdirectory、header、lock）。無視する運用では現在のworktree外にあるmain worktreeの保存先をworktree間で共有し、追跡する運用では現在のworktreeの保存先（保存先を持たないbranchのlinked worktreeではmain worktreeの保存先）を使い、他worktreeの未統合の記録は観測できない。Git外は最寄りの管理root。別worktreeは独立fixtureではない。

意図した保存先への当該mutationだけがsandboxに拒否された場合は、そのcommandだけをホストの許可機構へ渡す。無関係な読み取りやprogram、別binary、別の保存先への切替まで許可範囲を広げない。変更fileをstage/commitする権限は呼び出し側が別に与える。

## 成功とno-op

終了0と完全IDの確認文を読み、`axon show ID --details --skip-conditions`・log・個別Noteで要求した作用を検証する。`axon write`・`axon label set`の同値や同じparent/dependency/condition・種類の値は `No changes` の成功で履歴を増やさない。ただし終了したEntityへの`axon write`と`axon label set`は同値でも拒否される。同値lifecycleは拒否で、開始済みを新たな`axon start`成功と扱わない。作成とNote追記は非冪等で、再実行すると別IDになる。

reasonを渡した場合は、現在値に加えて`axon log ID`で対応する操作とreasonの保存を照合する。現在値の一致だけでは理由の保存を立証できない。同値操作はreasonを指定しても操作・reasonとも記録を増やさず、`No changes`と`Reason not saved; use axon note add to record it.`を示す。その結果を呼び出し側へ返し、自動でNoteへ振り替えない。保存済みreasonの補足・訂正は`axon-kit:add-note`で扱い、理由だけのために操作を繰り返さない。

## 失敗・部分適用・結果不明

保存境界は操作により大文字の `Not applied:`・`Result unknown:` とも、小文字の `not applied: …`・`result unknown after publication …` とも表示される。`Applied:` と `not applied` を取り違えない。`Applied:` または `storage applied; output failed` は保存済み。出力失敗を未適用と解釈して作成・Noteを繰り返さない。`Not applied:` は示された保存段階の未適用で、前段の成功まで否定しない。記録fileのrename後の同期失敗など `Result unknown:` は成功でも未適用でもない。

元processの終了を確認し、同じbinary/rootで現在値・記録を再読する。reasonを渡した操作は、固定した引数・payloadとlogから操作・reason両方の保存状況を照合し、未確認のまま再実行やNote追記をしない。`axon storage check` で保存先の検査も行う。現在値を同じ効果へ収束させる操作は、現在状態と反復契約が合う場合に限り原因を修正して再試行できる。lifecycleは対象状態とlogを照合し、別状態へ進んでいれば再送しない。追加操作の照合は [作成](creation.md) または `axon-kit:add-note` の事前集合・固定payloadの手順に従う。

競合・一致候補複数・欠損した事前証拠などで結論できなければ不明として返し、payloadと観測を保持する。記録は追記専用で、結果不明のまま別の作用を重ねると適用済み範囲を確定できなくなるため、補償遷移、取消、rollback、別保存先へ作成で「修復」しない。

lockはOSがwriter終了時に解放する。lock fileの削除は別writerとの相互排他を壊すため行わない。詳細は [保存先と復旧](storage.md)。
