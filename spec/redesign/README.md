# 再設計モデル

保存層のやり直しと、それに伴うコアの意味論の変更を、実装と契約文書の前に Quint で固めるためのモデル群です。案が確定したら、`spec/` 直下の正式なモデル・契約文書・実装へ反映し、この directory は役目を終えます。

コア側のモデル（`lifecycle_rules`・`plan_lifecycle`・`plan_reachability`）は `spec/` 直下の正式版（`lifecycle_rules.qnt`・`group_lifecycle.qnt`・`lifecycle_reachability.qnt`）へ取り込み済みで、コアの規則・探索・検証結果は [spec/README.md](../README.md) を参照します。この directory に残るのは保存層の統合側（`record_integration*.qnt`）で、その正式版への取り込みまで再設計後の案として置きます。

## 前提とした設計

保存層の前提:

- 保存先は不変な記録の集合。現在値は記録から導出し、本文・関係・条件の編集も記録にする。書込は 1 行の追記。
- Entity ごとの記録は因果 DAG を成す。head が複数ある Entity を「衝突」とし、全 head を親にする解決記録で一つの値を選ぶ。値が等しくても衝突として扱い、黙って畳まない。解決記録以外の記録は親を一つしか持たない。
- Note は因果を持たない追記専用の集合。
- Git の merge・rebase・cherry-pick はいずれも「相手の記録の部分集合を取り込む」操作で、Axon は Git の統合時に呼ばれない。衝突と構造の不整合は次の読取と CI が検出する。
- 記録 ID は内容から決まる hash で、同じ ID なら同じ内容。Entity ID は乱数 8 桁で、同じ ID の作成記録が二つあれば作成記録が二つとも head になるので、通常の衝突として見える。
- cherry-pick や revert で記録の一部だけが branch に入ると、親の欠けた記録が古い記録と並んで head になり、偽の衝突として見える。これは受け入れ、CLI が「片方は親記録が欠けていて新しい可能性が高い」と示す。祖先集合を記録に持たせて偽の衝突をなくす案は、困ったときに後から足す。

コアの意味論の変更（`Reopen`、Group の実効 lifecycle、Issue の `Start` の前提、kind の変換、一覧の状況）は [spec/README.md](../README.md) の「モデルが表す規則」を正とし、統合モデルはその規則のうち統合の検査に要る範囲を持つ。

## モデル一覧

| ファイル | 対象 | 対象外 |
| --- | --- | --- |
| [`record_integration.qnt`](record_integration.qnt) | 2 replica が記録の集合を任意の部分集合で持ち寄る統合、衝突の検出と解決、構造の違反の検出と修復 | file の bytes と行の統合、lock、探索、変換、再浮上条件と一覧 |
| [`record_integration_paths.qnt`](record_integration_paths.qnt) | `record_integration` の到達しにくい経路を狙う 5 つの入口 | 同上 |
| [`record_integration_test.qnt`](record_integration_test.qnt) | 固定した順序で特定の経路を再現する `run` テスト 6 本 | 探索 |

`record_integration` の lifecycle 操作の前提は、統合の検査に要る範囲へ簡略化した部分集合です（変換、再浮上条件、Group の `Cancel` の細部を持たない）。lifecycle・包含・dependency の規則の正本は `spec/group_lifecycle.qnt` とし、両者で異なる部分は `group_lifecycle` を優先します。下の「モデリングで確定した規則」は両方のモデルに入れています。

## モデリングで確定した規則

書き始めた時点の案では足りず、反例と独立 review から足した規則です。いずれも invariant を弱めずに規則側を直しました。コア側の 3 件（守る性質、`Undecided` の Group の下の完了済み子孫、`Blocked` の残余定義）は [spec/README.md](../README.md) の「反例と review から足した規則」を正とし、統合モデルにも同じ規則を入れています。以下は統合側の規則です。

