# Quintモデル

Axon の lifecycle、包含と dependency、候補一覧の評価、文面・Note・状態変更履歴の意味論を、実行可能な Quint のモデルとして置きます。状態・遷移・包含・候補集合・情報の契約そのものは [lifecycle](../docs/reference/lifecycle.md) と [候補と外部条件](../docs/reference/candidates.md) が定義し、ここでは各モデルが何を対象にし、何をどう検査するかを案内します。

## モデル一覧

| ファイル | 対象 | 対象外 |
| --- | --- | --- |
| [`lifecycle_rules.qnt`](lifecycle_rules.qnt) | Issue と Group が共有する状態5種類・操作7種類と、その前提・遷移先 | 状態変数と遷移の実行、包含、dependency、条件 |
| [`issue_lifecycle.qnt`](issue_lifecycle.qnt) | 単独 Issue の lifecycle と、抽象化した再浮上条件による浮上の導出 | 包含、dependency、Group、登録操作、declaration による編集、履歴の保存、記録者による権限判定、条件の種類と設定操作、一覧、浮上と着手許可の接続、CLI、永続化 |
| [`group_lifecycle.qnt`](group_lifecycle.qnt) | 計画と Issue の包含、所属変更、新規登録、Issue・計画間の dependency、判断候補・着手候補・着手中の導出 | 文面と記録、条件の種類・設定と評価失敗、CLI と表示、ID 発行、永続化 |
| [`lifecycle_reachability.qnt`](lifecycle_reachability.qnt) | `group_lifecycle` の到達性を補う3つの探索入口 | 許可条件・状態更新・検査する性質（`group_lifecycle` のものをそのまま使う） |
| [`candidate_evaluation.qnt`](candidate_evaluation.qnt) | `axon proposals`・`axon tasks` の1回の取得における評価範囲、評価回数、結果の共有、判定失敗 | 実コマンドと shell、作業ディレクトリ、終了コードの解釈、timeout と中断、出力の上限と診断、並行実行 |
| [`lifecycle_information.qnt`](lifecycle_information.qnt) | title・description の編集範囲、Note の追記、状態変更履歴と状態の一致 | 包含・dependency・最終確認による追加の制約、日時・理由・記録者情報、表示順、公開 ID、永続化 |

Quint の `import` の向きは `lifecycle_rules` を起点にした一方向です。`issue_lifecycle`・`group_lifecycle`・`lifecycle_information` がそれぞれ `lifecycle_rules` を取り込み、`lifecycle_reachability` と `candidate_evaluation` が `group_lifecycle` を取り込みます。`issue_lifecycle` と `lifecycle_information` は互いにも `group_lifecycle` にも依存しません。

## モデル化と実装検証の分担

すべての設計判断を Quint の状態に追加することはしません。状態・関係・操作の相互作用に不確実性がある部分をモデル化し、実行環境や付随情報の取得・保存は設計判断として記述して実装側で検証します。モデルへ残す検証専用の観測値も、対象の性質を確かめるために必要な範囲に留めます。

| 対象 | 扱いと理由 |
| --- | --- |
| lifecycle、包含、dependency | 遷移の前提・循環・終了条件が相互作用するため維持する |
| 候補一覧の評価範囲・結果共有・失敗 | 評価省略が候補の取りこぼしや失敗の見落としにつながらないかを検査するため維持する |
| 条件コマンドの不透明な識別子 | 条件の置き換えと保持を区別するため維持し、実コマンドや環境はモデル化しない |
| 文面・Note・状態変更履歴 | 編集可能状態、追記専用性、履歴と状態の一致を検査するため維持する |
| 履歴の日時・任意の理由 | 遷移に影響せず値の保持を調べるだけなので、探索には含めず実装検証で扱う |
| 記録者情報・エージェント連携・外部コマンドの実行環境 | 操作の可否に使わない付随情報や実行詳細として、設計判断と実装検証で扱う |
| 重複着手 | モデル上は原子的な状態遷移で扱い、実際の競合は保存処理の並行テストで確認する |

