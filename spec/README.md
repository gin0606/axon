# Quintモデル

Axon の lifecycle、包含と dependency、Group の実効 lifecycle、候補一覧の評価、文面・Note・状態変更履歴、記録の集合の Git 統合の意味論を、実行可能な Quint のモデルとして置きます。契約は [lifecycle](../docs/reference/lifecycle.md)、[候補と外部条件](../docs/reference/candidates.md)、[保存と統合](../docs/reference/storage.md) が定義します。各モデルの対象外は各 `.qnt` の冒頭に、探索の前提は冒頭か各定義の横に、検査する性質は各定義の横のコメントにあります。ただし、到達性を補う探索入口の `lifecycle_reachability`・`record_integration_paths` と、`run` テストの `record_integration_test` は、取り込むモデルの初期状態と操作をそのまま使うため、冒頭に対象外を持たず、取り込むモデルのものに従います。

## モデル一覧

| ファイル | 対象 |
| --- | --- |
| [`lifecycle_rules.qnt`](lifecycle_rules.qnt) | Issue と Group が共有する状態5種類・操作8種類と、種類ごとの基本遷移の前提・遷移先 |
| [`issue_lifecycle.qnt`](issue_lifecycle.qnt) | 単独 Issue の lifecycle（`Reopen` を含む）と、抽象化した再浮上条件による浮上の導出 |
| [`group_lifecycle.qnt`](group_lifecycle.qnt) | Issue と Group の包含、所属変更、新規登録、種類の変換、dependency、Group の実効 lifecycle、`Reopen`、判断候補・着手候補・`axon tasks` の一覧の行と状況、`axon show` も使う詰まっている理由の導出 |
| [`lifecycle_reachability.qnt`](lifecycle_reachability.qnt) | `group_lifecycle` の到達性を補う4つの探索入口 |
| [`candidate_evaluation.qnt`](candidate_evaluation.qnt) | `axon proposals`・`axon tasks` の1回の取得と `axon show` の1回の表示における評価範囲、評価回数、結果の共有、判定失敗、条件の許可 |
| [`lifecycle_information.qnt`](lifecycle_information.qnt) | title・description の編集範囲、Note の追記、種類の変換をまたぐ履歴と現在の状態・種類の一致 |
| [`record_integration.qnt`](record_integration.qnt) | 2 replica（Git worktree）が記録の集合を任意の部分集合で持ち寄る統合、衝突の検出と解決、構造の違反の検出と修復 |
| [`record_integration_paths.qnt`](record_integration_paths.qnt) | `record_integration` の到達しにくい経路を狙う 5 つの入口 |
| [`record_integration_test.qnt`](record_integration_test.qnt) | 固定した順序で特定の経路を再現する `run` テスト |

## 検査の実行

[`scripts/check-models`](../scripts/check-models) は、全モデルの型検査、`record_integration_test` の `run` テスト、各モデルの探索を行い、下記の成功の判定を満たさなければ 0 以外で終わります。引数がなければ「検証結果」と同じ探索量ですべてを行い、1 時間を超えます。対象の検査と探索の入口、探索量は引数で選べます（`scripts/check-models --help`）。

```sh
scripts/check-models typecheck test issue_lifecycle      # 型検査、run テスト、issue_lifecycle
scripts/check-models --samples 300 group_lifecycle       # 探索量を上書きし、補助の入口も合わせて行う
scripts/check-models record_integration:breakAndRepair   # 一つの入口だけを行う
```

seed は固定しません。反例が出た場合は、出力の `Use --seed=0x… to reproduce.` にある seed を、反例の出た入口に `-- --seed 0x…` として渡すと再現できます。

## 成功の判定

`quint run` は bounded random simulation であり、exit status だけでは成功を判定できません。指定した全 invariant に反例がなく、0 が期待の witness を除く全 witness が 1 trace 以上で観測され、0 が期待の witness がどの探索でも観測されないことを成功とします。invariant と witness は、各モデルの module 直下で `inv`・`w` で始まる `val` をすべて指定します（`record_integration` の本実行だけは、計算量を抑えるため補助の入口が担う witness を外します）。