- **複数 head はすべて衝突にする。** 当初は値の等しい並行 head を次の記録が畳む規則を置いていたが、それが要った場面（両側の worktree が親 Group を `axon start` する）は Group の `Start` を導出に変えて消えた。残るのは同じ actor が二つの worktree で同じ Issue を `axon start` した、両側で同じ提案を `axon accept` した、といった稀な場合で、衝突として見えた方が二重作業に気づける。畳む規則をやめると、現在値の導出が「head が一つならその値、複数なら衝突」の二分岐になり、二重登録の特別扱いも消える。
- **Issue の `Start` の排他性は現在値で表す。** 統合モデルでは `InProgress` の現在値に着手した actor を含め、着手の記録が誰のものかを現在値で読める。特別な規則を足さずに、通常の衝突判定で済む。
- **衝突があれば通常操作を止める。** 衝突中の Entity が一つでもある replica では、解決と Note 以外の操作を拒否する。
- **構造の不整合は「違反」の集合として導出し、通常操作は違反を増やさない。** 違反は Entity ごとに種類を持つ（包含の循環、親の不在、終了した親の下の未終了、実効 `InProgress` の Group の未採用の祖先、`Completed` の未完了の依存先、依存先の不在、通常完了経路の循環）。所属変更・dependency の追加・登録は「操作後の違反が操作前の違反の部分集合」であることを要求し、それ以外の操作は各操作の前提で扱う。無関係な違反が残っていても操作は止まらない。
- **違反に含まれる Entity には修復のための免除を与える。** 終了した親の配下は変更しないという固定と、`Completed` の dependency の固定、`Completed` の依存元による `Reopen` の阻止を、その Entity が違反に含まれるときだけ免除する。終了した Group どうしの包含の循環（両側の移動と完了の取り込みで起きる）は取り外しで、完了済みの相互依存（解決の選択で起きる）は dependency の削除で直せる。免除は有効な store では働かない。違反があり衝突がない store には、修復を確実に進める通常操作が一つはあることを invariant にしている。修復の進み具合は「違反の集合が縮む」か「集合が同じでも違反に含まれる Entity の dependency と所属の辺が減る」で測る。
- **解決記録は違反の検査を免除する。** 解決の選択で違反ができることがあり、それを解決時に拒否すると、どの選択も拒否されて行き止まりになる場合がある。解決で head を一つに戻してから、通常操作で直す。
- **同じ ID の二重登録は、片方の系列を選んで解決する。** 作成記録が二つとも head なので通常の衝突として見え、解決記録が両系列の head を親にして片方の値を採る。捨てた側の内容は登録し直す。

## 各モデルの探索と検証する性質

コア側（`group_lifecycle`・`lifecycle_reachability`）の探索と性質は [spec/README.md](../README.md) にあります。

### `record_integration`

replica 2 つ、actor 2 つ、Entity は固定 ID 5 件。0 は Group、1 は 0 の子 Issue、2 は所属なしの Group、3 は所属なしの Issue、4 は未登録の枠で、両 replica は同じ base から始める。`step` は一方の replica での通常操作（lifecycle 8 操作、文面編集、移動、dependency の追加と削除、登録、Note）、解決、取り込みから選ぶ。取り込みは相手にあって自分にない記録を、全部（merge）、連番の接頭辞まで（rebase の途中）、一件だけ（cherry-pick・squash）のいずれかで取る。

検証専用に、各記録は祖先の記録 ID の集合を持ち、各 replica の view（head、衝突、settled、現在値、gap、違反）は状態変数として状態ごとに一度だけ更新する。どちらも記録と store から導出できる値の写しで、実装の保存項目ではない。head の判定には祖先集合を使わない（実装は親だけを知る）。