同一時刻の履歴が上書きされないこと、日時・理由・記録者情報の保持、自動取得と取得不能時の継続、Note の同内容追記の区別、保存失敗時の原子性は実装側の検証対象です。独立した予約状態やエージェント別の状態は持ちません。各モデルはそれぞれの検証範囲を維持し、変更に関係のある検査を選んで実行します。検査の分担と実装側の入口は [検証方針](../docs/development/verification.md) にあります。

## 各モデルの探索と検証する性質

### `lifecycle_rules`

状態変数を持たない純粋な定義だけの module で、単独では実行しません。`canPerform` と `applyOperation` を他のモデルが共有することで、同じ遷移規則を Issue と計画の双方に適用します。

### `issue_lifecycle`

提案として登録した直後の `Undecided` から始め、外部入力は不成立から開始します。`step` は7操作と外部入力の変化を探索候補にし、前提を満たすものだけが実行されます。外部入力は両方向へ変化でき、`Cancelled`・`Completed` の間も変化しますが、条件の評価対象には戻りません。

9つの invariant は、`NotStarted`・`InProgress`・`Completed` には直近の見直しより後の採用が必要なこと、`InProgress` の入口が `NotStarted` からの `Start` に限ること、`Completed` の入口が `InProgress` からの `Complete` に限ること、一度完了すると完了のままであること、明示操作の行き止まりが完了だけであること、外部入力と明示操作が互いの値を変えないこと、`Cancelled`・`Completed` が評価されず浮上しないこと、評価対象の浮上が入力の成立と一致することを検査します。19の witness は7操作それぞれの到達に加え、未判断からの見送り・未着手からの取りやめ・進行中からの打ち切りを区別し、外部入力の両方向の変化と各状態での浮上・非浮上を観測します。初期状態だけではどの witness も成立しません。

条件定義を固定しているため、取りやめ時の定義の保存や読み戻しは検証しません。`NotEvaluated` は意味上の評価除外を表すもので、実際に外部コマンドが呼ばれないこと、読み取りコスト、実行失敗と副作用は実装側で検証します。

### `group_lifecycle`

計画3件と Issue 用の固定 ID 3件を使い、2 Issue が計画0に所属し、残り1件を未登録の枠として始めます。未登録は探索領域を有限にする仕組みであって、新しい lifecycle ではありません。全 Entity は `Undecided`、計画はすべて所属なし、条件入力はすべて不成立から始めます。これは探索の開始点であり、製品の再浮上条件の初期値ではありません。`step` は計画の lifecycle、Issue の lifecycle、それ以外の操作（所属変更、新規登録、dependency の編集、条件入力の変化）の三枝から選び、前提を満たして着手・完了へ進む経路も探索しやすくします。ID は固定集合から選び、未登録 ID の再利用と削除は扱いません。

34の invariant は、包含（進行中の子の親と終了した計画の子、終了した計画の構成の固定、進行中の Entity の祖先、終了した計画の子孫、所属の一意性と循環の不在、計画の移動が子孫を変えないこと）、最終確認を経た明示操作でのみ起きる計画の完了、操作ごとの影響範囲、dependency（参照の妥当性、通常完了経路の非循環、自己依存と祖先・子孫間の依存の不在、着手・完了時の前提と完了後の保持、完了した Entity の固定）、候補（評価されない状態、判断候補と着手候補の定義、判断候補と着手候補・着手中の非重複、条件入力が着手可否を変えないこと）、そして未終了の仕事が残る間に全操作が行き止まりにならないことを検査します。97の witness は各操作の到達に加え、進行中のままの移動、最終確認待ちとその不合格、計画をまたぐ依存、依存先の完了による解禁、直接・間接の循環の拒否、親の浮上による候補の除外と復帰、三階層の包含と Issue・子計画の混在を観測します。

行き止まりがないという性質は整理・再判断の操作も含み、当初の計画どおり必ず完了できることを意味しません。完了した祖先の配下にある取りやめ済みの Entity は、再検討可能な仕事として数えません。依存先が取りやめでも依存元の取りやめは強制されず、依存関係の見直しや再検討は明示操作として残ります。同じ所属の再指定はモデルでは無効な操作として探索から外しますが、CLI では同値の指定は成功した no-op です。前提の循環検査と包含の検査はこの固定範囲を対象とし、四階層以上の木や、より多い Entity を含む具体的なグラフは探索していません。このモデルでは追加できる Issue が1件という探索上の上限があり、新規登録が無効になることは製品が追加件数を制限するという意味ではありません。最終確認が通るかは抽象入力で、確認工程の実装や不合格の理由は扱いません。

