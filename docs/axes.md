# 状態モデルの軸の整理

このファイルは決定事項ではなく、議論を俯瞰するための作業台。確定した項目は **(決着)** を付け、未決のものは **論点** として残す。

## まとめ

### 確定した状態モデル

| 分類 | 内容 |
| --- | --- |
| 保存する Entity 状態 | `kind` (`Issue` / `Group`) · **A Progress** (`NotStarted` / `InProgress` / `Ended`) · **B Disposition** (`Undecided` / `Accepted` / `Rejected`) · **C Resurface condition** (`Always` / `AtDate` / `AfterEntity`) |
| 保存する関係 | **D 依存** — Entity 間の前提 1 種類 · **包含** — group だけを親にできる一親 tree |
| 導出値 | ready · blocked · orphaned · surfaced · terminal · active scope · blocking cause · group の完了可能性と子孫集計 |
| 持たない | **F 優先度** · **G 表示** |

- A の終端は「やり切った」ではなく「もう進めない」。成果物の有無は A × B から読む
- C は状態ではなく条件の属性。退避させないので進行状態を壊さない
- issue と group は identity、文面、A / B / C を共有し、許される関係と group の完了条件だけが異なる
- 導出値の定義はカスタマイズさせない (不変条件を保証できなくなるため)

実装と CLI で使う英語語彙は次に固定する。A の終端は完遂だけでなく打ち切りも含むため `Done` ではなく `Ended`、B は将来の作業への約束ではなく現在の採否判断なので `Commitment` ではなく `Disposition` と呼ぶ。作業の保持と解放は `claim` / `release`、関係は `dependency` / `dependent`、阻害を説明する表示は `root cause` または `blocking cause` とする。

思想的コアは Quint (`spec/axon.qnt`)、group を明示的な計画 Entity にする拡張は
`spec/group_plan.qnt`、plan declaration と追加情報の統治は
`spec/information_model.qnt` のランダムシミュレーションで別々に検査する。詳細は §5。

### 保留した論点

足すかどうかの判断そのものは axon の issue にある。ここにあるのは判断するときの検討材料。

| 論点 | 検討材料 |
| --- | --- |
| C-5 | C の条件に外部コマンドを許すか (評価コスト・失敗の扱い・決定性) |
| F-2 / F-3 | 優先度を足すなら絶対値か相対順序か、D と統合するか |
| G-1 | 表示をデータモデルに入れるか |

## 0. 出発点 — brで観察された混在

beads_rust (br) は `status` という単一フィールドに、性質の異なる値を並べている。

| 値 | 実際に表しているもの |
| --- | --- |
| `open` / `in_progress` / `closed` | 作業の進行 |
| `blocked` | 依存グラフから導出できるはずの値 |
| `deferred` | 「今は見ない」という可視性の操作 |
| `draft` | 成熟度 (issue の記述、または採否の未確定) |
| `pinned` | 表示上の強調 |
| `tombstone` | 削除の記録 (ストレージの都合) |

単一フィールドなので値は排他になり、次の破壊が起きる (実測済み):

- `in_progress` の issue を pin すると進行状態が消える
- `in_progress` の issue を defer → undefer すると `open` に戻る (進行状態が失われる)
- `defer_until` を過ぎても自動で着手候補に戻らない (静的な集合定義に時間条件を書く場所がない)
- 依存グラフと epic の進捗率が「closed か否か」しか見ないため、採否未決のものを分母から外せない

**この 4 つを構造的に起こさないことが、新しいモデルの最低要件。**

## 1. 軸の候補

### A. 進行 (lifecycle)

**これ以上進むかどうか。**「どこまでやり切ったか」ではない。

- 未着手 / 着手中 / 終了

**A-3 (決着)**: 終端の意味は「やり切った」ではなく **「もう進めない」**。打ち切りも終了に含む。状態の表現としてこちらのほうが正確で、「やり切ったか」は A 単独ではなく A × B の組み合わせから読む。

| A × B | 意味 | 成果物 |
| --- | --- | --- |
| 終了 × 採用 | やり切った | ある |
| 終了 × 不採用 | 着手して打ち切った | ない |
| 終了 × 未判断 | 調べ終えて採否はこれから | 調査結果のみ |

A が「進行が止まったか」、B が「その結果をどう扱うか」を担う。値は `Progress.Ended` と呼ぶ。`Done` は完遂だけを示すように読めるため使わない。

**2-1 (決着)**: 「やり切った後に振り返って不採用と記録する」と、打ち切りと同じ状態になり、成果物があるのに依存元が orphaned になる。これは**許容する**。orphaned は依存元の扱いを再判断するための導出値なので、利用者が「成果物はあるから依存を切って続行」と判断すればよい。終了後の B 変更を禁止する制約は設けない。

**A-1 (決着)**: 「やらないことにした」は A の終端値として持たない。B に分離したため不要。A はあくまで作業がどこまで進んだかだけを表す。

**A-2 (決着)**: 中断のための値は**足さない**。`A=着手中` かつ `C=再浮上日` で表現する。

