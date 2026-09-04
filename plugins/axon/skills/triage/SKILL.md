---
name: triage
description: 既存Axon Entityの採否、時期、declaration、包含、dependencyを調査し、供給された判断と合意済み計画から確定できる変更を安全に反映する個人用ワークフロー。独立したNoteや実装には使わない。
---

# 既存Axon Entityを判断する

`axon:conventions`と`axon-kit:triage`を使う。

## 判断材料を揃える

対象の全情報、関係するDeclaration Revision、判断履歴、関連Entityとfrontierを読み、未判断、前提喪失、既存判断の見直し、declarationだけの修正、Resurface conditionの変更を区別する。

現状維持を含む現実的な選択肢、Progress、Disposition、時期、関係、active scope、frontierへの影響、推奨と不確実性を示す。Dispositionの選択はユーザーの明示した結論だけを反映し、分析や推奨から採否を決めない。結論が供給されていなければ判断材料を返す。

Resurface conditionは、明示された日付または待機先をそのまま反映できる。いつ再浮上させるかを新たに選ぶ必要がある場合はユーザーへ返す。

## declarationと構造を整える

合意済みの目的、scope、完了条件を保持する文面修正と構造整理は自律して行う。意味を変えない表現の改善だけを理由に全文の再確認を求めない。目的、scope、完了条件、採否、公開仕様、計画の意味を変更する場合は確認する。

- 親またはdependencyが合意済み計画から一意に決まる場合は反映する。
- orphaned dependencyに一意な同等後継があれば置き換える。前提を続行、代替、放棄する選択が必要なら確認する。
- 合意済みscopeを子Entityへ分解し、必要な包含とdependencyを作れる。新しい成果、採用判断、独立した完了単位を加える場合は確認する。
- Groupの包含は合意済みscope内で整えられるが、子孫のProgressまたはDispositionは、明示された判断や別workflowの権限なしに変更しない。

固定declarationの変更は、公式kitの段階的な操作に従い、必要なUndecidedへの移行、関係の変更、供給済みDispositionへの再判断まで完了する。

## InProgressを扱う

declaration変更のための一時的なUndecided化では、作業継続が変わらない限り既存claimを維持し、機械的な再判断まで自律して進める。継続が明示されている場合もclaimを変えない。

InProgress EntityをRejectedまたはUndecidedへ変更するだけで、作業を継続、一時停止、恒久停止のどれにするか不明な場合は確認する。「却下して停止」のように恒久停止が明示されていれば、必要なNoteを残してdoneにする。「保留」のように一時停止が明示されていれば、必要なNoteを残してreleaseする。

Groupの判断は子孫の保存状態を副作用で変更しない。Rejected化によるactive scope、dependent、waiterへの影響と、非terminal子孫が祖先Groupの完了へ与える影響を確認する。

変更したID、判断理由、claimの扱い、作成した子Entity、構造と導出状態への影響、未解決事項を報告する。このskillは判断、declaration、構造の反映と検証で終了し、同じ依頼に実装workflowが明示されている場合を除いてstartや実装へ進まない。
