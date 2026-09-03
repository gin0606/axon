# Entity 情報モデルの整理

**位置づけ**: 2026-09-03 時点の作業記録。

これは現在の実装仕様や将来を拘束する決定ではない。追加情報を Entity の
definition と分けて扱う検討から出発し、問題をどう分解し、どこまでを一つの
設計問題として捉えたかを残す。今後この整理を採用、変更、または破棄するときは、
結論だけでなく、どこを変えたかとその理由を記録するための比較対象として使う。

表に出る名前、コマンド構文、出力文言、表示順、実装の分割はこの記録に含めない。

## 発端

調査結果、申し送り、実装中に判明した制約などを既存 Entity に追加するには、
現在の description を読み、既存内容を含む全文を組み立て直して書き戻す必要がある。
この方法では次の意味が混ざる。

- その Entity が何であるか
- その Entity について後から分かったこと

全文の read-modify-write は、長い本文を読み直すコストだけでなく、読み落としや
並行更新によって既存内容を失う可能性も持つ。definition の変更履歴だけを追加しても、
補足情報のたびに definition 自体を変える構造は残る。一方、追加情報の別経路だけを
用意しても、決定済み definition を同じ意味のまま書き換えられるなら、両者の境界は
利用者の使い分けだけに依存する。

したがって、追加情報、definition の変更制御、definition の履歴を個別に検討せず、
axon が持つ情報全体の中で責務を整理する。

## 現行実装から使える区分

2026-09-03 時点の main には、今回の整理に使える次の区分がある。

- Issue と Group は、共通の Entity identity、title、description、Progress、
  Disposition、Resurface condition、claim を持つ
- plan declaration の export / import は、title、description、parent Group、
  outgoing dependency を編集対象にする
- 同じ declaration では、Progress、Disposition、Resurface condition、claim、
  incoming relationship を observed な読み取り専用情報として扱う
- Disposition と Resurface condition の変更理由は判断履歴に残す
- Progress の遷移は判断履歴と分けた進行履歴に残す
- ready、blocked、orphaned、active scope などは保存せず、現在の保存情報から導出する
- declaration の fingerprint は stale な編集の適用を拒否するが、意味的な lock や
  変更履歴ではない
- 状態変更に結び付かない自由記述の追加記録は持たず、作業結果や通常の申し送りを
  description に残している

判断履歴と進行履歴を分けた設計は、記録を一つの汎用イベント列に畳まず、何についての
情報かによって保存と参照の意味を分ける先例になる。

## 達成したい状態

既存 Entity の definition や状態を変更せず、その Entity に関して後から得られた情報を
独立して蓄積できる。後から Entity を理解する利用者には、definition、状態、履歴、
追加情報が意味を混ぜずに観測可能である。

axon が保証するのは、情報が欠落せず観測可能であることまでとする。利用者が情報を読み、
理解してから操作する責任までは axon が引き受けない。

## 情報の大分類

| 分類 | 役割 |
| --- | --- |
| Identity | ID と Issue / Group の kind。対象の同一性を表す |
| Plan declaration | Entity が何で、どこに属し、何を前提とするかを宣言する |
| Control state | Progress、Disposition、Resurface condition、claim により現在の扱いを表す |
| Derived facts | ready、blocked、orphaned、active scope など、保存情報から機械的に計算する |
| Typed history | 判断または進行という特定の操作について、過去の事実と理由を残す |
| Supplemental information | 状態変更に結び付かず、Entity について後から得られた情報を蓄積する |

優先度や表示上の強調など、現在 axon が持たない概念を、この整理だけを理由に追加しない。
A / B / C / D の意味や既存の導出規則も、情報の統治に必要な矛盾が見つからない限り
再定義しない。

## 整理のアプローチ

各機能へ個別に revision、lock、追加経路を足すのではなく、情報の分類ごとに次を決める。

- 何のための情報か
- 現在値と過去の記録のどちらを正とするか
- 置換、状態遷移、追記のどの操作に属するか
- 変更履歴、actor、時刻、理由のどれを必要とするか
- Entity のライフサイクルによる変更制限が必要か
- axon が保証する範囲と利用者に委ねる範囲
- 他のどの情報と一緒に観測されるべきか

Plan declaration と observed state を分ける現行の宣言ファイルを出発点にする。一方で、
すべての変更を一つの汎用イベントとして扱う event sourcing には広げない。判断履歴と
進行履歴を分けた理由が失われ、既存の状態モデルと永続化を全面的に作り直すためである。

上位の統治契約は一つに揃えるが、保存先、利用者向けの操作、実装する Entity は分割して
よい。実装が分かれていることと、意味の正が複数箇所に散っていることは区別する。

## 現時点の具体像

以下は上記の分類から導いた 2026-09-03 時点の作業仮説であり、まだ実装仕様ではない。

### Plan declaration の所有範囲