中断には「他の作業を優先して手を止めた」と「外部要因で進められない」の 2 種類があるが、後者 (例: ライブラリが未対応で、対応されるまで進めない) は進行の度合いではなく**ITS の管理外にある要因による阻害**であり、D (依存) の親戚。外部イベントをツールが判定できないことは C-1 で決着済みなので、日付を決めて利用側が定期的に確認する。いつ対応されるか分からないものは複数回の確認が要るが、そこは C-1 で諦めた部分。

「なぜ止まっているか」は A の値では表現できない (§4 メタ情報の話)。

### B. 採否 (disposition)

やると決めたかどうか。br に存在しない軸で、全 issue が暗黙に「採用済み」として扱われている。

- 未判断 / 採用 / 不採用

**B-1 (決着)**: 「採用したが今はやらない」は B ではなく C で表す。B は判断の内容、C は判断や着手のタイミング。この切り分けなら両者は混ざらない。

**B-1 派生 (決着)**: 「未判断」は値の不在 (null / undefined) ではなく、enum の値として持つ。理由: (a) 未判断は「トリアージ対象である」という情報を持つ積極的な状態、(b) 「未判断 → 採用」を履歴に残すなら遷移元にも名前が要る、(c) null 許容だと「未判断だけ一覧」が他の絞り込みと非対称になる、(d) 未判断に属性 (いつ判断するか / 何を判断すべきか) を持たせたくなるが、値の不在には属性を付けられない。

論点 B-2: 不採用の理由を構造化するか (重複 / 影響が小さい / 前提が変わった / 判断コストが見合わない)。

**B-3 (決着)**: 不採用から採用に戻す遷移は**自由に許す**。特別な経路は設けない。「前提が変わったのでやっぱりやる」は自然に起きるし、禁止する理由がない。履歴に残れば十分。再検討のきっかけは C が持つ — 不採用にするとき再浮上日を設定すれば「N ヶ月後に再検討」になる。

### C. 時期 (schedule)

**いつ再び意識に上げるか。**「いつ着手するか」ではない。

この定義にすると B の値に関わらず C が独立に効く。浮上したとき何をするかは B が決める — 未判断なら採否の判断、採用済みなら着手。

**C-2 (決着)**: 条件は保存しておき、読み取り時またはコマンド実行時に評価する。状態を書き換えて退避させる方式は採らない。

**C-3 (消滅)**: 「復帰したときどの状態に戻すか」は、退避しなければ発生しない。C を状態値ではなく条件の属性として持てば、後送り中も A と B は保持されたまま。br の undefer が `in_progress` を `open` に戻してしまうのは、defer が状態を上書きしているため。

**C-1 (決着・差し替え)**: 条件は **日付** または **他 issue / グループが終端に達すること** の 2 種類。将来 3 種類目を足せるよう、**直和型として定義する**。

当初は「他 issue の完了は D (依存) と重複する」として日付のみに絞ったが、これは誤りだった。両者は依存先が不採用になったときの挙動が違う。

| | 意味 | 依存先が不採用になったら |
| --- | --- | --- |
| D (依存) | X の**成果物**がないと Y ができない | **orphaned** (前提が永久に失われた) |
| C (時期) | X が終わるまで Y を**見たくない** | 条件は満たされる (終端に達した) → Y は**浮上する** |

「今のスコープが終わったら着手」でそのスコープが不採用になった場合、Y は前提を失ったのではなく待つ理由が消えただけ。D で代用すると挙動を誤る。

| 条件 | 評価 | 扱い |
| --- | --- | --- |
| 日付 | 動的評価 (軽い) | 採用 |
| 他 issue / グループの終端到達 | 動的評価 (DB 内なので軽い) | 採用 |
| 外部コマンドによる判定 | 重い・失敗しうる・非決定的 | **将来の拡張点** (下記) |

論点 C-5: 外部コマンドを条件に使えるようにするか (例: ライブラリが rc 以上になったら浮上)。C の定義「機械が判定できる条件」には一貫するが、C-2 (読み取り時に動的評価) と 3 点で衝突する。

- 評価コスト: ready を見るたびに外部コマンドを実行するのは重く、ネットワークにも触りうる
- 失敗の扱い: 条件が「不明」という第三の値になる。日付にはない状態
- 決定性: 「導出値は保存された状態から計算される」性質が崩れ、形式検証の保証が外側に出る

現実的な折衷は**日付との組み合わせ**。「日付で浮上 → そのときコマンドで判定 → 満たしていなければ自動で再後送り」。評価対象が日付到来分に絞られ、sweep も要らない。今は実装せず、条件を直和型にしておくことで拡張点だけ確保する。

C は `Always` または条件の直和型で表す。永続化層で `Always` を null に対応させてもよいが、domain model では明示的な値として扱う。

動的評価を選んだ代償として、次の 2 つを諦めている。

- 「いつ浮上したか」が状態遷移の履歴に乗らない (計算結果であって遷移ではないため)
- 浮上をフックにした通知が作れない (欲しくなったら差分検知が別途要る)

