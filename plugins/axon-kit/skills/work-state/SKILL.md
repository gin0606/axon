---
name: work-state
description: 指定Entityのstart・release・completeを同期する。実装、対象選択、採用判断は行わない。
---

# 作業状態を同期する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 show --details・logと関係する親・依存を照合する。startはNotStartedから、releaseとcompleteはInProgressから実行する。すでにInProgressなら無条件に開始し直さず、作業の継続意図を呼び出し側へ返す。記録者情報から作業再開や所有権を推測しない。

`start ID` はCLIの親・依存guardに従う。`release ID -r ...` は中断を表し、GroupではInProgress子孫がないことが必要。

`complete ID` は外部workflowの成果・検証が完了したという判断を受けて実行する。Groupでは目的・完了条件と成果の統合、全子孫の終了、必要な検証を独立に確認する。showで子を辿り、必要な本文・Note・logを読む。全子孫のNote一括取得は必須にしない。Groupのcomplete自体を最終確認済みの入力とし、別の確認フラグや状態を作らない。子の終了から親のcompleteへ自動で広げない。

complete前後に直接dependent、祖先・子孫とtasks/proposalsへの影響を確認する。追加Noteは別capabilityとして保存確認してから進める。最終lifecycle、保存結果と波及を返す。