16 の invariant のうち意味のあるものは、通常操作が衝突のない replica でしか起きず違反を増やさず gap を変えず他の Entity の記録を増やさないこと、有効な store では修復の免除が働かないこと、違反があり衝突がない store には修復を確実に進める通常操作があること、全部取り込みの後に settled な Entity の現在値が取り込み前の自分か相手の現在値であること（値を捏造しない）、両側が settled で値が異なりどちらの head も他方に先んじていなければ取り込み後は衝突になること（黙って片方を選ばない）、解決の直後に settled になること。記録が消えないこと、記録 ID の一意性、解決記録以外の記録が親を一つしか持たないこと、解決記録が親のどれかの値を採ること、view の写しの一致、owner と lifecycle の対応は構成の確認として残す。

43 の witness は、別 actor の並行 `Start` の衝突、値の等しい並行記録が衝突として見えその解決で片方を選ぶこと、解決、解決記録どうしの並行（同じ値でも違う値でも再び衝突になり、解決の解決で収束）、部分取り込みによる gap とその充足、gap による偽の衝突、収束、違反の種類ごとの観測（終了した Group への子の流入、包含の循環、通常完了経路の循環、未採用の祖先、完了済みの未完了依存先）、それらの修復（`Reopen` による修復、免除を使った修復）、完了と解放の衝突で未完了側を選んで再着手すること、取りやめと着手の衝突、文面の衝突、両側の Note、同じ ID の二重登録とその解決、各操作の到達を観測する。

### `record_integration_paths`

`divergeAndResolve` は両 replica が Issue 1 だけを操作して全部取り込みと解決を繰り返す（別 actor と同 actor の並行 `Start`、並行 `Accept`、完了と解放、解決記録どうしの並行）。`breakAndRepair` は着手・完了・再開・取りやめ、移動、dependency の追加、Group への登録と全部取り込みを選ぶ。`crossMoves` は Group 0 と 2 を互いの下へ移す操作と全部取り込みだけを選ぶ。`reopenUnderDependent` は Group 2 を完了して Issue 1 の dependency にし、片側の完了と他側の `Reopen` を組み合わせる。`reopenAfterInflow` は Issue 1 を終了して Group 0 を完了し、他側で Group 0 に Issue 4 を登録して取り込み、Group 0 の `Reopen` で直す。

### `record_integration_test`

固定した順序の `run` が 6 本。孫が着手中の祖父 Group の `Withdraw` の拒否、解決記録だけが先に届いた replica の gap と現在値、終了した Group どうしの循環の取り外しによる修復、解決の選択で作った完了済みの相互依存の dependency 削除による修復、二重登録の解決、cherry-pick による偽の衝突とその解決を確認する。後の 4 本は独立 review が見つけた経路。

## モデリングで見えた edge case

- **同じ衝突を両側で別々に解決すると解決記録が並行になる。** 同じ値を選んでいても再び衝突として見え、もう一段の解決で収束する。増殖はしない。
- **cherry-pick や revert で親記録が欠けた記録が入ると、偽の衝突になる。** 受け手が古い祖先の記録を持っている普通の場合、その祖先と新しい記録が並んで head になる。記録は親しか知らないので、実際には祖先どうしだと判定できない。新しい方を選んで解決すれば正しい値になり、後で残りの記録が届いても値は変わらない。gap を作るのは、Axon の状態を触った commit を後から部分的に取り消す（revert、rebase での drop）か拾う（cherry-pick）操作に限られ、squash merge と rebase merge では起きない。code と Axon の状態を同じ commit に入れる運用は無関係。
- **解決記録だけが先に届いた replica でも現在値は決まる。** 選ばれた側の親が無くても解決記録が値を持つので、gap として報告しつつ読める。
- **Git は構造の違反を止めない。** 片側が Group を `Complete`、他側がその Group に子を登録した場合、両側の移動で循環ができた場合、片側が完了しつつ他側でその依存先を `Reopen` した場合、片側で Group を未採用の下へ移し他側でその配下を完了した場合は、取り込みだけで違反になる。いずれも次の読取で検出でき、`Reopen`・取り外し・dependency の削除・再採用で直せる。
- **dependency の追加と完了は黙って混ざらない。** dependency は依存元の Entity の記録なので、片側で dependency を足し他側でその Entity を完了すると、同じ Entity の値の異なる head になり衝突として見える。完了済みの Entity に未完了の dependency が黙って入るのは、依存先を他側で `Reopen` した場合に限る。
- **解決の選択が違反を作ることがある。** 両側が互いを依存先にして完了していると、両方の dependency 側を選べば完了済みの相互依存になる。免除された dependency の削除で直す。
- **二重に絡んだ dependency の循環は一手では違反が減らない。** 両側で足した dependency が取り込みで絡み、直接の dependency と祖先の dependency の二経路で互いを待つ形になると、どの一手も違反の集合を縮めない。dependency を一本ずつ外せば直る。修復可能性の検査はこのため辺の数も進み具合に数える。
- **終了した Group どうしの循環は取り外しで直す。** 循環の中では互いの下への移動が拒否され、親が終了しているので通常は取り外しもできないが、違反に含まれる Entity には免除が働く。
- **完了と解放の衝突で未完了側を選べる。** 解決記録の後に通常の `Start` が続く。`Completed` からの再開経路ではない。
- **再開は依存元と祖先の側から順に行う。** `Completed` の依存元がある Entity は `Reopen` できず、依存元を `Reopen` するにはその祖先が採用済みである必要がある。数段の `Reopen` を要する場合がある。
- **`Undecided` の祖先の下の採用済み Group は `Blocked` として一覧に出る。** 祖先そのものは `Undecided` なので一覧に出ない。