#### 却下した案: 再判断を別タスクに切り出す

「N ヶ月後に再判断するタスク」を作り、元の issue から依存を張る運用。モデルの穴を運用で埋める形になり、次の漏れがある。

1. issue が倍に増える。再判断タスク自体も進行状態と採否を持つ
2. 再判断タスクが完了しても元の issue が浮上する保証がない。依存解消をトリガーにするなら、C 軸を依存グラフで代用していることになり軸を分けた意味が薄れる
3. 「期日で浮上させる」方法が未解決のまま残る (C-2 そのもので、問題が移動しただけ)
4. 再々判断のときタスクを作り直すのか使い回すのか決まらない

C を第一級の軸として持てば不要になる。

### D. 依存 (dependency)

他の作業に阻まれているか。

**これは第一級の状態ではなく導出値**とする (br は status に持ち、整合性維持のためキャッシュテーブルを別に抱えている)。

**D-1 (決着)**: 依存先が不採用になったとき、依存元を解放も阻害継続もせず、**orphaned (前提喪失)** という第三の導出値で表す。

| 依存先 X の状態 | 依存元 Y への影響 |
| --- | --- |
| A = 完了 | 解放 |
| A ≠ 完了 かつ B = 採用 | blocked (通常の待ち) |
| A ≠ 完了 かつ B = 未判断 | blocked (X の採否待ち) |
| **B = 不採用** | **orphaned — Y の前提が失われた。再判断が要る** |

理由:

- 「解放する」は誤り。依存は「X の成果物がないと Y ができない」の意味なので、X が不採用 = 成果物が永久に作られない = Y も成立しない
- 「阻害し続ける」も誤り。依存元の扱いを再判断する必要があるのに、通常の依存待ちと区別がつかず一覧に滞留する
- orphaned は導出値なので、Y の B を自動で書き換えない。意図しない状態変化が起きず、履歴が機械の書き込みで汚れない

**D-1 派生 (決着)**: 依存の種類 (前提 / 順序の希望) は分けず、**前提の 1 種類のみ**とする。順序の希望は依存を張らずに済ませられる (記録したければ description に書く)。br は依存の型を 11 種持ちながら ready を止めるのは 3 種だけで、その 3 種の挙動差も観測できなかった。型を増やしても意味論を定義しなければ、注釈が増えるだけで判断は自動化されない。

**D-1 派生 (決着)**: orphaned は依存の鎖を**推移させない**。Entity 自身と祖先 group を依存元とする直接の依存先だけを見る。代わりに「阻害の根本原因」を辿る導出値 `blocking cause` を定義し、Y が動けない原因が根本の Entity にあることをツール側が計算して見せる。利用者が処理するのは根本原因のみで、その下流は自動的に決まる。

推移的な orphaned を採らない理由: 鎖 Z(不採用) <- X <- Y のとき、X と Y の両方が要対応として出るが、実際に判断が必要なのは X だけ。同じ問題が二重に一覧へ出る。弱点: Y はどのリストにも要対応として出ないため、X が放置されると Y も見えないまま滞留する。

→ 前提として「orphaned の一覧を定期的に見る」運用が要る。

**D-2 (解決)**: X が C で後送り中のとき依存元も実質動かない件は、`blocking cause` が兼ねる。後送り中の issue も根本原因として返るため、専用の導出は不要。

**D-3 (決着・差し替え)**: dependency と `AfterEntity` は意味論と保存先を分けたまま、包含を含む待機グラフで一緒に循環検査する。group を依存元または `AfterEntity` の主体にした辺は、その group 自身と全子孫へ展開する。

包含は phase によって待つ向きが変わる。親 group の start 前は子が親を待つため `child -> parent`、親 group の start 後は親の完了が子孫の終端を待つため `parent -> child` になる。関係の追加・置換時には、論理的な待機辺へそれぞれの包含辺を加えた **activation wait graph** と **completion wait graph** の両方が非循環であることを要求する。この 2 射影により、たとえば「子が祖先 group の完了を待つ」「未着手 group が、自身の非アクティブな子を前提として待つ」といった、phase の変化で顕在化する deadlock も入力時に拒否する。`Progress=Ended` の Entity は再開しないため循環検査の node から外す。

### F. 優先度 (priority)

同時に着手できるものが複数あるとき、どれを先にやるか。**それ以外の意味を持たせない。**

観察: 実運用では全 issue が高優先度に寄る。原因は採否と優先度の混同。「やる」と決めたものに低い優先度を付けるのは、「やると決めたのに重要でない」という矛盾した宣言になるため。br の `P4 = backlog` はこの混入そのもので、優先度軸に B (採否) と C (時期) の意味が漏れていた。B と C を独立させた後は、優先度は順序だけを担うので矛盾は起きない。

観察: 「優先度が低いと放置される」の一部は C の守備範囲。放置されること自体は低優先度なら正常。問題は「永久に見返さない」ことで、それは定期的な浮上で扱う。