### `lifecycle_reachability`

通常の探索では、依存の追加や取りやめが先行し、複数の仕事を進行中に保った移動、未登録枠を残した最終確認待ち、依存先の完了による後続の解禁に到達しにくくなります。これらは操作の順序が長く噛み合ったときにしか成立せず、通常探索では観測率が0.1%を下回ります。探索を厚くするより、選ぶ操作を絞った入口から狙うほうが、同じ試行数で桁違いに濃く検査できます。

`group_lifecycle` と同じ `init` と操作を使い、1 step で選ぶ操作だけを絞る3つの入口を持ちます。`workAndMove` は依存のない状態から採用・着手・移動・登録を選び、進行中の部分木の移動を調べます。`finishAndExtend` は採用・着手・完了・取りやめと計画1の計画0への所属を選び、計画0が最終確認可能になった時点で新規登録を選びます。`dependAndFinish` は依存の追加と採用・着手・完了だけを選び、依存先の完了によって後続が着手・完了できるようになる瞬間と、全子完了を経た計画の完了を調べます。許可条件・状態更新・到達目標は変えないため、invariant は `group_lifecycle` のものをそのまま指定します。これらの選び方は到達性の確認用であり、製品の workflow を定めるものでも、通常探索の分布を表すものでもありません。

### `candidate_evaluation`

`group_lifecycle` の状態と許可条件を再利用し、一覧の取得を開始・Entity ごとの評価・公開という複数の観測ステップへ具体化します。`queryInit` は `group_lifecycle` の `init` に加えて全 Entity の条件を `Unset` から始め、`queryStep` は取得の各段と、取得外の条件編集・計画変更を混ぜて探索します。`group_lifecycle` の bool 入力と候補集合は失敗のない場合の意味を定める基準として残り、評価回数・省略・失敗をこのモデルで扱います。`group_lifecycle` の `false` 初期入力は未成立の外部入力から探索を始める指定で、条件未設定の初期値ではありません。このモデルはすべて `Unset` から始め、未設定が成立として扱われることを検査します。未設定の成立判定も観測ステップとして数えますが、外部コマンドを呼ぶという意味ではありません。

成功時には、未評価の入力をすべて成立と仮定しても、すべて未成立と仮定しても、`group_lifecycle` 側の候補集合と一致することを検査します。これにより、評価を省いた入力が候補の欠落を隠していないかを調べます。失敗は成功の候補集合とは異なる variant で表し、失敗した Entity を保持します。途中で観測した候補は成功結果として公開しません。このほか、評価済みの各 Entity が必要な候補かその祖先であること、全祖先の成立後にだけ評価すること、評価回数の上限、同一呼び出し内の結果共有と次回の初期化、条件編集と明示操作が評価処理を呼ばないこと、取得と外部の結果が保存した計画・条件を変えないことを検査します。成立の持続、次回の未成立化、失敗からの回復はそれぞれ到達目標として扱います。

一覧取得中の lifecycle・包含・dependency・条件設定は固定し、明示操作は取得の間に行います。これは単一呼び出しを調べるための前提であり、並行更新や snapshot の実装契約を決めるものではありません。外部の結果は Entity ごとの評価時に選ぶため、異なる Entity の条件を同一瞬間に観測する保証も置きません。独立した枝の評価順は非決定的で、最初の失敗で取得を終了します。エラー時にどの枝まで観測済みかは保証しません。実行契約は自然言語で定め、Quint では実行結果を成立・未成立・判定失敗へ抽象化します。shell・作業ディレクトリ・環境変数、終了コードの解釈、timeout と process group の終了、出力の上限と診断、実際に外部コマンドを呼ばないことと呼び出し回数は実装側で検証します。

### `lifecycle_information`

固定2 Entity、文面の値3種類で、未判断の登録済み Entity から始めます。文面と Note の内容は不透明な整数で更新と保持を区別し、履歴は変更前後の状態だけを持つ追記列へ抽象化します。日時・任意の理由・記録者情報は設計上の保存項目として残しますが、操作の可否や状態遷移に影響しないため探索変数にしません。登録操作と初期状態の記録は扱いません。

