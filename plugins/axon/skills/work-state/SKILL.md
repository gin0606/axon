---
name: work-state
description: 指定Entityのstart・release・completeを外部作業の完了条件や申し送りと同期する。
---

# 指定した作業の状態を同期する

`axon:conventions` と `axon-kit:work-state` を使う。対象と作業範囲は依頼または明示workflowが与える。show --details・logと必要なNote、祖先・依存を読み、lifecycleと外部作業の現在地を照合する。記録者だけで前workerの終了や引継ぎを推測しない。

startは指定したNotStartedに限る。中断・引継ぎは確定した現在地と残作業をNoteへ記録・確認してからreleaseする。継続と恒久終了を区別する。

completeは目的・完了条件、実装、必要な検証・reviewが客観的に満たされ、残作業がない場合に実行する。Groupは全子孫の終了だけでは不十分で、目的・成果の統合・検証を計画全体として確認し、明示completeを最終確認済みの入力にする。全子孫Noteの一括読込は初回必須ではない。親Groupを自動完了せず、その操作の権限を確認する。

確定結果Noteの保存、最終lifecycle、dependent・祖先と候補一覧への影響を照合する。実装・commitなどの権限は呼び出し元workflowの範囲に従う。