**F-1 (決着)**: **持たない。**B と C が機能していれば ready に残るのは「やると決めて・今の時期で・依存解消済み」のものだけ。5 件なら順序づけは要らない。20 件に膨らんだなら B か C の運用が甘い証拠であって、並べ替えても解決しない。優先度は「ready が絞れていないこと」への対症療法である可能性。個人運用では厳密に順序を管理するより、サクッと手を付けたほうが早い。実際に ready が膨らんで困ってから足す。以下 F-2 / F-3 はそのときの検討材料として残す。

論点 F-2: 持つ場合、絶対値 (P0-P4) か相対順序か。

| | 絶対値 | 相対順序 |
| --- | --- | --- |
| 各段の意味を定義する必要 | ある (運用中に基準がドリフトする) | ない (「これはあれより先か」の二択) |
| 全部高優先度になる | なる | **原理的に起きない** (順序は一意) |
| 新規追加のコスト | 低い | 高い (毎回どこに挿すか決める) |
| チーム運用 | 耐える | 破綻しやすい |

論点 F-3: 相対順序を採ると、優先度は issue 単体の属性ではなく **issue 間の関係**になり、§1.5 の分類で D (依存) と同じ行に入る。依存グラフは既に部分順序を与えており (X が終わらないと Y ができない)、優先度の全順序はその拡張。両者は「順序を決める」同じ目的を持ち、違うのは強制力だけ (依存は「できない」、優先度は「後でいい」)。統合できるか、分けたままにするか。

### G. 表示 (presentation)

pin、色、並び順。

論点 G-1: そもそもデータモデルに入れるか。状態ではないので、クライアント側の関心として分離できる。

**G-2 (決着): 人が読む観測出力は、保存構造ではなく利用者の問いの順に並べる。** Entity の identity と現在の snapshot を、長文や蓄積された履歴より先に置く。短い scalar は比較できる密度でまとめ、description や Note のような長文だけを独立した block にする。存在しない任意 block は場所を取らず、同じ事実を概要と詳細へ重複表示しない。

`show` はこの原則を最初に適用する。一つの出力で現在地から plan graph、本文、追加情報、履歴へ読み進められるようにし、情報を減らした概要 mode は別に作らない。この原則は他の人向け出力にも適用し、Entity の一覧、履歴と索引、一件の詳細、状態変更の確認という役割ごとに共通の文字構造を持たせる。同じ意味には同じ語彙と装飾を使うが、役割の異なる出力を一つの万能 format へ押し込まない。

人が直接見る TTY と、pipe・redirect・エージェントが読む非TTYは同じ文字構造を使う。色や太字は状態の識別を補助するだけで、文字、位置、ラベルが持つ意味を代替しない。非TTYの record 境界、先頭の識別子、標準 stream は安定した外部契約とする。一方、偶然生じた後半 field の不統一は互換性として固定せず、役割ごとの共通 grammar へ揃える。利用者が保存した本文は表示装飾の対象にしない。

すべてを一項目一行にする案は、短い Entity と Group 集計を縦に引き延ばして一覧性を下げるため採らない。密な平面へ色だけを足す案は、状態、定義、履歴の境界を色なしで辿りにくいため採らない。冒頭の要約と後続の詳細へ同じ事実を出す案と、TTYごとに別 layout を持つ案は、表示内または実装内に二重管理を作るため採らない。

生成物そのものを標準出力へ書く `export` と completion は人向け表示の対象外とし、装飾しない。人向け出力を pipe の途中で閉じられた場合は正常な終了として扱い、表示先の都合で追跡操作自体を失敗にしない。

この決着は現在の状態と導出値の見せ方を出力側で定めるもので、presentation のための保存値を追加しない。将来、保存する presentation 自体を導入するかという G-1 は別の論点として残る。

## 1.5 軸の性質は同列ではない

「軸」と呼んでいたものを詰めた結果、保存されるものの性質が 3 つに分かれた。

| 軸 | 性質 | 保存されるもの |
| --- | --- | --- |
| A 進行 / B 採否 | 利用側が明示的に遷移させる状態。履歴を持つ | enum |
| C 時期 | 条件を書いておくと機械が判定する属性。状態ではない | `Always` / 日付 / Entity 参照 |
| D 依存 | Entity 間の関係 | 依存元・依存先の ID |
| 包含 | group を親にする一親 tree | Entity ごとの親 group ID (またはなし) |
| ready / blocked / orphaned など | 完全な導出値 | 何も保存しない |
| F 優先度 | **持たない** | — |
| G 表示 | データモデルに入れない方向 | — |

### 軸を直交させたことの効用

軸が独立していると、**要らない軸だけを落としても他が壊れない**。F (優先度) を持たない判断はこれで成立している。

br はこれができない。単一の `status` に進行・可視性・成熟度・表示・削除を畳んでいるため、1 つの値を捨てると他の意味まで道連れになる。機能を減らす自由度は、軸を分解して初めて手に入る。

### 現時点の最小構成

Entity ごとに保存する軸は 3 つだけで、issue と group が同じ語彙を使う。