通常探索で観測率の低い witness は補助の入口が担うため、`group_lifecycle` と `lifecycle_reachability` の入口、`candidate_evaluation` の `queryStep` と `workAndList`、`record_integration` と `record_integration_paths` の入口は、それぞれ合わせて一つの検査とし、witness はいずれかの探索で観測されればよいとします。そのうち一部の探索だけを選んだときは、`scripts/check-models` は未観測の witness を判定しません。

0 が期待の witness は `group_lifecycle` の `wStalledWithoutListedReason`・`wEffectiveWorkingUnderUnadopted`・`wUnadoptedViaComplete`・`wUnadoptedViaMove`・`wUnadoptedViaAcceptOrReopen` の 5 つで、規則で塞いだ経路と、条件に依らない理由のない詰まっている Group を観測します。

## 検証結果

いずれも bounded random simulation の結果であり、全状態の証明でも、必ず完了することの保証でもありません。どのモデルも、完了への到達を強制する公平性は仮定しません。seed を固定しないため、観測される trace 数は実行ごとに変わります。ここに残すのは実行条件と判定で、trace 数は witness の到達しやすさの目安として添えます。

以下は Quint 0.32.0、Rust backend、並列実行、seed 未固定での結果です。`group_lifecycle`・`lifecycle_reachability` は 2026-10-09 に、浮上していなくても `axon tasks` に出す Group を着手中の Issue を子孫に持つものに絞った後に再実行し、`candidate_evaluation` は 2026-10-27 に条件の許可を足した後に再実行しました。`issue_lifecycle` は 2026-09-25 に一覧の Group の行の規則（`Empty` の優先、詰まっている理由の導出範囲）を直した後に実行しました。`lifecycle_information` は 2026-09-26 に種類の変換と各時点の種類での履歴の再生を足した後に実行しました。`record_integration` と `record_integration_paths` は 2026-09-26 に、所属変更・dependency の追加・登録で新しく持った関係が誘導する辺が循環上に載る変更を違反のある store でも拒否する前提を足した後に実行しました。9モデルの型検査と `record_integration_test` の `--match Scenario` の `run` は 2026-09-26 にいずれも成功し、9モデルの型検査は 2026-10-09 と 2026-10-27 にも成功しました。