## 再現手順

repository root から実行します。seed は固定しません。`quint run` は bounded random simulation で、検査の成功は全 invariant に反例がなく、0 が期待の witness を除く全 witness が 1 trace 以上で観測されたことで判定します。

```sh
for f in spec/redesign/*.qnt; do quint typecheck "$f"; done
quint test spec/redesign/record_integration_test.qnt --match Scenario
```

```python
from pathlib import Path
import re
import subprocess

def names(path, prefix):
    return re.findall(rf"val ({prefix}\w+)\s*=", Path(path).read_text())

ri = "spec/redesign/record_integration.qnt"
main_witnesses = ["wResolved", "wGap", "wGapFilled", "wSpuriousGapConflict", "wPrefixSync", "wSingleSync",
                  "wConverged", "wCompleteVsRelease", "wCancelVsStart", "wTextConflict", "wNotesFromBoth",
                  "wDuplicateCreation", "wDuplicateResolved", "wTerminalGroupGainsChild", "wUnadoptedAncestor",
                  "wRepairedAfterSync", "wWaivedRepair", "wAccept", "wStart", "wComplete", "wReopen", "wCancel",
                  "wReconsider", "wWithdraw", "wRelease", "wEdit", "wMove", "wAddDep", "wRemoveDep", "wCreate",
                  "wGroupCompleted"]
subprocess.run(["quint", "run", ri, "--invariants", *names(ri, "inv"), "--witnesses", *main_witnesses,
                "--max-samples", "1000", "--max-steps", "40", "--backend", "rust", "--verbosity", "1"], check=True)
for step, samples, steps in [("divergeAndResolve", 400, 40), ("breakAndRepair", 200, 40),
                             ("crossMoves", 200, 12), ("reopenUnderDependent", 300, 30),
                             ("reopenAfterInflow", 300, 20)]:
    subprocess.run(["quint", "run", "spec/redesign/record_integration_paths.qnt", "--step", step,
                    "--invariants", *names(ri, "inv"), "--witnesses", *names(ri, "w"),
                    "--max-samples", str(samples), "--max-steps", str(steps), "--backend", "rust", "--verbosity", "1"], check=True)
```

`record_integration` の本実行は 1 trace あたりの計算が重く、1,000 traces で 15 分前後かかります。補助探索が担う witness は本実行の一覧から外し、状態ごとの評価を減らしています。

## 検証結果

いずれも bounded random simulation の結果で、全状態の証明ではありません。seed は固定していないので、観測される trace 数は実行ごとに変わります。