7つの invariant は、操作が対象以外の Entity を変えないこと、文面の編集が編集可能な状態に限ること、終了後の文面が固定されること、Note が追記専用であること、状態変更と履歴追加が一体で確定すること、履歴を再生すると現在の状態になること、完了が戻らないことを検査します。18の witness は7遷移それぞれの到達、採用後・着手中・再検討後の文面編集、各状態での Note 追加、同じ内容の追記が別の記録になることを観測します。基本遷移だけを持つため、このモデルが許す遷移がそのまま実際の Group や依存を持つ Issue で許可されるわけではありません。実時刻の取得、精度、時計補正、公開 ID、actor、記録の表示順、永続化と保存失敗はこのモデルの外です。

## 再現手順

以下は repository root から実行します。`quint run` は bounded random simulation であり、検査の成功は exit status だけでなく、列挙した全 invariant に反例がなく、列挙した全 witness が出力上1 trace 以上で観測されたことを確認します。通常探索で観測率の低い witness は補助探索が担保するため、`group_lifecycle` と `lifecycle_reachability` は合わせて1つの検査として扱い、全 witness がいずれかの探索で観測されたことを確認します。

seed は固定しません。`quint run` は seed を渡すと再現のために単一スレッドで実行し、渡さないときだけ CPU 数に応じて並列化します。同じ seed を使い続けても、モデルが変わらない限り同じ経路をなぞるだけで新しい情報は得られないため、実行ごとに異なる経路を探索させます。観測される trace 数は実行ごとに変わります。反例が出た場合は `Use --seed=0x… to reproduce.` が出力されるので、その seed を指定すれば同じ反例を再現できます。`--n-threads` は既定で CPU 数になるため指定しません。

### `issue_lifecycle`

```sh
quint typecheck spec/issue_lifecycle.qnt
quint run spec/issue_lifecycle.qnt \
  --invariants invAdoptedBeforeWork invCompletedFinal \
    invWorkEnteredThroughStart invCompletionRequiresWork invOnlyCompletionStops \
    invConditionChangePreservesLifecycle invLifecycleOperationsPreserveCondition \
    invTerminalNotEvaluated invActiveEvaluationMatchesInput \
  --witnesses wAccept wWithdraw wStart wRelease wComplete wCancel wReconsider \
    wRejectProposal wCancelBeforeStart wCancelDuringWork \
    wConditionRose wConditionFell wUndecidedSurfaced wNotStartedSurfaced \
    wInProgressSurfaceFell wCancelledSuppressed wCompletedSuppressed \
    wReconsiderSurfaced wReconsiderWaiting \
  --max-samples 100000 --max-steps 80 --backend rust --verbosity 1
```

### `group_lifecycle`