各 Entity は次を自身の plan declaration として所有する。

- title
- description
- parent Group
- outgoing dependency

incoming dependency と子 Entity は、関係の反対側にある Entity が所有する。Group に
子を追加する場合、変更される declaration は parent を持つ子側であり、Group 自身が
所有する declaration ではない。

### 決定済み declaration の固定

- Disposition が Undecided の Entity は、自身の declaration を編集できる
- Accepted または Rejected の Entity は、自身の declaration を変更できない
- 決定済み declaration を変える場合は、採否を Undecided に戻し、編集後に改めて
  採否を判断する
- Progress はこの規則と直交し、InProgress や Ended でも同じ規則を適用する
- 現在値と同じ値を再指定する no-op は declaration の変更ではないため成功してよい
- declaration の全変更経路に同じ制約を適用し、一部の経路から固定を迂回できない
- supplemental information の追加は declaration の変更ではないため、固定中も許可する

Rejected も固定するのは、不採用も特定の declaration に対する判断だからである。
不採用のまま declaration を変更できると、何を不採用にしたのかが現在値から失われる。

この固定は権限制御ではない。同じ actor が Undecided へ戻して編集し、再び採否を判断する
ことを禁止しない。決定済み declaration の変更を採否の再検討として露出させる、意味上の
ガードとして捉える。

### 決定済み declaration の記録

- Accepted または Rejected と判断された declaration の全文 snapshot を残す
- snapshot は、その Entity が所有する plan declaration 全体を含む
- 判断履歴から、各判断がどの snapshot に対するものかを特定できる
- Undecided 中の編集過程は逐一履歴化しない
- declaration が変わらず採否だけが変わった場合は、同じ snapshot に別の判断を結び付ける
- 前回と次回の決定済み snapshot の全文および差分を観測できる
- migration 前の履歴は復元せず、既存 Entity の現在値を baseline として扱う

この形では、すべての write を履歴にするのではなく、採否判断の対象になった declaration を
記録する。Undecided の間は draft として編集でき、決定時点の内容を後から検証できる。

### Supplemental information

- Issue と Group の両方に追加できる
- Progress、Disposition、Resurface condition によって追加を制限しない
- 追記専用とし、一度追加した記録は通常操作で編集または削除しない
- 誤りや前提変化は、既存記録の置換ではなく新しい記録として追加する
- 各記録は対象内で安定して参照できる識別子、本文、actor、時刻を持つ
- 追加は原子的に行い、並行した追加を上書きしない
- 繰り返し実行は同じ記録への no-op ではなく、新しい記録の追加として扱う
- 保存順を正とし、並行操作で前後しうる入力時刻から順序を推測しない
- declaration、Control state、関係、Derived facts を変更しない
- 記録の古さから重要度を推定しない

訂正元を指す専用の関係は持たず、安定した識別子を本文から参照できればよい。通常操作での
編集や削除を許さないことで、definition と同じ変更履歴問題を追加記録側へ持ち込まない。

### 既存履歴との境界

- Disposition と Resurface condition の理由は、現在どおり判断履歴に残す
- Progress の操作は、現在どおり進行履歴に残す
- 決定済み declaration の snapshot は、両者と異なる意味の記録として扱う
- supplemental information は、状態変更に結び付かない追加記録としてさらに分ける
- どの記録も、別の意味を持つ履歴へ無条件に混ぜない

### 観測

- Entity の詳細表示から、付与された各種記録の件数を観測できる
- supplemental information は省略せず全件を観測できる
- 古い supplemental information を暗黙に低い重要度として扱わない
- 状態変更の操作は、利用者が事前に Entity の詳細を理解していることを前提にし、
  supplemental information の再掲や既読確認を担わない
- 出力は保存事実と導出事実を示し、次の行動を案内しない

## axon が保証しない範囲

- actor の本人性
- DB を直接編集した場合の耐改ざん性
- 入力内容が plan declaration と supplemental information のどちらに属するかの自動判定
- 利用者が情報を読んだか、理解したかの確認
- Undecided 中の複数利用者による declaration 編集の自動マージ
- 誤投入した機密情報を履歴全体から削除する redaction

## この記録に含めなかったもの

概念と挙動の整理後に決める次の事項は、意図的に記録対象から外した。

- supplemental information と declaration snapshot の公開名称
- 追加、一覧、個別参照、差分参照を行うコマンド構文
- 出力の固定文言、ブロックの位置、並び順
- 公開識別子の具体的な表記
- schema、内部型、migration の実装方法
- 実装作業の分割、依存関係、着手順

## 関連する既存 Entity

- `axon-eq8xys`: definition を変更せず追加情報を蓄積する経路の検討
- `axon-t5ztjc`: Entity definition の変更を後から検証できない問題

この記録は両者の採否や統合方法を決めない。各 Entity の扱いは、ここで整理した上位の
問題と保証境界を参照して改めて判断する。
