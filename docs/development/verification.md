# 検証方針とモデル検査

## モデルの位置付けと保守方針

`spec/axon.qnt` は必須の基礎状態モデルとして維持する。その対象となる A / B / C / D の意味や満足条件を変える場合は、モデルを更新・検証する。

`spec/group_plan.qnt` と `spec/information_model.qnt` は、操作間の相互作用や考慮漏れを調べる補助モデルとして保持する。モデルの新設や検証範囲の拡張は一律に必須とせず、設計上の不確実性に応じて判断する。

現行仕様を表すモデルは、その対象の意味が変わったときに追随させる。共有する意味を変更する場合は、該当する既存モデルも更新・検証する。モデルの対象外の変更について、既存モデルへの追加や全モデルの検査を必須にはしない。補助モデルの保守を終える場合は、過去の検討資料であることをモデルと参照元に明示するか削除する。

## 設計変更の進め方

モデルで検証する場合は、まず確かめたい性質と前提を整理し、spec を更新・検証する。検証後に確定した設計と理由を関連する docs に反映してから実装する。モデルを使わない設計変更では、判断と理由を docs に反映してから実装する。

実装中に設計の不足や矛盾が見つかった場合も、この手順に戻る。検証結果は対象モデルと実行条件を明記して残す。モデルの検査はモデル内の性質を調べるものであり、Rust 実装の適合性は実装のテストで確認する。

## 各モデルの対象範囲

`spec/axon.qnt` は axon の思想的コアである A / B / C / D の直交性、dependency と Resurface condition の差、ready / blocked / orphaned / blocking cause だけを扱う。group の identity、包含、状態、依存は持ち込まない。

`spec/group_plan.qnt` はGroup の設計を扱う補助 model で、3 issue と 3 group からなる固定 Entity 集合を共有状態にする。実装上の DB transaction に対応して、各操作は 1 action で原子的に実行する。時刻は `AtDate` の評価に必要な小さい整数 clock だけを持つ。通信、障害、複数 actor、wall-clock、永続化はこの状態機械の関心ではない。

`spec/information_model.qnt` は Entity ごとの plan declaration、Control state、
Declaration Revision、Note、判断履歴、進行履歴を共有状態として扱う。title と
description の内容、actor の本人性、wall-clock、SQLite、CLI 構文は抽象化し、
情報の所有範囲、固定、Revision の現在参照、追記専用性、操作ごとの変更範囲を検査する。

Command のプロセス実行・終了コード・診断は Rust のテストで検証する。
core / Group model は Entity ごとの外部観測入力 `commandSatisfied` を非単調に変え、
観測と導出の一致、Group gate の閉鎖、保存状態の保持を検査する。この入力は Axon の
Control state の保存値ではない。情報モデルでは Command を不透明な Control 値として扱い、
設定変更の所有範囲と履歴を検査する。

拡張 model が保存状態として持つのは Entity map、一親の parent map、dependency 集合、clock である。`ready`、`blocked`、`orphaned`、active scope、`triage`、blocking cause、group の完了可能性、2 つの待機グラフは純粋関数で導出する。操作 witness のために使う `observed` は ghost state であり、axon の保存対象ではない。

## Group 拡張で検査する性質

| 種類 | 性質 |
| --- | --- |
| invariant | Entity の kind は変化しない |
| invariant | activation / completion wait graph は非循環である |
| invariant | ready は blocked / orphaned と排他で、active scope 内の Entity だけを含む |
| invariant | activation gate が閉じた group の子孫は ready にならない |
| invariant | Rejected の依存先は、依存元 group の子孫を含む対象を orphaned にする |
| invariant | Ended group の全子孫は terminal のまま保たれる |
| invariant | Rejected の子 group 自身は親の完了を妨げない |
| invariant | NotStarted に release された group の下に InProgress の子孫は残らない |
| invariant | Ended group 自身を別の親へ移動または親から解除できない |
| invariant | blocked の Entity には blocking cause が 1 件以上ある |
| invariant | blocking cause は未終端で、依存を遡る停止条件を満たす Entity だけである |
| invariant | Rejected を参照する `AfterEntity` は surfaced になる |
| invariant | triage は active scope 内の判断 frontier だけを示す |
| invariant | Command の導出が外部観測と一致し、観測変更が保存状態を変えない |
| witness | 進行中の子孫の状態を保ったまま Command で Group gate が閉じる |
| witness | 列挙した全 action が到達可能である |
| witness | 全子孫が終端した InProgress group が、Ended へ自動変更されず明示 done を待てる |
| witness | Rejected の子 group を含む親 group が完了可能になる |
| witness | Rejected の group を依存先にすると依存元が orphaned になる |
| witness | group を依存元にした未解決 dependency が子孫を blocked にする |
| witness | 祖先 group 由来の dependency が子孫の blocking cause に現れる |
| witness | group から group を参照する `AfterEntity` を設定できる |
| witness | 親 group の start 後に入れ子の Entity が ready になる |
| witness | 親 group が判断対象なら、その子孫は triage に出ない |

## 検査方法

検査対象のモデルについて、以下の Quint 0.32.0 の再現条件を使う。先頭の version 出力が異なる場合は、この再現条件の成功として扱わない。`quint run` は bounded random simulation であり、反例が見つからなかったことは全状態についての証明ではない。

