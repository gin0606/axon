# 作成と照合

呼び出し側から採否、目的、範囲、完了条件、kind、labelと必要な関係を受け取る。登録は`axon capture`の一つで、採否は `--accept` の有無、kindは `--kind issue|group`、labelは必須の `--label` で指定する。未判断の懸念は `--accept` なしで`Undecided`、採用済み計画は `--accept` 付きで`NotStarted`を作る。`--kind` は省略するとissueになるため、供給されたkindを毎回明示する。kindが供給されていなければ既定に頼らず呼び出し側へ返す。labelは固定集合（[モデルと参照](model.md#情報を混同しない)）の値で、供給されていなければ推測せず呼び出し側へ返す。登録後の種類の変換は `axon convert ID --kind issue|group` で、保存値が`Undecided`・`NotStarted`のEntityだけに行え、子を持つGroupと`InProgress`のIssueは変換できない（`axon release`するかは呼び出し側の判断）。lifecycle・所属・dependency・文面・label・条件・Noteは変わらないので、変換後の本文がその種類の役割に合うかを確かめる。変換は呼び出し側が種類を与えた場合だけ行う。

一つのEntityを作る入力を先に揃える。titleは `--title`、供給されたkindは `--kind`、labelは `--label`、本文は `-m/--description` または `-F/--file`。採否に応じた `--accept`、必要な `--parent G`、繰り返せる `--needs B`、供給された初期shell条件 `--command` を同じ作成へ含める。初期条件は未設定またはshell文字列として扱い、未指定なら未設定にする。条件の意味や実行影響が未確定なshellへの変換は、呼び出し側へ返す。

最初の作成前に`axon list`の完全ID集合と固定payloadを保持する。作成は非冪等で、タイトル一致による自動upsertではない。成功後は返された完全IDを保存し、`axon show ID --details --skip-conditions`とlogでkind・label・本文・lifecycle・親・dependency・条件を照合する。全入力を保存する一回の作成が成功したことを確認してから後続操作へ進む。

結果不明なら元writerの終了後、作成前集合になかったEntityを列挙し、固定した全入力とCreated記録を比較する。時刻、actor、同名だけで自分の操作と決めない。適用を立証できれば再作成せず、複数候補・不十分な証拠なら不明として返す。後続追加をすべて観測でき一致がなく未適用を立証できる場合に限り、同じpayloadで一度だけ再試行し、その結果も同じ手順で照合する。
