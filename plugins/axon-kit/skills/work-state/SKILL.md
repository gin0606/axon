---
name: work-state
description: Axonの指定Entityの`axon start`・`axon release`・`axon complete`を同期する。実装、対象選択、採用判断は行わない。
---

# 作業状態を同期する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 `axon show --details`・logと関係する親・依存を照合する。`axon start`は`NotStarted`から、`axon release`と`axon complete`は`InProgress`から実行する。すでに`InProgress`なら無条件に開始し直さず、作業の継続意図を呼び出し側へ返す。記録者情報から作業再開や所有権を推測しない。

`axon start ID` はCLIの親・依存guardに従う。`axon release ID -r ...` は中断を表し、Groupでは`InProgress`子孫がないことが必要。

`axon complete ID` は外部workflowの成果・検証が完了したという判断を受けて実行する。Groupでは目的・完了条件と成果の統合、全子孫の終了、必要な検証を独立に確認する。`axon show`で子を辿り、必要な本文・Note・logを読む。全子孫のNote一括取得は必須にしない。Groupの`axon complete`自体を最終確認済みの入力とし、別の確認フラグや状態を作らない。子の終了から親の`axon complete`へ自動で広げない。

`axon complete`前後に直接dependent、祖先・子孫と`axon tasks`/`axon proposals`への影響を確認する。追加Noteは別capabilityとして保存確認してから進める。最終lifecycle、保存結果と波及を返す。
