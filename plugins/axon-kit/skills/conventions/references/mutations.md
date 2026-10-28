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

終了コード0で、成功出力が対象と要求した作用を示していれば検証済みとする。作成・本文更新・Note追記などの成功通知は、固定した入力の保存を示すもので、入力全項目の出力は求めない。読み直すのは、成功出力に出ない波及（対象・祖先Groupの実効lifecycleなど）を報告や後続判断に使う場合、呼び出し側が保存値の照合を求める場合と、結果不明・出力失敗・想定と異なる出力の場合に限る。後者は下の復旧手順に従う。

`axon write`・`axon label set`の同値や同じparent/dependency/condition・種類の値は `No changes` の成功で履歴を増やさない（同じconditionの`axon condition set`は記録を作らないが、文字列を許可する。次の段落）。ただし終了したEntityへの`axon write`と`axon label set`は同値でも拒否される。同値lifecycleは拒否で、開始済みを新たな`axon start`成功と扱わない。作成とNote追記は非冪等で、再実行すると別IDになる。

`axon condition set --command`と`axon capture --command`は記録の公開後に文字列を許可する。記録の公開が`Result unknown`なら許可は書かれていない。公開後に許可を書けなければ、記録は適用済みのまま `Warning: approval not saved` を出して終了0になり、その条件は許可されるまで実行されない。許可のfileの置換後の同期に失敗すると `Warning: approval result unknown` で終了0になり、許可が効いているかは`axon condition unapproved`で読む。保存値と同じ文字列の`axon condition set`は記録を作らずに許可し、新たに許可すれば`No changes`の後に`Allowed condition`を示す。この経路で許可を書けなければ `Error: approval not saved`、置換後の同期に失敗すれば `Error: approval result unknown` で、どちらも記録は変わらず終了1になる。許可の失敗を記録の失敗として復旧手順に回さず、許可の状態（未許可、または`axon condition unapproved`で読んだ結果）を呼び出し側へ返す。

成功時に`Reason not saved`がなければ、reasonは操作と一体で保存されたとみなし、logでの照合は求めない。同値操作はreasonを指定しても操作・reasonとも記録を増やさず、`No changes`と`Reason not saved; use axon note add to record it.`を示す。その結果を呼び出し側へ返し、自動でNoteへ振り替えない。保存済みreasonの補足・訂正は`axon-kit:add-note`で扱い、理由だけのために操作を繰り返さない。

## 失敗・部分適用・結果不明

保存境界は操作により大文字の `Not applied:`・`Result unknown:` とも、小文字の `not applied: …`・`result unknown after publication …` とも表示される。`Applied:` と `not applied` を取り違えない。`Applied:` または `storage applied; output failed` は保存済み。出力失敗を未適用と解釈して作成・Noteを繰り返さない。`Not applied:` は示された保存段階の未適用で、前段の成功まで否定しない。記録fileのrename後の同期失敗など `Result unknown:` は成功でも未適用でもない。

元processの終了を確認し、同じbinary/rootで現在値・記録を再読する。reasonを渡した操作は、固定した引数・payloadとlogから操作・reason両方の保存状況を照合し、未確認のまま再実行やNote追記をしない。`axon storage check` で保存先の検査も行う。現在値を同じ効果へ収束させる操作は、現在状態と反復契約が合う場合に限り原因を修正して再試行できる。ただし`--command`を含む操作と`axon condition allow`の再試行は許可を書くため、呼び出し側が認めた場合だけ行う。`axon condition allow`は記録を作らないので、この再読と`axon storage check`ではなく、`axon condition unapproved`で許可の状態を読む。lifecycleは対象状態とlogを照合し、別状態へ進んでいれば再送しない。追加操作の照合は [作成](creation.md) または `axon-kit:add-note` の事前集合・固定payloadの手順に従う。

競合・一致候補複数・欠損した事前証拠などで結論できなければ不明として返し、payloadと観測を保持する。記録は追記専用で、結果不明のまま別の作用を重ねると適用済み範囲を確定できなくなるため、補償遷移、取消、rollback、別保存先へ作成で「修復」しない。

lockはOSがwriter終了時に解放する。lock fileの削除は別writerとの相互排他を壊すため行わない。詳細は [保存先と復旧](storage.md)。
