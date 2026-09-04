---
name: work-state
description: 指定されたAxon Entityのstart、release、doneを、claim、申し送り、外部作業の完了条件、波及確認と組み合わせる個人用ワークフロー。実装や無権限の対象選択には使わない。
---

# Axonの作業状態を個人用方針で同期する

`axon:conventions`、`axon-kit:work-state`、必要な場合は`axon-kit:add-note`を使う。

## 対象を確定する

対象はユーザーが直接指定するか、明示的な上位workflowが与えた範囲内で選ばれていなければならない。このskill自身は`ready`から着手対象を選ばない。

対象の全情報と関係するDeclaration Revisionを読み、操作可能な状態を確認する。すでにInProgressで現在のactorとworktreeに整合するclaimがあれば、再度startせず再開可能な状態として返す。別のactorまたはworktreeが保持するclaimは奪わない。

## 着手する

指定されたEntityが`ready`なら`axon-kit:work-state`でstartする。Groupのstartは子孫をstartしない。新しく現れた`ready`と`triage`のfrontierを報告するが、上位workflowから権限を与えられていない子Entityは選ばない。

## 解放する

releaseは一時中断または引き渡しであり、完了ではない。明示された保留または引き継ぎでは、必要な現在地、残作業、検証状況をreasonと重複しないNoteへまとめ、保存を確認してからreleaseする。継続、一時停止、恒久停止のどれかが不明なら確認する。

GroupはInProgressの子孫が0件の場合だけreleaseする。子孫が残る場合は、その扱いをユーザーまたは呼び出し元workflowへ返し、副作用でreleaseしない。

## 完了する

Accepted Entityでは、declarationと外部作業が定める実装、検証、review、commitなどの条件が客観的に満たされ、残作業がない場合は自律してdoneにできる。完了条件が主観的、未定義、または一部未達なら判断を返す。

RejectedまたはUndecidedのInProgress Entityは、作業を恒久的に止める明示的な判断がある場合だけdoneにする。一時中断や未解決成果をclaim解消のために完了扱いしない。

Groupをdoneにするのは、そのGroup自身の完了が指示され、全子孫がterminalの場合に限る。子孫が完了しただけで祖先Groupを自動的にdoneにしない。完了可能になった祖先Groupは呼び出し元へ返し、上位workflowが明示的に完了を所有している場合だけ、そのworkflowから改めてdoneする。

後から参照すべき確定済みの結果や検証記録がある場合は、定型報告ではない簡潔なNoteを先に追加する。`axon:conventions`に従ってdependent、reverse `AfterEntity` waiter、Group ancestry、frontierへの波及を確認してから`axon-kit:work-state`を実行する。

安全に照合できる再試行は自律して行い、競合、結果不明、別の最終状態が必要な場合は停止する。最終Progress、claim、Note番号、検証した波及、完了可能になった祖先Group、未解決事項を報告する。
