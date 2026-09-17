---
name: merge-snapshot
description: Axonのfile snapshotを`axon merge prepare`/`axon merge check`/`axon merge apply`で統合する。通常Git操作の完遂やbackend切替には使わない。
---

# file snapshotの統合

`axon-kit:conventions` を使い、指定されたbinaryとfile保存先を固定する。実行前に [統合手順](references/workflow.md) と [保存境界](../conventions/references/storage.md)を読む。入力base/ours/theirsとoutput、workspaceを呼出し側の要求から確定し、保存先や選択を勝手に広げない。

`axon merge prepare --base … --ours … --theirs … --output .axon/state.jsonl --workspace …` は正本を変えず入力を保全する。非0でもworkspaceの保全入力とreportを確認し、準備を成功と誤認しない。

choices/reportを読み、呼出し側の解決判断をresolution.jsonへ反映する。Leftはours、RightはtheirsのEntity全体値。意味上の選択が未確定なら呼出し側へ返す。

`axon merge check WORKSPACE` の成功後に候補とreportを確認し、許可された `axon merge apply WORKSPACE` を単独で実行する。`axon merge apply`は入力と保存先の再照合を行う。適用後は`axon storage check`と記録を照合し、Git indexがunmergedなら通常操作前に検証済み正本のstageが必要と報告する。

結果不明時はwriter終了と保存済み記録を照合し、`axon merge apply`やNoteを推測で繰り返さない。対象、workspace、保存結果、未解決の選択とindex状態を返す。親GroupやEntityのlifecycleを統合の副作用として操作しない。
