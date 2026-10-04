---
name: work-state
description: Axonの指定Issueの`axon start`・`axon release`と、指定Issue・Groupの`axon complete`を外部作業の完了条件や申し送りと同期する。
---

# 指定した作業の状態を同期する

`axon:conventions` と `axon-kit:work-state` を使う。対象と作業範囲は依頼または明示workflowが与える。`axon show ID --details --skip-conditions`と関係する祖先・依存を読み、`axon:conventions`の「本文を入口に読む」に従ってlifecycleと外部作業の現在地を照合する。

`axon start`・`axon release`は指定したIssueに限り、`axon start`は`NotStarted`のIssueに使う。Groupの扱いと着手の前提は`axon-kit:work-state`に従い、着手できなければ不足する採用判断や前提を呼び出し元へ返す。中断・引継ぎは確定した現在地と残作業を記録し、`axon:conventions`に従ってNoteの保存と必要な本文への反映を確認してから`axon release`する。継続と恒久終了を区別する。

`axon complete`は目的・完了条件、実装、必要な検証・reviewが客観的に満たされ、残作業がない場合に実行する。直属の子のないGroupも最終確認を省かない。Groupは全子孫の終了だけでは不十分で、Groupの目的と全子孫の成果の統合・検証を確認し、明示的な`axon complete`を最終確認済みの入力にする。親Groupの`axon complete`は、子の終了とは別にその操作の権限が与えられている場合だけ実行する。

`Completed`を`NotStarted`へ戻す`axon reopen`は、その判断が与えられた場合だけ`axon:update`で扱う。

確定結果Noteの保存、最終lifecycle、dependent・祖先と候補一覧への影響を照合する。実装・commitなどの権限は呼び出し元workflowの範囲に従う。