- `issue_lifecycle`: 100,000 traces、最大80 steps（約7秒）。9 invariant に反例はなく、23 witness はすべて観測されました。最も少ない完了後の非浮上で37.8%です。
- `group_lifecycle`: 5,000 traces、最大150 steps（約4.5分）。44 invariant に反例はなく、97 witness のうち規則で塞いだ経路と条件に依らない理由のない詰まっている Group を観測する5つが期待どおり0で、残り92をすべて観測しました。最も少ないのは祖先の dependency の完了による着手の解禁で7 traces、次いで `Completed` の依存元による `Reopen` の拒否で10、`Reopen` 後の再着手と孫の着手による祖父の実効 `InProgress` で各12、完了済みの子を持つ Group の `Reopen` で14 traces です。完了済みの子孫だけで実効 `InProgress` の Group が浮上せず行に出ない状態は291、着手中の Issue を子孫に持ち浮上していない Group が行に出る状態は242 traces で観測しました。行に出ない Group での浮上していない祖先の理由は2,396、浮上していない着手可能な子孫の理由は1,020 traces で観測しました。自身の再浮上条件が未成立という理由は、行に出ない Group で4,596、そのうち着手可能な子孫を隠している状態で1,462、完了済みの子孫だけで実効 `InProgress` の Group で208 traces で観測しました。完了できる空の Group の `Empty` は3,412、着手中の子孫がない実効 `InProgress` の行に理由が付く状態は206、完了できない `Empty` の行に理由が付く状態は4,303 traces で観測しました。
- `lifecycle_reachability`: 同じ44 invariant を指定して反例なし。`workAndReopen`・`dependAndStart`・`listAndWork` は各300 traces、最大100 steps、`readoptAfterClose` は5,000 traces、最大60 steps。`workAndReopen` は `Reopen` 後の再着手を295、完了済みの子を持つ Group の `Reopen` を59、`Completed` の依存元による `Reopen` の拒否を298 traces で観測しました。`dependAndStart` は祖先の dependency の完了による着手の解禁を36、自身の依存先の完了による解禁を243 traces で観測しました。`listAndWork` は `Ready` と実効 `InProgress` が重なる Group を276、実効 `InProgress` の Group の移動を300、完了できる空の Group の `Empty` を300 traces で観測しました。`readoptAfterClose` では規則で塞いだ経路の witness は0のままで、完了済みの子孫だけで実効 `InProgress` の Group が浮上せず行に出ない状態を1,358、その Group に自身の再浮上条件が未成立という理由が付く状態を1,162 traces で観測しました。この入口は条件入力を変えず不成立のままなので、着手中の子孫がない実効 `InProgress` の行に理由が付く状態は観測せず、通常探索が担います。通常探索と補助探索を合わせ、0が期待の5つを除く92 witness がいずれかの探索で1 trace 以上に到達しました。
- `candidate_evaluation`（2026-10-27、条件の許可を足した後）: `queryStep` は30,000 traces、最大60 steps（約6分）。21 invariant に反例はなく、41 witness のうち `Reopen` した Issue の再表示を除く40を観測しました（未観測の1つは補助入口が担保します）。通常探索で最も少ないのは評価失敗後の着手で5 traces、次いで未成立の結果を持つ進行中の Group の表示で9 traces です。許可の witness は、自動の許可を25,060、許可しない設定を22,019、未許可の条件の表示を5,421、未許可の祖先の配下の評価を2,669、`allow` 後の次の取得での実行を511、取得の途中の `allow` を1,710、取得の途中の `allow` の後も未許可のまま公開された結果を5,624 traces で観測しました。`axon show` の witness は、Group の浮上していない着手可能な子孫の評価を52、祖先の未成立で評価されない Issue を1,692、`axon tasks` の行にならない対象での非評価を29,721、`axon tasks` より狭い範囲を15,157、判定失敗を6,280 traces で観測しました。`workAndList` は3,000 traces、最大60 steps（約5秒）で反例なし、`Reopen` した Issue の再表示を52、評価失敗後の着手を50、未成立の結果を持つ進行中の Group の表示を223、進行中の Group の評価を644 traces で観測しました。`workAndList` は条件を自動で許可する設定だけを選ぶため、許可の witness は通常探索が担います。両方を合わせ、41 witness はすべて1 trace 以上で観測されました。
- `lifecycle_information`: 100,000 traces、最大60 steps（約33秒）。9 invariant に反例はなく、27 witness はすべて観測されました。最も少ないのは Issue として `Start` し続けて `Release` してから Group へ変換した履歴で2.2%、次いで `Release` で3.0%、`Reopen` 後の文面編集で3.2%、Group として完了し `Reopen` してから Issue へ変換した履歴で4.6%です。各時点の種類では妥当な履歴が現在の種類で判定すると不正になる状態は8.2%で観測しました。
- `record_integration`: 1,000 traces、最大40 steps（約10.5分）。14 invariant に反例はなく、本実行に指定した31 witness のうち30を観測しました。未観測の1つ（完了と解放の衝突）は補助探索で観測しています。最も少ないのは未採用の祖先と免除を使った修復で各2 traces、次いで `Reopen` と取りやめと着手の衝突で各7、終了 Group への子の流入で11 traces です。修復の witness は取り込み後の修復で14 traces でした。gap は193、gap による偽の衝突は79、二重登録の衝突は409、その解決は35、文面の衝突は278 traces で観測しました。
- `record_integration_paths`: 同じ14 invariant を指定して反例なし。`divergeAndResolve` は400 traces、最大40 steps（約1分）で、別 actor の並行 `Start` の衝突5、値の等しい並行記録の衝突338とその解決335、完了と解放の衝突4、未完了側の選択15、解決後の再着手79、解決記録どうしの並行が同値58・異値39、解決の解決87。`crossMoves` は200 traces、最大12 steps で包含の循環76。`reopenUnderDependent` は300 traces、最大30 steps で完了済みへの未完了 dependency の流入44、`Reopen` による修復12。`reopenAfterInflow` は300 traces、最大20 steps で終了 Group への子の流入4と `Reopen` による修復4、完了済みの子を持つ Group の `Reopen` 9。`breakAndRepair` は200 traces、最大40 steps（約40分）で、通常完了経路の循環125、免除を使った修復34、取り込み後の修復31、包含の循環6、未採用の祖先2、完了済みへの未完了 dependency の流入1、`Reopen` による修復1。本実行と補助探索を合わせ、44 witness はすべて1 trace 以上で観測しました。