| | 内容 |
| --- | --- |
| A 進行 | 未着手 / 着手中 / 完了 |
| B 採否 | 未判断 / 採用 / 不採用 |
| C 時期 | `Always` / 再浮上する日付 / 他 Entity の終端 |

D (依存) と包含は関係として別に保存されるが、blocked / orphaned / active scope / ready はそこからの導出値。F (優先度) と G (表示) は持たない。

## 2. 直交性の検証

軸を分けるなら、組み合わせに意味があることを確認する必要がある。

| A (進行) | B (採否) | 意味があるか |
| --- | --- | --- |
| 未着手 | 未判断 | ある — 投げ込んだだけの状態 |
| 未着手 | 採用 | ある — 着手待ち |
| 未着手 | 不採用 | ある — 見送りの記録 |
| 着手中 | 未判断 | ある — 調べないと採否が決まらない場合 (spike) |
| 着手中 | 採用 | ある — 通常の作業中 |
| 着手中 | 不採用 | **ある** — 着手した結果、不要と判明した時点の状態 |
| 完了 | 未判断 | **論点** — 「調査だけ終わって採否はこれから」と読めなくもない |
| 完了 | 採用 | ある — 通常の完了 |
| 完了 | 不採用 | **ある** — 着手した結果いらないと分かり、そこで打ち切った |

A と B を分ける効用が最も出るのがこの右下の領域。br は「完了した」と「やらないことにした」を両方 `closed` に畳んでいるため、着手して不要と判明したケースと、やり切ったケースを後から区別できない。

(2-1 は A-3 とあわせて決着。§1 A を参照)

## 3. 導出値として定義したいもの

| 名前 | 定義 |
| --- | --- |
| active scope 内 | 全祖先 group の activation gate が開いている。root Entity は常に active scope 内 |
| group activation gate | group 自身が `InProgress` / `Accepted` / surfaced で、blocked でも orphaned でもない間だけ子孫を開く |
| 着手可能 (ready) | active scope 内かつ A=未着手 かつ B=採用 かつ C の条件を満たす かつ blocked でない かつ orphaned でない |
| 阻害中 (blocked) | Entity 自身または祖先 group の依存先に未終端 (A≠完了 かつ B≠不採用) のものがある |
| 前提喪失 (orphaned) | Entity 自身または祖先 group の依存先に B=不採用 のものがある (依存の鎖は**推移しない**) |
| 浮上中 (surfaced) | Resurface condition が満たされている |
| 終端 (terminal) | A=`Ended` または B=`Rejected` |
| triage frontier | active scope 内かつ非終端で、B=未判断または orphaned の Entity。gate が閉じた group の子孫は含めない |
| group 完了可能 | group 自身が `InProgress` で、全子孫 Entity が terminal |
| 阻害の根本原因 (blocking cause) | Entity 間依存だけを遡り、前提喪失 / 後送り中 / active scope 外 / それ以上依存を持たない Entity に到達した地点。包含による非 active の理由は別に示す |

**3-1 (決着)**: 導出値の**定義はカスタマイズさせない**。

カスタマイズを許すと、検証対象が「1 つのモデル」ではなく「設定のパラメータ空間全体」になり、全パラメータで不変条件が成り立つことを確かめるのは実質不可能。ready の定義に orphaned を含める設定が書けてしまえば、前提喪失のものが着手候補に出る。br の `status_groups.ready` は任意の status 集合を ready と呼べるため、ready の意味を保証していない。

| カスタマイズの種類 | 可否 |
| --- | --- |
| ready / blocked / orphaned の**定義**を変える | **禁止** — 不変条件が壊れる |
| 一覧の並び順、絞り込み、表示項目 | 自由 — 導出値を消費する側で、モデルの意味は変わらない |
| クエリやビューの保存 | 自由 — 同上 |

原則: **カスタマイズしたくなったらモデルが間違っている兆候**と扱う。設定で逃げるとモデルの誤りが隠れたまま運用される。個人用ツールなので、設定ではなくモデルを直す。

### 操作の分類と反復実行の契約 (決着)

保存内容を変えるコマンドは、操作の意味によって 3 種類に分ける。同じ入力を繰り返したときの扱いは、CLI 全体で一律にせず、この分類から決める。

| 分類 | コマンド | 同じ入力を繰り返したとき |
| --- | --- | --- |
| 状態遷移 | `start` · `done` · `release` · `decide` · `when` | 事前条件を満たさないため失敗 |
| 設定 | `write` · `group set/unset` · `dep add/rm` | 目標を満たしていれば成功 no-op |
| 追加 | `plan` · `capture` · `group plan` · `group capture` · 将来の append / comment | 新しい実体や記録を増やす |

状態遷移が成功するのは、次の遷移だけとする。

| コマンド | 事前条件 | 遷移先 |
| --- | --- | --- |
| `start` | ready な未着手 | 着手中 |
| `done` | 着手中。group は加えて全子孫が終端 | 終了 |
| `release` | 着手中。group は加えて着手中の子孫がない | 未着手 |
| `decide` | 指定した採否と現在値が異なる | 指定した採否 |
| `when` | 指定した条件と現在値が異なる | 指定した条件 |

