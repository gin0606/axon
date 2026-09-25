---
name: work-state
description: Axonの指定Issueの`axon start`・`axon release`と、指定Issue・Groupの`axon complete`を同期する。実装、対象選択、採用判断は行わない。
---

# 作業状態を同期する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 `axon show --details`・logと関係する祖先・依存を照合する。`axon start`と`axon release`はIssueだけに使い、`axon start`は`NotStarted`から、`axon release`は`InProgress`から実行する。Issueの`axon complete`は`InProgress`から、Groupの`axon complete`は保存値`NotStarted`から実行する。Issueがすでに`InProgress`なら無条件に開始し直さず、作業の継続意図を呼び出し側へ返す。記録者情報から作業再開や所有権を推測しない。

Groupは`Start`・`Release`を持たない。Groupの実効値は配下から導出され、配下の変化でGroup自身の保存値は変わらない。Issueの着手のために祖先Groupを着手させる操作はない。`axon start ID` はCLIの祖先・依存guardに従い、全祖先が採用済みで、自身と全祖先の依存先が`Completed`であることを要する。`Undecided`の祖先や未完了の依存先で拒否されたら、採用判断や前提の完了を呼び出し側へ返す。`axon release ID -r ...` は中断を表す。

`axon complete ID` は外部workflowの成果・検証が完了したという判断を受けて実行する。Groupでは目的・完了条件と成果の統合、全子孫の終了、必要な検証を独立に確認する。Groupの`axon complete`は実効値が`InProgress`でも行え、直属の子が全員終了し、全祖先が採用済みで、自身の依存先が`Completed`であることを要する。直属の子がないGroupも同じ最終確認を経て完了する。`axon show`で子を辿り、必要な本文・Note・logを読む。全子孫のNote一括取得は必須にしない。Groupの`axon complete`自体を最終確認済みの入力とし、別の確認フラグや状態を作らない。子の終了から親の`axon complete`へ自動で広げない。

`Completed`から`NotStarted`へ戻す`axon reopen`はその判断を要し、このcapabilityでは扱わず`axon-kit:triage`が適用する。

`axon start`・`axon release`・`axon complete`は祖先Groupの実効lifecycleと状況を変えうる。前後に直接dependent、祖先・子孫と`axon tasks`/`axon proposals`への影響を確認する。追加Noteは別capabilityとして保存確認してから進める。最終lifecycle、保存結果と波及を返す。
