---
name: triage
description: 既存 Entity を調査し、与えられた Disposition、Resurface condition、declaration、containment、dependency の変更を安全に適用する。判断支援と既存 Entity の変更に使い、判断者の選択、独立した Note の追記、実装には使わない。
---

# 既存の Axon Entity を調査または変更する

`axon-kit:conventions` を使う。モデル contract を読み、書き込み前には mutation contract も読む。

## 判断対象を明らかにする

対象の現在の context を読む。保存された history や状態が、可能な選択肢または要求された結果に影響する場合は、Declaration Revision、`axon log <id>`、関連 Entity を読む。

次を区別する。

- 未判断の declaration
- prerequisite の喪失による Entity の orphaned 化
- Accepted、Rejected、Ended の Entity の再検討
- declaration だけの訂正
- Resurface condition の変更

Disposition 変更を提案または適用する前に、現在と提案後の terminal status を比較する。対象の `axon show <id> --skip-command-evaluation` から direct dependent、Group context、`AfterEntity waiter` を調査する。waiter が Group の場合は必要に応じてその Group を個別に `show` し、影響を受ける frontier を別途評価する。

orphaned Entity では、prerequisite の repair と reject を同じ操作として扱わない。

## 助言と適用を分離する

判断支援では、現在の判断対象、現実的な選択肢、関係する状態・関係への影響、要求された場合は recommendation を返す。

分析は mutation を許可しない。呼び出し側の依頼または workflow が与えた結論だけを適用する。その結論に重要な declaration または関係の選択が未解決で残る場合は、書き込まず `input required` を返す。

Axon は結論の出所を人または agent に限定しない。この skill は協業方針を選ばず、与えられた結論を検証して適用する。

## 与えられた変更だけを適用する

各 mutation を別々に実行し、次の phase の前に postcondition を検証する。

- Disposition には `axon decide ...`、Resurface condition には `axon when ...` を使う。reason が与えられた場合は `-r <reason>` を含め、Note ではなく typed history に保存する。
- `Undecided` declaration は Disposition を変えずに編集できる。
- 固定済み declaration を実際に変更する場合、まず reason が与えられていればそれを付けて `axon decide undecide <id>` を実行し、draft を検証し、与えられた declaration field と関係だけを編集し、完全な declaration を検証してから、与えられた最終 Disposition を別の transition として適用する。
- no-op の declaration value に再検討は不要である。
- 段階的な変更が失敗した場合は mutation contract に従って結果を照合する。無効な入力構文など復旧可能な実行 error は、与えられた判断を変えずに訂正し、安全なら許可済み phase を続ける。結果が不明なままである場合、重要な判断が不足している場合、または復旧に未許可の作用が必要な場合は停止し、現在の状態、完了済み phase、残りの phase を報告する。以前の declaration や Disposition を自動的に復元しない。
- 補足の finding や handoff は `axon-kit:add-note` に渡し、description に追記しない。

採用済み計画を分解するときは、判断済み child に `axon-kit:plan`、未解決 child に `axon-kit:capture` を使う。与えられた parent と dependency の関係だけを適用し、child を暗黙に開始しない。

## work state の独立性を保つ

Disposition の変更は Progress や claim を変えない。呼び出し側が lifecycle 変更を別途要求しない限り両方を保持し、要求された場合はその作用を `axon-kit:work-state` に渡す。triage 中に claim を暗黙に clear しない。

Group の判断は descendant state を書き換えない。descendant の active scope への影響を報告する。Rejected Group はすでに terminal であり、可視な non-terminal descendant は安定した inactive saved state として残りうる。reject、release、completion が必要だと推測しない。保存済み claim または外部作業に実際に disposition が必要なら、その事実と Entity ごとの選択肢を報告し、descendant を自動変更しない。Entity の release または end の lifecycle precondition は `axon-kit:work-state` に属する。

## 検証して結果を返す

変更した各 Entity を読み、要求された declaration、Control state、関係、typed history を検証する。与えられた変更に関係する関連作用と導出作用も検証する。

変更したすべての ID、与えられた reason、作成した child、保持した claim state、関係する構造的・導出的影響、該当する場合は変更した storage artifact、未解決の選択、storage 結果の分類を返す。実装を開始しない。
