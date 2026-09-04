---
name: triage
description: 既存Axon Entityの採否、時期、declaration、包含、dependencyをユーザーと検討し、合意した結論だけを公式kitで反映する個人用ワークフロー。独立したNoteや実装には使わない。
---

# 既存Axon Entityを判断する

`axon:conventions`と`axon-kit:triage`を使う。

## 判断材料を揃える

対象の全情報、全Declaration Revision、判断履歴を読み、未判断、前提喪失、既存判断の見直し、declarationだけの修正、Resurface conditionの変更を区別する。前提喪失では、dependencyの修復とEntity自身のRejected化を別の選択肢として扱う。

現状維持を含む現実的な選択肢、各選択肢がProgress、Disposition、時期、関係、active scope、frontierへ与える影響、推奨と不確実性を示す。分析や推奨だけでは状態を変更しない。

採用に合わせて目的、完了条件、分解、dependencyへ実質的な判断を追加する場合は、変更後の完全なdeclarationを提示し、ユーザーの確認を得る。通常の実装詳細は実装者に委ねる。

## 合意を反映する

ユーザーが選んだ変更だけを`axon-kit:triage`で反映する。固定declarationの変更、理由付き再判断、段階的な操作、部分失敗は公式kitの契約に従う。

InProgress EntityのDispositionを変える場合は、既存claimを維持する、一時的にreleaseする、恒久的にdoneにする、のどれかをユーザーと確認する。継続ならclaimを変更しない。releaseまたはdoneが選ばれた場合だけ、必要な申し送りNoteを`axon-kit:add-note`で保存し、`axon:work-state`へ進む。

Groupの判断は子孫の保存状態を変更しない。Rejected化によるactive scope、dependent、waiterへの影響と、非terminal子孫が祖先Groupの完了へ与える影響を明示する。子孫の変更やGroupのreleaseまたはdoneは別の合意なしに行わない。

変更したID、判断理由、claimの扱い、作成した子Entity、構造上の影響、導出状態への影響、未解決事項を報告する。