```sh
quint typecheck spec/group_lifecycle.qnt
quint typecheck spec/lifecycle_reachability.qnt
quint run spec/group_lifecycle.qnt \
  --invariants \
    invParentOfWorkingChild invClosedChildrenTerminal invCompletedStayCompleted \
    invClosedStructureFrozen invCompletionReviewed invOperationScope \
    invDependencyOperationScope invDependenciesValid invPreconditionsAcyclic \
    invCompletedPrerequisites invStartAndCompleteRequirePrerequisites invCompletedDependenciesFrozen \
    invSelfDependencyRejected invNoAncestorDependencies invConditionInputScope \
    invTerminalConditionsNotEvaluated invCandidatesAreStartable invCandidateDefinition \
    invJudgmentDefinition invJudgmentSeparateFromWork invDependencyEditsPreserveJudgment \
    invWorkingVisibility invConditionChangesPreserveEligibility invMembershipValid \
    invRegistrationMatchesDecision invRegistrationNeverStarts invClosedEditsDisabled \
    invGroupParentsScope invContainmentAcyclic invWorkingAncestors \
    invClosedDescendantsTerminal invGroupClosedEditsDisabled invContainmentCycleEditsDisabled \
    invNoUnexpectedDeadEnd \
  --witnesses \
    wGroupAccept wGroupWithdraw wGroupStart \
    wGroupRelease wGroupComplete wGroupCancel \
    wGroupReconsider wIssueAccept wIssueWithdraw \
    wIssueStart wIssueRelease wIssueComplete \
    wIssueCancel wIssueReconsider wMixedAdoption \
    wParentStartRequired wParentReleaseBlocked wAwaitingFinalCheck \
    wFailedCheckCannotComplete wCompletedWithCancelledChild wCompletedAllChildrenDone \
    wBothChildrenWorking wUndecidedChildBlocksClosure wAttach \
    wDetach wMoveBetweenPlans wWorkingDetach \
    wWorkingMove wRegister wRegisterProposal \
    wRegisterAdopted wAdoptedRegistrationWaitsForParent wAdoptedRegistrationCanStart \
    wRegisterOutside wAddAfterFinalCheckReady wReconsiderAllowsMembership \
    wRemoveUndecidedForCompletion wAddDependency wAddDuringWorkBlocksCompletion \
    wRemoveDependency wCrossPlanDependency wDependOnCompletedIssue \
    wDependencyRetainedOnMove wCancelledPrerequisiteBlocks wPrerequisiteCompletionUnblocksStart \
    wPrerequisiteCompletionUnblocksFinish wIndirectCycleRejected wDirectCycleRejected \
    wGroupDependsOnIssue wIssueDependsOnGroup wGroupDependsOnGroup \
    wGroupWaitsForIssue wDependentWaitsForGroupReview wGroupCompletionUnblocksDependent \
    wGroupDependencyAddedDuringWork wIssueMoveRejectedByPrerequisites wGroupMoveRejectedByPrerequisites \
    wAncestorDependencyRejected wDescendantDependencyRejected wCancelledEscapeStillRejected \
    wCrossBranchCycleRejected wGroupConditionRose wGroupConditionFell \
    wIssueConditionRose wIssueConditionFell wJudgmentIssue \
    wJudgmentGroup wJudgmentBeforeParentStart wJudgmentWithUnfinishedPrerequisite \
    wParentHidesJudgment wParentResurfacesJudgment wAcceptUnderHiddenParent \
    wAcceptWithUnfinishedPrerequisite wCandidateIssue wCandidateGroup \
    wStartHiddenIssue wStartUnderHiddenParent wStartHiddenGroup \
    wParentHidesCandidate wParentResurfacesCandidate wParentHiddenKeepsWorkingChild \
    wIssueHiddenKeepsWorking wCompletedIssueConditionIgnored wCancelledGroupConditionIgnored \
    wGroupAttach wGroupDetach wGroupMoveBetweenPlans \
    wWorkingSubtreeMove wThreeGroupLevels wMixedChildren \
    wNestedIssueWorking wGroupWaitsForParentStart wGroupReleaseBlockedByGroup \
    wUndecidedGroupBlocksClosure wParentCompletesAfterGroup wNestedGroupCancel \
    wIndirectContainmentCycleRejected \
  --max-samples 30000 --max-steps 200 --backend rust --verbosity 1
```

### `lifecycle_reachability`

補助探索も上記と同じ全 invariant を指定します。各入口には、通常探索では観測率が低く、実行ごとの偶然で落ちうる witness を割り当てます。以下はモデルから定義名を読み取って再現します。

```python
from pathlib import Path
import re
import subprocess

model = Path("spec/group_lifecycle.qnt").read_text()
invariants = re.findall(r"val (inv\w+)\s*=", model)
for step, witnesses in [
    ("workAndMove", ["wWorkingSubtreeMove", "wNestedIssueWorking", "wBothChildrenWorking",
                     "wWorkingMove", "wWorkingDetach", "wStartUnderHiddenParent"]),
    ("finishAndExtend", ["wAddAfterFinalCheckReady", "wParentCompletesAfterGroup",
                         "wParentHiddenKeepsWorkingChild"]),
    ("dependAndFinish", ["wPrerequisiteCompletionUnblocksFinish", "wPrerequisiteCompletionUnblocksStart",
                         "wCompletedAllChildrenDone"]),
]:
    subprocess.run([
        "quint", "run", "spec/lifecycle_reachability.qnt",
        "--step", step, "--invariants", *invariants, "--witnesses", *witnesses,
        "--max-samples", "2000", "--max-steps", "100",
        "--backend", "rust", "--verbosity", "1",
    ], check=True)
```