`release` が対象にするのは実行時点の着手であり、claim の actor や worktree は事前条件にしない。状態遷移の事前条件は、更新および履歴の記録と同じトランザクションで検査する。失敗時は非 0 で終了し、保存状態、履歴、`updated_at` を変更しない。エラーには現在の事実だけを簡潔に示し、次の操作の指示や、入力された reason などの自由記述は含めない。

reason は状態からだけでは意図を復元できない操作に限って任意で受け取る。`start` と `done` は reason を持たず、`release`、`decide`、`when` は指定された場合だけ遷移または設定と同じトランザクションで理由を履歴に残す。reason の有無は操作の成否を変えない。

設定は目標値または集合を宣言する操作なので、すでにその状態なら成功 no-op とし、`updated_at` を変更しない。

追加操作は呼び出すたびに新しい実体や記録を増やす。現時点では追加操作に idempotency key を導入せず、重複を避ける必要性が確認された時点で設計する。

## 4. この軸の議論から外したもの

### メタ情報をどこまで構造化するか (決着)

軸 (状態モデル) の問題ではなく、フィールド設計と入力フローの問題として切り出した。結論は**専用フィールドを足さず、履歴に理由を残す**こと。

- **不採用の理由** (B-2) — 状態を変えた履歴に理由を書けるようにしたため、専用フィールドは不要になった
- **浮上したときに何を判断すればよいか** (元 C-4) — description で足りる。判断軸は「無いとモデルが壊れるか」で、これは壊れない
- **後送りの理由を必須入力にするか** — 必須にはしない。理由を残せる経路があれば足りる

## 5. Quint models と検査

`spec/axon.qnt` は axon の思想的コアである A / B / C / D の直交性、dependency と Resurface condition の差、ready / blocked / orphaned / blocking cause だけを扱う。group の identity、包含、状態、依存は持ち込まない。

`spec/group_plan.qnt` は今回の設計だけを扱う拡張 model で、3 issue と 3 group からなる固定 Entity 集合を共有状態にする。実装上の DB transaction に対応して、各操作は 1 action で原子的に実行する。時刻は `AtDate` の評価に必要な小さい整数 clock だけを持つ。通信、障害、複数 actor、wall-clock、永続化はこの状態機械の関心ではない。

`spec/information_model.qnt` は Entity ごとの plan declaration、Control state、
Declaration Revision、Note、判断履歴、進行履歴を共有状態として扱う。title と
description の内容、actor の本人性、wall-clock、SQLite、CLI 構文は抽象化し、
情報の所有範囲、固定、Revision の現在参照、追記専用性、操作ごとの変更範囲を検査する。

拡張 model が保存状態として持つのは Entity map、一親の parent map、dependency 集合、clock である。`ready`、`blocked`、`orphaned`、active scope、`triage`、blocking cause、group の完了可能性、2 つの待機グラフは純粋関数で導出する。操作 witness のために使う `observed` は ghost state であり、axon の保存対象ではない。

### Group 拡張で検査する性質

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
| witness | 全 14 action が到達可能である |
| witness | 全子孫が終端した InProgress group が、Ended へ自動変更されず明示 done を待てる |
| witness | Rejected の子 group を含む親 group が完了可能になる |
| witness | Rejected の group を依存先にすると依存元が orphaned になる |
| witness | group を依存元にした未解決 dependency が子孫を blocked にする |
| witness | 祖先 group 由来の dependency が子孫の blocking cause に現れる |
| witness | group から group を参照する `AfterEntity` を設定できる |
| witness | 親 group の start 後に入れ子の Entity が ready になる |
| witness | 親 group が判断対象なら、その子孫は triage に出ない |

検査は Quint 0.32.0 で次の順で行う。先頭の version 出力が異なる場合は、この再現条件の成功として扱わない。`quint run` は bounded random simulation であり、反例が見つからなかったことは全状態についての証明ではない。

