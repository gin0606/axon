---
name: merge-snapshot
description: Axonのfile snapshotをprepare/check/applyで統合する。通常Git操作の完遂やbackend切替には使わない。
---

# file snapshotの統合

`axon-kit:conventions` を使い、指定されたbinaryとfile保存先を固定する。実行前に [統合手順](references/workflow.md) と [保存境界](../conventions/references/storage.md)を読む。入力base/ours/theirsとoutput、workspaceを呼出し側の要求から確定し、保存先や選択を勝手に広げない。

`merge prepare --base … --ours … --theirs … --output .axon/state.jsonl --workspace …` は正本を変えず入力を保全する。非0でもworkspaceの保全入力とreportを確認し、準備を成功と誤認しない。保全入力、manifest、preimageは編集せず、同じ未使用workspaceへの盲目的な再試行をしない。

choices/reportを読み、呼出し側の解決判断をresolution.jsonへ反映する。Leftはours、RightはtheirsのEntity全体値。意味上の選択が未確定なら呼出し側へ返す。通常修正の制約、全記録保持、終了Groupの構成を免除しない。

`merge check WORKSPACE` の成功後に候補とreportを確認し、許可された `merge apply WORKSPACE` を単独で実行する。applyは入力と保存先の再照合を行う。driftを強制上書きで回避せず、新しい入力を保全して判断をやり直す。適用後はstorage checkと記録を照合し、Git indexがunmergedなら通常操作前に検証済み正本のstageが必要と報告する。stage・commit・Git設定はこのskillから許可されない。

結果不明時はwriter終了と保存済み記録を照合し、applyやNoteを推測で繰り返さない。対象、workspace、保存結果、未解決の選択とindex状態を返す。親GroupやEntityのlifecycleを統合の副作用として操作しない。
