# 新規登録

未判断の懸念にはcapture、目的と完了条件が確定し採用された計画にはplanを使う。Groupではgroup capture / group plan。kindと対象は呼び出し側が決める。

`--title` と本文 `-m` または `-F`、必要な `--parent G`、繰り返せる `--needs B` を初期入力とする。保存後に返されたIDをshowし、lifecycle、本文、関係を照合する。作成は非idempotent。出力失敗時は作成前後のlistとshow/logを比較し、不確かなまま再登録しない。
