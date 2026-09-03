---
name: axon-triage-issue
description: ユーザーが選んだ既存 axon Entity の採否相談、判断見直し、または plan declaration 編集を扱い、合意後に採否、title、description、parent、outgoing dependency、分解を反映する。Undecided の通常編集と、Accepted / Rejected / Ended の再判断を伴う編集にも使う。独立した Note 追記やユーザーの結論なしの採否決定には使わない。
---

# Axon Entity の判断と plan declaration を相談する

指定された Issue または Group の判断材料を整え、ユーザーとの合意を axon に反映する。
分析と提案は自律的に行ってよいが、採否や構造の変更はユーザーの結論後に行う。

## 共有 DB への書き込み権限

Git linked worktree では共有 `.axon` が作業ディレクトリの外に置かれることがある。状態変更 command が共有 DB への書き込み権限不足で拒否された場合は、その command だけを実行環境の許可機構で再実行する。読み取り・探索 command や他の program まで権限を広げず、再実行できなければ状態を推測せずに拒否と未反映を報告する。

## 調査

1. `axon show <id>` の出力を省略せず、kind、plan declaration、Control state、記録件数、表示された全 Note、依存、後続、親 Group、Group なら子孫集計と完了条件を読む。
2. `axon revision list <id>` と各 `axon revision show <id> <number>` で全 Declaration Revision を読む。判断の経緯は `axon log <id>` で確認し、declaration の変更内容を比較する場合は `axon revision diff` も使う。
3. 依存先、後続、親子関係が判断に影響する場合だけ、その Entity も確認する。
4. 未判断、前提喪失、Accepted からの不採用化、Rejected / Ended の再検討を区別する。前提喪失では、依存を外して続ける案と、Entity 自体を不採用にする案を少なくとも検討する。既存判断を変える場合は履歴と現在の Progress / claim を確認する。Ended は reopen できず Progress は変わらないが、Disposition の変更は dependent の satisfied / orphaned を切り替え得るため、全 kind の dependent と frontier への影響を判断前に確認する。追加・再実施する作業は新しい Entity として作り、Ended Group の外に置く。
5. Disposition の変更を提案する前に、対象 kind や Progress を問わず、現在値と変更後で terminal / Rejected がどう変わるかを比較する。直接または Group から継承する dependent、reverse `AfterEntity` waiter、Group 子孫、active scope、ready / triage frontier への影響を確認し、選択肢と一緒に提示する。reverse `AfterEntity` query はないため、`axon list` の全 ID を列挙してそれぞれ `axon show <id>` を実行し、対象を参照する `Resurface condition: AfterEntity(...)` を探す。特に Rejected から Accepted / Undecided へ戻す前は必須とし、見つけた waiter と Group 子孫を変更後にも再確認する。

調査中は `axon show`、`axon log`、`axon triage`、`axon list` などの読み取りだけを行う。関連する Group も共通の `axon show <group-id>` で確認する。
Entity の作成や既存状態の変更は行わない。

## 相談

次をユーザーが比較できる形で提示する。

- 何を判断する Entity か
- 現状維持を含む現実的な選択肢
- 各選択肢が進行、採否、時期、依存、active scope と triage frontier へ与える影響
- 推奨案と、その理由や不確実性
- 判断に先立って追加で確かめる必要があること

提案しただけでは合意とみなさない。ユーザーの結論が曖昧な場合は、状態を変える前に確認する。
採用に合わせてエージェントが目的、対象、完了条件、分解、依存へ実質的な判断を加える場合は、採否や構造を変える前に最終案を提示する。誤字修正や意味を変えない整形だけなら再確認しなくてよい。

採用は「やる」という判断であり、実装に必要な設計成果物がすべて揃ったという意味ではない。採用後の issue は、前提がなければ `ready` に出る。目的、スコープ、利用者から見える振る舞い、公開する契約、状態モデル、後戻りしにくいコストを変える未決事項が残る場合は、次のどちらにするかも相談する。

- 採用を反映する前に、この相談で未決事項を解消する
- 採用自体は反映し、設計成果物を作る別 issue を前提として実装 issue に依存させる

通常の実装詳細は実装者に委ねる。既知の重要な前提だけを本文に残して、依存のない実装 issue を着手可能として引き渡さない。

InProgress Entity の Disposition を変えても Progress / claim は変わらないため、判断時に claim の扱いも合意する。作業を継続するなら既存 owner の claim を維持する。一時中断または引き渡しなら、実質的な申し送りがある場合に [`axon-add-note`](../axon-add-note/SKILL.md) で現在地、残作業、検証状況を Note に追加・確認してから、`axon release <id> -r <reason>` を状態変更 command として単独で実行する。恒久的に停止するなら、後から参照すべき作業結果がある場合だけ同 skill で Note に追加・確認してから `axon done <id>` を単独で実行する。release / done が失敗または成否不明でも確認済み Note を重複追記せず、Note 番号と現在状態を報告する。いずれも `axon show <id>` の全出力と `axon claims` で結果を確認する。

