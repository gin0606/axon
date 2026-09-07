# 状態モデル

Entity、Progress、Disposition、Resurface condition、関係と導出値の規範契約。
情報の所有と変更制限は [情報モデル](information-model.md)、コマンドの入出力は
[CLI 契約](cli.md)、設計理由と代替案は [設計判断](../design/decisions.md) を参照する。
設計変更とモデル検証は [検証方針](../development/verification.md) に従う。

## Entity と独立した軸

Entity は `Issue` または明示的な計画範囲を表す `Group` である。
両 kind は identity、title、description、次の軸と claim を共有する。

| 保存する情報 | 値・意味 |
| --- | --- |
| Progress (A) | `NotStarted` / `InProgress` / `Ended`。これ以上作業を進めるか |
| Disposition (B) | `Undecided` / `Accepted` / `Rejected`。現在の採否判断 |
| Resurface condition (C) | `Always` / `AtDate` / `AfterEntity` / `Manual` / `Command`。いつ再び意識に上げるか |
| Dependency (D) | Entity 間の成果物・計画完了を必要とする前提関係 |
| 包含 | Group だけを親にできる一親 tree |

`Ended` は「やり切った」だけでなく「もう進めない」を表す。Ended × Accepted は完遂、
Ended × Rejected は打ち切り、Ended × Undecided は調査終了後の判断待ちとして読める。
採否は終了後も変更できるが、後述の Ended Group 配下の制約に従う。Ended は再開しない。

中断専用の Progress は持たない。InProgress のまま再浮上条件を設定できる。
Rejected から Accepted への再判断にも特別な経路は設けない。
一つの状態操作が Progress と Disposition を暗黙に同時変更することはない。

