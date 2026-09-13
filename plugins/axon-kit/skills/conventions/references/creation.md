# 作成と照合

呼び出し側から採否、目的、範囲、完了条件、kindと必要な関係を受け取る。未判断の懸念はcapture、採用済み計画はplanでNotStartedを作る。Groupはgroup capture / group plan。kit自身は重複候補から再利用や採用判断を選ばない。

一つのEntityを作る入力を先に揃える。titleは `--title`、本文は `-m/--description` または `-F/--description-file`。旧位置引数・旧本文optionへfallbackしない。必要な `--parent G`、繰り返せる `--needs B`、供給された初期shell条件 `--command` を同じ作成へ含める。未指定の条件は未設定。日時・Manual・AfterEntityを旧型として渡したり、意図を確認できないshellへ変換したりしない。

最初の作成前にlistの完全ID集合と固定payloadを保持する。作成は非冪等で、タイトル一致による自動upsertではない。成功後は返された完全IDを保存し、show --detailsとlogでkind・本文・lifecycle・親・dependency・条件を照合する。全入力を保存する一回の作成が成功したことを確認してから後続操作へ進む。

結果不明なら元writerの終了後、作成前集合になかったEntityを列挙し、固定した全入力とCreated記録を比較する。時刻、actor、同名だけで自分の操作と決めない。適用を立証できれば再作成せず、複数候補・不十分な証拠なら不明として返す。後続追加をすべて観測でき一致がなく未適用を立証できる場合だけ、同じpayloadで一度再試行する。その試行も照合し、以後は自動再追加しない。