Group の採否は子孫の保存状態を変更しない。Group を Rejected にする案では、配下が active scope 外になり、その Group を dependency target とする Entity は orphaned、`AfterEntity` で待つ Entity は surfaced になり得ることを明示する。NotStarted なら Rejected のままで terminal であり、start、release、done は行わない。ただし祖先 Group の完了に非 terminal 子孫の整理が必要なら、その扱いを合意する。InProgress Group を release する前は InProgress の子孫が 0 件であることを確認し、恒久的に打ち切って done にする前は、子孫を独断で変更せず、合意に従って全子孫を terminal にするか包含外へ移す。Accepted にして start する案では配下の frontier が段階的に現れることを明示し、包含の変更が計画範囲と完了条件へ与える影響も選択肢に含める。

## 合意の反映

ユーザーの結論に必要な操作だけを行う。

- 採用・不採用・未判断への戻しは `axon decide ... -r <理由>` で行う。時期の変更は `axon when ... -r <理由>` で行い、いずれも状態変更 command として単独で実行する。reason は状態変更の理由であり、同じ内容の Note を作らない
- title、description、parent、outgoing dependency は plan declaration である。Undecided の実変更は採否を変えずに反映する。Accepted / Rejected の現在値と異なる変更が合意された場合は、理由付き `axon decide undecide` を単独で実行し、draft になったことを全 `show` 出力で確認してから各変更を単独で反映する。変更後の declaration 全文と関係を `axon show`、全 Revision を `axon revision list|show` で確認した後、合意した採否を理由付き `axon decide` で改めて単独反映する。既存と同じ値の指定は再判断を要しない。この複数 phase の途中で失敗した場合は再判断せず停止し、`axon show` で現在の draft 全文を確認して、反映済みと未反映の phase を報告する。すでに Undecided なら `decide undecide` を再試行しない。自動で巻き戻さず、残りの再試行または別途承認された補償変更のどちらにするかをユーザーへ確認する
- 調査結果、作業結果、申し送りなど、Entity が何であるかを変えない追加情報は description へ混ぜず、独立した依頼として `axon-add-note` で扱う
- 合意済みの実装項目や設計成果物へ分解する場合、title と任意の親 Group だけで完成する Entity は `axon plan` / `axon group plan` で作る。description、dependency、その他の後続設定がある場合は `axon capture` / `axon group capture` で Undecided として作り、文面と関係を設定・検証してから最後に理由付き `axon decide accept` を実行する。まだ判断が必要な派生事項は Undecided のままにする。親 Group が合意済みなら作成 command に `--parent <group-id>` を付け、作成と包含を原子的に行う
- 依存、親 Group、元 Entity の扱いは一意に決めつけず、合意した関係だけを設定する

判断理由には、結論だけでなく、その選択に至った制約や比較を残す。
採用した Entity の実装や計画進行は、同じ相談の中で明示的に依頼されない限り開始しない。

## 確認

1. 変更した Entity と新しく作った Entity は `axon show` の全出力を読み直し、`axon revision list` と各 `axon revision show` で全 Declaration Revision も確認する。
2. 採否、title、description、時期、outgoing dependency、親 Group が合意と一致し、既知の前提がある実装 Issue が `ready` になっていないか確認する。
3. terminal にした Entity に親 Group がある場合は InProgress の祖先 Group を順に `axon show` し、現在 done できるかと残る非 terminal 子孫を確認する。祖先 Group の進行が依頼範囲に含まれない限り、自動で done にしない。
4. `decide`、dependency の追加・削除、Resurface condition の変更、Group の activation gate や包含を変える操作を行った場合は、kind を問わず関連 Entity の `show` と `axon list`、`axon ready`、`axon triage` で dependent、`AfterEntity` 待ち、Group 子孫、frontier への波及を確認する。Disposition を変えた InProgress Entity は `axon show` と `axon claims` で Progress / claim が合意した継続、release、done の状態に達したかも確認する。
5. ユーザーに、変更した ID と kind、判断理由、作成・分解した Entity、完了可能になった祖先 Group、active scope と frontier への影響、未解決の論点を報告する。

複数 Entity の重複整理や優先順位付けを依頼された場合は、まず候補と根拠を提示する。
統合、書き換え、採否変更は、ユーザーが対象と方針を確認した後に行う。
