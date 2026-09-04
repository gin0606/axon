---
name: work-state
description: 明示されたAxon Entityのstart、release、doneを、申し送り、外部作業の完了条件、完全な波及確認と組み合わせる個人用ワークフロー。実装や対象選択そのものには使わない。
---

# Axonの作業状態を個人用方針で同期する

`axon:conventions`、`axon-kit:work-state`、必要な場合は`axon-kit:add-note`を使う。

## 着手

対象の全情報と全Declaration Revisionを読み、対象自身が`ready`であることを確認する。すでにInProgressならclaimを奪わず、actor、worktree、Note、現在の作業状態から同じ作業を再開できるか確認する。別のEntityを自動的に選ばない。

Groupのstartは子孫をstartしない。新しく現れた`ready`と`triage`のfrontierを報告し、子を自律的に選ばない。

## 解放

releaseは一時中断または引き渡しであり、完了ではない。再開に必要な現在地、残作業、検証状況がある場合は、状態変更reasonと重複しない一件のNoteを先に追加して確認する。GroupではInProgressの子孫が0件であることを確認し、子孫を副作用でreleaseしない。

Note確認後に`axon-kit:work-state`でreleaseする。失敗または結果不明なら同じNoteを再追加せず、Note番号とclaimの現在状態を報告する。

## 完了

Accepted Entityでは宣言された目的が満たされ、所有するワークフローで必要な実装、検証、review、commitが完了し、残作業がない場合だけdoneにする。RejectedまたはUndecidedのInProgress Entityでは、作業を恒久的に止める判断がある場合だけdoneにする。一時中断、失敗したテスト、未解決のreview finding、必要な未commit成果をclaim解消のための完了扱いにしない。

Groupでは全子孫がterminalであることも確認する。外部作業で後から参照すべき結果や検証記録が生じた場合は、定型完了報告ではない簡潔な結果Noteを先に追加して確認する。

`axon:conventions`に従ってdependent、reverse `AfterEntity` waiter、Group ancestry、frontierへの波及を確認してから`axon-kit:work-state`でdoneにする。完了可能になった祖先Groupは報告するだけにし、自動でdoneにしない。

最終Progress、claim、Note番号、検証した波及、完了可能になった祖先Group、未解決事項を報告する。