Resurface condition の成立は保存状態を書き換えず、浮上時刻の履歴や通知フックも作らない。
各条件の意味は [Resurface condition](#resurface-condition) で定める。

優先度、pin、色、表示順を状態モデルの保存値として持たない。
表示方法は導出値を消費する側の関心であり、導出値の定義はカスタマイズしない。

## Resurface condition

| 条件 | 保存する付随情報 | 成立条件 |
| --- | --- | --- |
| `Always` | なし | 常に成立 |
| `AtDate` | 日付 | 現在のUTC日付が指定日以降 |
| `AfterEntity` | 参照先 Entity | 参照先が terminal |
| `Manual` | なし | 設定中は常に未成立。明示的な置換・解除を待つ |
| `Command` | シェル文字列 | 外部コマンドの観測結果が成立を示す |

条件の設定・置換・解除は Issue / Group、すべての Progress / Disposition で同じ規則に従い、
Progress、Disposition、claim を変更しない。解除は `Always` への変更であり、自動採用や
自動 start を意味しない。候補への復帰は ready / triage の他の要件にも従う。

Command の成立は非単調で、次回の観測で未成立に戻ることがある。成功しても `Always` に
書き戻さず、評価結果を保存しない。判定失敗は未成立と区別し、triage の第三分類にしない。
実行環境、終了コード、評価の共有と失敗時の操作は [CLI 契約](cli.md#外部条件の評価) で定める。

Group が非浮上になると activation gate が閉じ、子孫は active scope から外れる。
進行中の子孫を含め、子孫の保存状態と claim は変えない。Manual / Command は Entity への参照辺を
持たず、AfterEntity への置換には既存の待機グラフ制約を適用する。
再浮上は再検討・着手候補へ戻す意味であり、計画や外部前提の妥当性を保証しない。

## Identity と claim

Issue と Group は同じ公開 ID namespace を使い、kind を ID に埋め込まない。
ID の表記・解決は [CLI 契約](cli.md)、生成方法と actor の取得は
[アーキテクチャ](../development/architecture.md) で定める。

claim は InProgress のときだけ存在し、独立した予約状態を持たない。
Group の claim は子孫を lock せず、Group と子孫を別 actor が同時に claim できる。
actor と worktree は release の事前条件ではない。claim の経過時間やプロセス状態から
staleness を推定せず、保存された事実を確認して明示的に release する。

## 包含と active scope

包含は、各 issue / group が最大 1 つの親 group を持つ tree とする。group だけが親になれ、深さは制限しない。複数親による活性・採否の矛盾を避けるため一親にし、共有成果物は dependency、横断分類は query や将来の tag で表す。

group の activation gate が開いて子孫を active scope に入れるのは、次をすべて満たす間である。

- `Progress=InProgress`
- `Disposition=Accepted`
- 自身が surfaced
- 自身または祖先 group の dependency によって blocked / orphaned ではない

Entity は全祖先 group の activation gate が開いているときだけ active scope 内になる。root Entity は祖先を持たないため常に active scope 内で、自身の Progress / Disposition などは ready と triage が別に判定する。したがって、親 group が NotStarted、後送り中、Rejected、blocked、orphaned の間、配下の保存状態は変えず ready と triage から外す。子を start して親を暗黙に start する操作は持たない。

`group set <entity-id> <parent-group-id>` は親の新規設定または移動、`group unset <entity-id>` は親の解除を意味する。包含変更は一親制約、Ended group の構造固定、進行中の子を NotStarted group の下へ移さない制約、待機グラフの非循環を 1 transaction で検査してから適用する。

## 状態遷移

状態遷移が成功するのは、次の遷移だけとする。

| コマンド | 事前条件 | 遷移先 |
| --- | --- | --- |
| `start` | ready な未着手 | 着手中 |
| `done` | 着手中。group は加えて全子孫が終端 | 終了 |
| `release` | 着手中。group は加えて着手中の子孫がない | 未着手 |
| `decide` | 指定した採否と現在値が異なる | 指定した採否 |
| `when` | 指定した条件と現在値が異なる | 指定した条件 |

これらの遷移は対象 Entity 自身に作用し、子孫を自動 start / done / release しない。
採否変更は Ended Group 配下の terminal 制約に、条件変更は待機グラフの非循環に従う。
start は ready の検査と claim の取得を、done / release は事前条件の検査と進行更新を、履歴の記録と同じ transaction で行う。done / release は claim を解放する。

group の完了は子孫から自動導出しない。最後の子が terminal になった直後も group は InProgress のままで、利用者またはエージェントが計画全体を確認して `done` する。空の InProgress group は全子孫が terminal という空集合上の条件を満たすため明示 done できるが、空であるだけでは自動終了しない。

子 group の `Disposition=Rejected` は terminal なので、親の完了判定でその子 group 自身を妨げとして数えない。ただし親の条件は「全**子孫** Entity が terminal」であり、Rejected group の下に非 terminal な Entity が残っていれば、それらは別に終端化または包含外へ移す必要がある。Rejected は子孫へ伝播しないという原則と、Ended group の全子孫を terminal に保つ不変条件を両立させるためである。

## Rejected と Ended の境界

group は Rejected になった時点で Progress にかかわらず terminal であり、group 自身を完了させる追加操作は必要ない。子孫の Disposition / Progress は変更せず、Rejected の祖先 group 配下は active scope 外になり、保存状態を保ったまま ready と triage から外れる。非 terminal な子孫はそれだけで将来作業や後片付けを要求せず、Accepted の祖先 Group の完了条件や保存済み claim・外部作業に処遇が必要な場合だけ個別に判断する。これは dependency の前提喪失ではないため、包含だけを理由に orphaned とは呼ばない。

一方、dependency の依存先 group が Rejected なら、その成果または計画単位が得られないため依存元は orphaned になる。依存元が group なら group 自身と全子孫に作用するが、triage は判断 frontier の group だけを表示し、配下を一件ずつ並べない。

Ended group には再 open を用意しない。次を禁止して、完了宣言を後の操作で無効化せず、すでに解放した dependent を再び塞がない。

- Ended group 自身の親変更、および subtree への追加、subtree からの削除、subtree 内外への移動
- Ended group を依存元とする dependency の追加・削除
- Ended group の子孫を terminal から非 terminal に戻す Disposition 変更

Ended の Entity も declaration 固定について他の Progress と同じ規則に従う。title / description を訂正する場合は Undecided に戻して draft を編集し、改めて採否を判断する。Progress は Ended のままであり、Ended group の構造固定と子孫の terminal 制約も維持する。完了後に見つかった追加作業は、Ended group の外に新しい issue または group として作る。

## Dependency と Resurface condition

dependency は issue→issue、issue→group、group→issue、group→group の全組み合わせで同じ hard prerequisite を表す。

- 依存先が `Disposition=Rejected` なら、Progress にかかわらず依存元は orphaned
- Rejected でない依存先が `Progress=Ended` なら解消
- それ以外なら依存元は blocked
- 依存元が group なら、その group 自身と全子孫へ同じ結果を作用させる

包含とは違い、dependency は成果または明示完了した計画単位を待つ関係である。group の子孫がすべて終端でも group 自身を明示 done するまでは、その group を待つ dependency は解消しない。

`AfterEntity` の主体と参照先も issue / group の全組み合わせを許す。参照先が Ended または Rejected なら surfaced になる。参照先の Rejected で orphaned になる dependency と、「待つ時期が終わった」と解釈して surfaced になる Resurface condition の非対称は維持する。InProgress group を後送りすると activation gate が閉じ、子孫も ready / triage から外れるが、子孫の保存状態は変えない。

## Deadlock を作らない関係更新

dependency、`AfterEntity`、包含を別々の DAG として検査しても、関係を横断する循環は見つからない。そこで 2 つの待機グラフへ射影し、どちらかが循環する追加・置換を拒否する。

| graph | 包含辺 | 検出する代表例 |
| --- | --- | --- |
| activation wait | child → parent | start 前の group が、自身の active scope 外の子孫を dependency / when で待つ |
| completion wait | parent → child | 子孫が祖先 group の明示完了を dependency / when で待つ |

group を依存元または when の主体にした論理辺は group の全子孫へ展開する。これにより直接の 2-cycle だけでなく、複数の dependency / when / 包含をまたぐ間接循環も同じ非循環条件で拒否できる。削除と解除は循環を増やさないため、既存の問題を解消する経路を妨げない。

`Progress=Ended` の Entity は再開しないため、循環検査の node から外す。

## 導出値

次の値は保存せず、読み取り時に保存済み状態・関係と、必要な Command の外部観測から計算する。

| 名前 | 定義 |
| --- | --- |
| active scope 内 | 全祖先 group の activation gate が開いている。root Entity は常に active scope 内 |
| group activation gate | group 自身が `InProgress` / `Accepted` / surfaced で、blocked でも orphaned でもない間だけ子孫を開く |
| 着手可能 (ready) | active scope 内かつ A=未着手 かつ B=採用 かつ C の条件を満たす かつ blocked でない かつ orphaned でない |
| 阻害中 (blocked) | Entity 自身または祖先 group の依存先に未終端 (A≠Ended かつ B≠不採用) のものがある |
| 前提喪失 (orphaned) | Entity 自身または祖先 group の依存先に B=不採用 のものがある (依存の鎖は**推移しない**) |
| 浮上中 (surfaced) | Resurface condition が満たされている |
| 終端 (terminal) | A=`Ended` または B=`Rejected` |
| triage frontier | 非終端かつ自身が surfaced かつ active scope 内で、B=未判断または orphaned の Entity。gate が閉じた group の子孫は含めない |
| group 完了可能 | group 自身が `InProgress` で、全子孫 Entity が terminal |
| 阻害の根本原因 (blocking cause) | Entity 間依存だけを遡り、前提喪失 / 後送り中 / active scope 外 / それ以上依存を持たない Entity に到達した地点。包含による非 active の理由は別に示す |

blocked と orphaned は依存の鎖を推移させず、Entity 自身と祖先 Group が所有する直接の依存先を見る。根本原因の探索は blocking cause が担う。

### Observed 情報と triage frontier

group の保存済み Progress と、配下の状況は別々に表示する。最低限、次は保存せず読み取り時に導出する。

- 直下と全子孫について、Entity 総数、kind 別件数、Progress / Disposition / terminal の件数
- blocked / orphaned / active scope 外の理由
- InProgress の子孫数
- group を現在 done できるかと、妨げている非 terminal 子孫

`triage` は次の4条件をすべて満たす Entity だけを返す。

1. 非 terminal である。
2. 自身の Resurface condition が成立している（surfaced）。
3. 全祖先 Group の activation gate が開き、active scope 内にいる。
4. Undecided または orphaned である。

root Entity は常に active scope 内だが、自身が Manual なら未浮上のため含まれない。一方、祖先 gate が閉じている子 Entity は、自身が surfaced でも active scope 外のため含まれない。親 Group が Undecided / orphaned なら子孫の gate は閉じ、親自身も残りの条件を満たす場合だけ frontier に入る。子が active scope 内になり、自身も surfaced なら、Undecided / orphaned の子が対象に入る。

非表示は Entity の不存在や作成・更新の失敗を意味しない。非表示だけを根拠に作成を再実行せず、管理 root 全体の棚卸しには `list`、個別の保存状態と未浮上・inactive 等の理由には `show` を使う。`ready` は着手候補、`status` は進行中の作業や待ちを含む状況把握を担い、`status` は全件一覧ではない。