以下は 2026-09-24、Quint 0.32.0、Rust backend、並列実行、seed 未固定での、独立 review の finding を反映した後の結果です（コア側の取り込み前、6 file 構成での実行）。6 file の型検査と `record_integration_test` の 6 本の `run` はいずれも成功しました。コア側の取り込み後の結果は [spec/README.md](../README.md) にあります。

- `record_integration` の本実行: 1,000 traces、最大 40 steps（約 8 分）。16 invariant に反例はなく、本実行に指定した 33 witness のうち 30 を観測しました。未観測の 3 つ（完了と解放の衝突、未採用の祖先、免除を使った修復）は補助探索で観測しています。最も少ないのは取りやめと着手の衝突で 5 traces です。値の等しい並行記録の衝突は 145、その解決は 27、二重登録の衝突は 392、その解決は 28、gap による偽の衝突は 87 で観測しました。
- `record_integration` の補助探索: 16 invariant を指定して反例なし。`divergeAndResolve` は 400 traces、最大 40 steps で、別 actor の並行 `Start` の衝突 6、値の等しい並行記録の衝突 345 とその解決 340、完了と解放の衝突 2、未完了側の選択 10、解決後の再着手 63、解決記録どうしの並行が同値 58・異値 38、解決の解決 88。`crossMoves` は 200 traces、最大 12 steps で包含の循環 86。`reopenUnderDependent` は 300 traces、最大 30 steps で完了済みへの未完了 dependency の流入 33、修復 16。`reopenAfterInflow` は 300 traces、最大 20 steps で終了 Group への子の流入と `Reopen` による修復 6、完了済みの子を持つ Group の `Reopen` 5。`breakAndRepair` は 100 traces、最大 40 steps（約 11 分）で、通常完了経路の循環 60、免除を使った修復 11、包含の循環 5、未採用の祖先 3。
- 本実行と補助探索を合わせ、43 witness はすべて 1 trace 以上で観測しました。

規則や検査を直す前の実行で出た反例は、上の「モデリングで確定した規則」と「edge case」に取り込みました。コア側のモデル（現在の `spec/group_lifecycle.qnt`）では、五つの状況で尽くせない Group の行、`Undecided` の祖先の下の実効 `InProgress`（空の Group の完了、完了済み Entity の移動、取りやめ・見送り・再検討・採用の組合せ）、理由のない `Blocked`、近傍が狭すぎた行き止まり検査の偽陽性の 4 件。`record_integration` では、孫が着手中の祖父 Group の `Withdraw`、親より先に届いた解決記録を親の値と照合していた検査、二重に絡んだ循環で一手では違反が減らない修復可能性の検査の 3 件。独立 review からは、実効 `InProgress` の Group の祖先を統合モデルの全体検査が見ていなかったこと、Issue の `Complete` で不整合を正常化できたこと、構造の不整合が無関係な操作を止めていたこと、終了した Group どうしの循環と完了済みの相互依存が直せなかったこと、二重登録で replica が凍ること、空虚な invariant と名前とずれた witness を取り込みました。

## 対象外と限界

- bounded random simulation の結果であり、全状態の証明ではない。
- `record_integration` は file の bytes、Git の union 属性が行をどう残すか、途中で切れた行、lock、探索を扱わない。これらは Rust の fixture で検査する。GitHub の web merge が union 属性を尊重するかは実験で確かめる。
- `record_integration` は Entity 5 件・replica 2 つ・最大 40 step の範囲で、3 replica 以上や長い分岐は探索していない。未登録の枠は 1 つで、登録は 1 trace に 1 回。
- `record_integration` の修復可能性の invariant は「修復を進める操作が一つはある」ことだけを見る。有効な状態まで戻れることは、進み具合が有限で単調に減ることから従うが、テストの経路以外で直接は確かめていない。
- 変換操作と再浮上条件・一覧の状況は `spec/group_lifecycle.qnt` だけが扱い、統合との組合せは探索していない。