### `candidate_evaluation`

```sh
quint typecheck spec/candidate_evaluation.qnt
```

```python
from pathlib import Path
import re
import subprocess

source = Path("spec/candidate_evaluation.qnt").read_text()
invariants = re.findall(r"val (inv\w+)\s*=", source)
witnesses = re.findall(r"val (w\w+)\s*=", source)
subprocess.run([
    "quint", "run", "spec/candidate_evaluation.qnt",
    "--init", "queryInit", "--step", "queryStep",
    "--invariants", *invariants, "--witnesses", *witnesses,
    "--max-samples", "30000", "--max-steps", "60",
    "--backend", "rust", "--verbosity", "1",
], check=True)
```

### `lifecycle_information`

```sh
quint typecheck spec/lifecycle_information.qnt
```

```python
from pathlib import Path
import re
import subprocess

source = Path("spec/lifecycle_information.qnt").read_text()
invariants = re.findall(r"val (inv\w+)\s*=", source)
witnesses = re.findall(r"val (w\w+)\s*=", source)
subprocess.run([
    "quint", "run", "spec/lifecycle_information.qnt",
    "--invariants", *invariants, "--witnesses", *witnesses,
    "--max-samples", "100000", "--max-steps", "60",
    "--backend", "rust", "--verbosity", "1",
], check=True)
```

## 検証結果

いずれも bounded random simulation の結果であり、全状態の証明でも、必ず完了することの保証でもありません。どのモデルも、完了への到達を強制する公平性は仮定しません。seed を固定しないため、観測される trace 数は実行ごとに変わります。ここに残すのは実行条件と判定で、trace 数は witness の到達しやすさの目安として添えます。

以下は 2026-09-22、Quint 0.32.0、Rust backend、並列実行、seed 未固定での結果です。5モデルの型検査はいずれも成功しました。

- `issue_lifecycle`: 100,000 traces、最大80 steps。9 invariant に反例はなく、19 witness はすべて観測されました。最も少ない進行中の浮上の消失で28.3%です。
- `group_lifecycle`: 30,000 traces、最大200 steps。34 invariant に反例はなく、97 witness 中96 witness を観測しました。進行中の部分木移動は通常探索では未到達で、補助探索が担保します。通常探索だけが到達する witness のうち最も少ないのは、計画の完了による後続着手の解禁で291 traces、次いで別の枝を経由する循環の拒否で408 traces です。
- `lifecycle_reachability`: 同じ34 invariant を指定し、3入口を各2,000 traces、最大100 steps で実行して反例はありませんでした。`workAndMove` は進行中の部分木移動を1,635 traces、`finishAndExtend` は最終確認待ちからの追加を1,798 traces、`dependAndFinish` は依存先の完了による後続の完了解禁を85 traces、全子完了を経た計画の完了を2,000 traces で観測しました。通常探索と補助探索を合わせ、全97 witness がいずれかの探索で1 trace 以上に到達しました。補助入口は到達性の確認用に操作を選び分けており、通常探索の分布とは区別します。
- `candidate_evaluation`: 30,000 traces、最大60 steps。15 invariant に反例はなく、27 witness はすべて観測されました。最も少ない評価失敗後の着手で50 traces です。
- `lifecycle_information`: 100,000 traces、最大60 steps。7 invariant に反例はなく、18 witness はすべて観測されました。最も少ない Release で9,248 traces です。

各モデルの traces 数は、補助探索が担保しない witness が偶然に左右されずに観測される水準を下限とし、そのうえで invariant を叩く厚みを加えて決めています。観測率の低い witness を通常探索の traces 数で拾おうとするより、補助入口を足すほうが確実です。

## 更新するとき

状態・遷移・初期状態・モデルの対象範囲を変えるときは、該当するモデルを更新して再検証し、確定した意味を [lifecycle](../docs/reference/lifecycle.md) や [候補と外部条件](../docs/reference/candidates.md) などの契約文書へ反映してから実装へ進みます。実行条件とその結果はこのファイルの「検証結果」に、時点と条件を明記して残します。手順の全体は [検証方針](../docs/development/verification.md) にあります。