```sh
quint --version

quint typecheck spec/axon.qnt
quint run spec/axon.qnt --main axon \
  --invariants invRejectedEndedStillBlocks invReadyExclusive \
    invIssueWaitsAcyclic invBlockedHasCause invCauseIsUnresolved \
    invCondRefSatisfiedByRejection invCondAndDepDiffer invCommandObservation \
  --witnesses wCommandFell \
  --max-samples 1000 --max-steps 80 --backend rust --n-threads 8 \
  --seed 2026090301

quint typecheck spec/group_plan.qnt
quint run spec/group_plan.qnt --main group_plan \
  --invariants invEntityKindsStable invRelationsSafe invReadyExclusive \
    invInactiveGroupDescendantsNotReady invRejectedDependencyOrphans \
    invEndedGroupDescendantsTerminal invRejectedChildGroupAllowsCompletion \
    invReleasedGroupHasNoInProgressDescendant invEndedGroupsCannotMove \
    invBlockedHasCause invCauseIsUnresolved invCauseStopsAtRoot \
    invAfterRejectedSurfaces invTriageIsCurrentFrontier \
    invCommandObservation invCommandObservationPreservesControl \
  --witnesses wDecide wStart wDoneIssue wDoneGroup wReleaseIssue wReleaseGroup \
    wSetDate wSetAfter wClearWhen wSetParent wUnsetParent wAddDependency \
    wRemoveDependency wTick wGroupAwaitingExplicitDone \
    wRejectedChildGroupCanComplete wGroupDependencyOrphaned \
    wGroupDependencyBlocksDescendant wInheritedGroupBlockingCause \
    wGroupAfterGroup wNestedEntityReady wTriageFrontier \
    wSetCommand wObserveCommand wCommandGateClosed \
  --max-samples 1000 --max-steps 80 --backend rust --n-threads 8 \
  --seed 2026090302

quint typecheck spec/information_model.qnt
quint run spec/information_model.qnt --main information_model \
  --invariants invDecidedDeclarationFrozen invOnlyOwnerDeclarationChanges \
    invChildSetOwnedByChild invIncomingDependencyOwnedBySource invParentsAcyclic \
    invCurrentSnapshotByDisposition invDecidedMatchesSnapshot \
    invLatestDecisionMatchesCurrentSnapshot invSnapshotOnlyForDecided \
    invEverySnapshotHasOrigin invDecisionSnapshotIndexesMonotonic \
    invConsecutiveSnapshotsDiffer invRecordsAppendOnly invDeclarationOpScope \
    invDecideOpScope invSetWhenOpScope invProgressOpScope invSupplementalOpScope \
    invSupplementalNeverRestricted invSupplementalRefUniquePerTarget \
    invSupplementalTargetsExist invSupplementalFullyObservable \
  --witnesses wSetTitle wSetDescription wSetParent wUnsetParent wAddDependency \
    wRemoveDependency wDecide wSetWhen wStart wDone wRelease wAddSupplemental \
    wFrozenNoOpAccepted wEndedUndecidedDeclarationEdited wNotStartedAndFrozen \
    wFrozenGroupGainsChild wFrozenTargetGainsIncomingDependency \
    wRedecidedWithNewSnapshot wSameSnapshotTwoDecisions \
    wUndecideClearsCurrentSnapshot wHistoricalSnapshotReappearsAsNew \
    wBaselineWithoutDecisionHistory wSupplementalOnDecidedAndEnded \
    wRepeatedSupplementalCreatesNewRecord wStorageOrderDiffersFromInputTime \
    wAllRecordKindsPresent \
  --max-samples 1000 --max-steps 80 --backend rust --n-threads 8 \
  --seed 2026090303
```

検査成功は command の exit status 0 だけではない。列挙した全 invariant に反例がなく、列挙した全 witness が出力上 1 trace 以上で観測されたことを確認する。witness 未観測でも `quint run` 自体は成功終了するため、出力確認を省略しない。backend、sample 数、seed を変えた検査は、その実行条件も結果とともに記録する。

## 過去の検査結果

2026-09-03 の統合確認では、core、group 拡張、情報統治を各 1,000 traces、最大 80 steps で実行した。core の 7 invariant、group 拡張の 14 invariant、情報統治の 22 invariant に反例は見つからず、列挙した witness はすべて少なくとも 1 trace で観測された。これは bounded random simulation の結果であり、完全探索による証明ではない。

## Group モデルと要求の対応

`spec/group_plan.qnt` は共有 Entity 状態、包含、dependency、Resurface condition、原子的な状態操作と導出値を対象にする。ID の文字列表現、title / description、SQLite migration、履歴、時刻の実装、actor / session claim、CLI の構文・表示、宣言ファイルと fingerprint は対象外である。

モデルの保守と設計変更は、この文書の冒頭の方針に従う。

| 要求 | Quint 上の対応 |
| --- | --- |
| 共通 Entity | `Entity` record と `State.entities` |
| 一親 tree | `State.parent`、`parentEdges`、`relationsSafe` |
| 共通 dependency / when | `State.dependencies`、`AfterEntity`、`waitingScope` |
| start / done / release / decide / when | 対応する `do*` action と guard / apply pure function |
| 包含・dependency の変更 | `doSetParent` / `doUnsetParent` / `doAddDependency` / `doRemoveDependency` |
| ready / orphaned / triage | 同名の pure function |
| active scope と group の完了 | `opensDescendants`、`withinActiveScope`、`groupCompletionSatisfied`、`canCompleteGroup` |
| blocking cause | `unresolvedTargets`、`blockingCauses`、対応する invariant / witness |
| Ended group の固定 | `hasEndedAncestor`、`structureMutable`、`invEndedGroupDescendantsTerminal`、`invEndedGroupsCannotMove` |
| cross-relation deadlock 防止 | `activationWaitEdges`、`completionWaitEdges`、`invRelationsSafe` |

過去の反例から得た設計判断は [設計判断](../design/decisions.md#モデル検査で見つかった考慮漏れ) を参照する。
