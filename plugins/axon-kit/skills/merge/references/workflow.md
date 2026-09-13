# file snapshotの統合手順

## 入力を固定する

完全なbase/ours/theirs、現在のfile正本output、未使用workspaceを確定する。workspaceの親directoryを先に用意する。`merge prepare --base B --ours O --theirs T --output OUTPUT --workspace WORKSPACE` を単独で実行する。

非0でも保全入力やreportが残りうるためworkspaceを調べる。同じworkspace名で盲目的に再prepareしない。保全されたbase.jsonl・ours.jsonl・theirs.jsonl、output元bytesのpreimage、絶対path・digest・固定recorder contextを持つmanifest.jsonは編集しない。

## 選択と修正

choices.jsonで自動選択と衝突を読む。resolution.jsonだけを編集し、choicesをEntity IDからLeft（ours）またはRight（theirs）へのmapにする。選択対象はEntityの現在値全体で、baseは比較材料。最新timestampやtitleだけで採用側を選ばない。呼び出し側の明示判断または合意済み効果から選択が一意に決まる場合だけ反映する。意味上の採否・scope・完了の選択が未確定なら、その衝突と具体案を返す。

両側のNoteと状態履歴は保持され、現在値の採用は通常遷移と異なる統合記録に残る。衝突Entityをすべて選び、候補全体の循環や固定構成を検査する。自動選択Entityも必要なら明示選択できる。構造的に不正な候補をrepairsで救済することはできない。

repairsはvalidな選択結果への通常編集で、operationはwrite（id/title/description）、parent（id/parent）、dependency（id/needs/present）、condition（id/command）。与えられた効果の範囲に限る。旧workflowの状態遷移・start・Note追加repairは提供されない。固定構成、Completedの本文固定、記録追記専用性を迂回しない。条件は実行しない。

## 検査して公開する

resolution変更のたびに `merge check WORKSPACE` を単独実行する。成功したcandidate.jsonl、report.json、checked.jsonを確認する。これらは編集しない。失敗したcheckは以前のcheckedを無効化する。check再実行は統合記録IDを再生成しうるため、レビューした候補を不用意に再生成しない。

公開直前に固定入力、解決案、候補、backend、store identity、保存先がその検査と一致することを確かめ、許可された `merge apply WORKSPACE` を実行する。apply自体もworkspaceと正本をlockしてdriftを拒否する。正本がvalidならours/theirsいずれかに一致する必要がある。conflict markerがある場合もprepare時の元bytesから変わっていてはいけない。driftを手動上書きで回避せず、新入力と判断で別workspaceを用意する。

apply後は `storage check OUTPUT` で完全性を検証し、index解決後に影響Entityと記録を照合する。applyの再実行は保存先変更として拒否され、一般的なno-op再送ではない。結果不明時はwriter終了後にoutput・保全candidate・digestと記録を照合する。入力やdestinationを変更して再送可能に見せかけない。

## Git conflict

driverは `%O %A %B` を読み、成功時だけGitのours temporaryへ公開する。失敗時はoursを保持する。Git stage 1/2/3の完全なsnapshotを保全し、実際の `.axon/state.jsonl` をoutputとする明示workspaceで解決する。Git temporaryを正本と取り違えない。

検査済み正本をstageする権限は呼び出し側が与える。このkitはgit config、stage、commit、merge/rebase続行、abortを許可しない。indexがunmergedの間は通常Axon操作が引き続き拒否されることを報告する。

不足した入力・意味上の選択、照合不能なdrift、不明な公開結果、進展のないcheckでは、保全artifactと適用済み範囲、次に必要な観測または判断を返す。