```sh
quint --version

quint typecheck spec/axon.qnt
quint run spec/axon.qnt --main axon \
  --invariants invRejectedEndedStillBlocks invReadyExclusive \
    invIssueWaitsAcyclic invBlockedHasCause invCauseIsUnresolved \
    invCondRefSatisfiedByRejection invCondAndDepDiffer \
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
  --witnesses wDecide wStart wDoneIssue wDoneGroup wReleaseIssue wReleaseGroup \
    wSetDate wSetAfter wClearWhen wSetParent wUnsetParent wAddDependency \
    wRemoveDependency wTick wGroupAwaitingExplicitDone \
    wRejectedChildGroupCanComplete wGroupDependencyOrphaned \
    wGroupDependencyBlocksDescendant wInheritedGroupBlockingCause \
    wGroupAfterGroup wNestedEntityReady wTriageFrontier \
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

2026-09-03 の統合確認では、core、group 拡張、情報統治を各 1,000 traces、最大 80 steps で実行した。core の 7 invariant、group 拡張の 14 invariant、情報統治の 22 invariant に反例は見つからず、列挙した witness はすべて少なくとも 1 trace で観測された。これは bounded random simulation の結果であり、完全探索による証明ではない。

### 以前の model で見つかった考慮漏れ

旧 issue-only model では、依存の鎖 `Z(Rejected) <- X <- Y` で X だけが orphaned になり、Y が通常の blocked と区別できなかった。これは orphaned を推移させず、別途 `blocking cause` で根本原因を辿る設計につながった。

旧 group model では group 依存が issue 依存と別構造だったため、blocking cause が group 依存を説明できない反例も見つかった。新 model は依存先・依存元を Entity に統一し、group 依存の作用範囲だけを子孫へ展開するため、関係の種類による説明漏れを作らない。

group 拡張 model のレビューでは、Ended group の祖先だけを検査していたため、Ended group 自身を別の親へ移動できる反例が見つかった。包含変更の guard に対象自身の Ended 判定を加え、Ended group の親変更と親解除が常に無効である invariant を追加した。

## 6. Group を明示的な計画 Entity として扱う

### Group が必要な理由

issue 間の dependency だけでは、計画に含まれる子タスクと、その計画全体が待つ外部の前提を区別できない。group は issue / group を計画範囲として包含し、group に置いた dependency と判断 frontier を配下へ効かせる独立 Entity とする。

group は単なる分類ではない。利用者またはエージェントが計画単位を明示的に start し、子孫を確認してから明示的に done する。子孫の集計は観測事実、group の `Progress=Ended` は計画単位についての宣言であり、両者を同じ値にしない。

### Issue と共有する核

issue と group は次を共有する。

- 管理 root の prefix とランダム 6 文字からなる、同じ namespace の自動生成公開 ID
- title と description
- `Progress`、`Disposition`、`ResurfaceCondition`
- entity 単位の claim
- ID から対象 kind を解決する状態操作と参照

ID に kind を埋め込まず、一覧と `show` が `Issue` / `Group` を表示する。短縮 ID の解決と一意性判定も全 Entity を対象にする。group の slug は alias としても残さない。変更可能な人間向け文面を identity にすると、rename、重複、import の解決規則が別に必要になるためである。

group の claim は子孫を lock しない。group と子孫は別 actor / session が同時に claim でき、計画担当から子 issue を委譲できる。release の整合だけは子孫の進行状態から判定する。

### 包含

包含は、各 issue / group が最大 1 つの親 group を持つ tree とする。group だけが親になれ、深さは制限しない。複数親による活性・採否の矛盾を避けるため一親にし、共有成果物は dependency、横断分類は query や将来の tag で表す。

group の activation gate が開いて子孫を active scope に入れるのは、次をすべて満たす間である。

- `Progress=InProgress`
- `Disposition=Accepted`
- 自身が surfaced
- 自身または祖先 group の dependency によって blocked / orphaned ではない

Entity は全祖先 group の activation gate が開いているときだけ active scope 内になる。root Entity は祖先を持たないため常に active scope 内で、自身の Progress / Disposition などは ready と triage が別に判定する。したがって、親 group が NotStarted、後送り中、Rejected、blocked、orphaned の間、配下の保存状態は変えず ready と triage から外す。子を start して親を暗黙に start する操作は持たない。

`group set <entity-id> <parent-group-id>` は親の新規設定または移動、`group unset <entity-id>` は親の解除を意味する。包含変更は一親制約、Ended group の構造固定、進行中の子を NotStarted group の下へ移さない制約、待機グラフの非循環を 1 transaction で検査してから適用する。

### 状態遷移

issue と group は共通の top-level `start` / `done` / `release` / `decide` / `when` を使う。group 専用の状態は増やさない。

| 操作 | group の事前条件 | 効果 |
| --- | --- | --- |
| `start` | ready | group 自身だけを InProgress にする。子孫は自動 start しない |
| `done` | 自身が InProgress かつ全子孫 Entity が terminal | group 自身だけを Ended にする |
| `release` | 自身が InProgress かつ InProgress の子孫が 0 件 | group 自身だけを NotStarted にする |
| `decide` | 現在と異なる値。Ended group の子孫を terminal から非 terminal に戻さない | group 自身の Disposition だけを変える |
| `when` | 現在と異なる条件で、`AfterEntity` は待機グラフを循環させない | group 自身の条件だけを変える |

group の完了は子孫から自動導出しない。最後の子が terminal になった直後も group は InProgress のままで、利用者またはエージェントが計画全体を確認して `done` する。空の InProgress group は全子孫が terminal という空集合上の条件を満たすため明示 done できるが、空であるだけでは自動終了しない。

子 group の `Disposition=Rejected` は terminal なので、親の完了判定でその子 group 自身を妨げとして数えない。ただし親の条件は「全**子孫** Entity が terminal」であり、Rejected group の下に非 terminal な Entity が残っていれば、それらは別に終端化または包含外へ移す必要がある。Rejected は子孫へ伝播しないという原則と、Ended group の全子孫を terminal に保つ不変条件を両立させるためである。

### Rejected と Ended の境界

group を Rejected にしても子孫の Disposition / Progress は変更しない。Rejected の祖先 group 配下は active scope 外になり、保存状態を保ったまま ready と triage から外れる。これは dependency の前提喪失ではないため、包含だけを理由に orphaned とは呼ばない。

一方、dependency の依存先 group が Rejected なら、その成果または計画単位が得られないため依存元は orphaned になる。依存元が group なら group 自身と全子孫に作用するが、triage は判断 frontier の group だけを表示し、配下を一件ずつ並べない。

Ended group には再 open を用意しない。次を禁止して、完了宣言を後の操作で無効化せず、すでに解放した dependent を再び塞がない。

- Ended group 自身の親変更、および subtree への追加、subtree からの削除、subtree 内外への移動
- Ended group を依存元とする dependency の追加・削除
- Ended group の子孫を terminal から非 terminal に戻す Disposition 変更

Ended の Entity も declaration 固定について他の Progress と同じ規則に従う。title / description を訂正する場合は Undecided に戻して draft を編集し、改めて採否を判断する。Progress は Ended のままであり、Ended group の構造固定と子孫の terminal 制約も維持する。完了後に見つかった追加作業は、Ended group の外に新しい issue または group として作る。

### Dependency と Resurface condition

dependency は issue→issue、issue→group、group→issue、group→group の全組み合わせで同じ hard prerequisite を表す。

- 依存先が `Disposition=Rejected` なら、Progress にかかわらず依存元は orphaned
- Rejected でない依存先が `Progress=Ended` なら解消
- それ以外なら依存元は blocked
- 依存元が group なら、その group 自身と全子孫へ同じ結果を作用させる

包含とは違い、dependency は成果または明示完了した計画単位を待つ関係である。group の子孫がすべて終端でも group 自身を明示 done するまでは、その group を待つ dependency は解消しない。

`AfterEntity` の主体と参照先も issue / group の全組み合わせを許す。参照先が Ended または Rejected なら surfaced になる。参照先の Rejected で orphaned になる dependency と、「待つ時期が終わった」と解釈して surfaced になる Resurface condition の非対称は維持する。InProgress group を後送りすると activation gate が閉じ、子孫も ready / triage から外れるが、子孫の保存状態は変えない。

### Deadlock を作らない関係更新

dependency、`AfterEntity`、包含を別々の DAG として検査しても、関係を横断する循環は見つからない。そこで §1 D-3 のとおり 2 つの待機グラフへ射影し、どちらかが循環する追加・置換を拒否する。

| graph | 包含辺 | 検出する代表例 |
| --- | --- | --- |
| activation wait | child → parent | start 前の group が、自身の active scope 外の子孫を dependency / when で待つ |
| completion wait | parent → child | 子孫が祖先 group の明示完了を dependency / when で待つ |

group を依存元または when の主体にした論理辺は group の全子孫へ展開する。これにより直接の 2-cycle だけでなく、複数の dependency / when / 包含をまたぐ間接循環も同じ非循環条件で拒否できる。削除と解除は循環を増やさないため、既存の問題を解消する経路を妨げない。

### Observed 情報と triage frontier

group の保存済み Progress と、配下の状況は別々に表示する。最低限、次は保存せず読み取り時に導出する。

- 直下と全子孫について、Entity 総数、kind 別件数、Progress / Disposition / terminal の件数
- blocked / orphaned / active scope 外の理由
- InProgress の子孫数
- group を現在 done できるかと、妨げている非 terminal 子孫

`triage` は active scope 内かつ非 terminal で、Undecided または orphaned の Entity だけを返す。親 group が Undecided / orphaned なら親だけが frontier になり、その判断と start を経て子が active scope 内になった後に子側の判断を見せる。全件と inactive 理由を確認する経路は `list` と `show` が担う。

### CLI の境界

issue 作成は `axon plan` / `axon capture`、group 作成は `axon group plan` / `axon group capture` とし、どちらも必要なら `--parent <group-id>` で親を同時指定できる。

`show`、`write`、`start`、`done`、`release`、`decide`、`when`、`dep`、`log` は ID から kind を解決する共通 top-level command とする。`ready`、`triage`、`claims`、`list` は両 kind を同じ一覧へ出し、必要なら `--kind issue|group` で絞り込む。包含操作だけを `group set` / `group unset` として group namespace に残す。

旧 `group new` / `group show` / `group list` / `group reject` / `group dep` と slug identity は互換 alias を残さない。

### Model の scope と更新条件

`spec/group_plan.qnt` は共有 Entity 状態、包含、dependency、Resurface condition、原子的な状態操作と導出値を対象にする。ID の文字列表現、title / description、SQLite migration、履歴、時刻の実装、actor / session claim、CLI の構文・表示、宣言ファイルと fingerprint は対象外である。

この拡張 model は Rust 実装より先に更新する。実装中に遷移、禁止操作、導出値の不足や矛盾が見つかった場合、コードだけで補わず、この文書と `spec/group_plan.qnt` の意味を揃えてシミュレーションしてから実装へ反映する。A / B / C / D のコア自体を変える場合だけ `spec/axon.qnt` も更新する。

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
