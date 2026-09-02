# CLI

## 方針

axon の操作対象は `Issue` と `Group` の 2 kind を持つ Entity である。どちらも同じ公開 ID、文面、Progress、Disposition、Resurface condition、claim を持つ。対象 kind を先に選ばせる namespace は作らず、ID を受け取る top-level command が kind を解決する。

完全な利用者向け help は英語の `docs/help.md`、状態モデルと設計理由の正は `docs/axes.md` に置く。Usage、引数、option、コマンドツリーは Clap の定義から生成する。`axon help` と `axon --help` は完全 help、`axon -h` は短い一覧を返す。

## コマンド境界

### 作成

| kind | Accepted で作成 | Undecided で作成 |
| --- | --- | --- |
| Issue | `axon plan <title>` | `axon capture <title>` |
| Group | `axon group plan <title>` | `axon group capture <title>` |

4 コマンドはいずれも `--parent <group-id>` を受け取り、作成と包含設定を同じ transaction で行う。呼び出すたびに新しい Entity を作る追加操作である。

旧 slug ベースの `group new` / `group list` / `group show` / `group reject` / `group dep` は提供しない。Group は Issue と同じ自動生成 ID で参照する。

### 共通の状態・文面・関係操作

次の command は ID から kind を解決し、Issue / Group の両方へ同じ入口を使う。

- `show` / `write` / `log`
- `start` / `done` / `release`
- `decide accept|reject|undecide`
- `when at|after|clear`
- `dep add|rm`

`when after` の参照先と dependency の両端も kind の全組み合わせを許す。`AfterEntity` は参照先が Ended または Rejected なら浮上する。dependency は Rejected を前提喪失として扱う。

### 包含

包含だけを group namespace に置く。

- `group set <entity-id> <parent-group-id>` は親の新規設定または移動
- `group unset <entity-id>` は親の解除

親は必ず Group とし、各 Entity は最大 1 つの親を持つ。設定操作なので、すでに同じ親である set と、親がない unset は成功 no-op になる。

### 一覧

`ready` / `triage` / `claims` / `stale` / `list` は両 kind を同じ一覧に出し、`--kind issue|group` で任意に絞る。各行の第 1 列は ID、第 2 列は kind とする。

`stale` は claim の経過時間やプロセス状態から自動判定せず、`claims` と同じ保存済み事実を返す。解放は人が判断して `release` を明示する。

## Group の進行

Group は計画範囲を明示的に進める Entity であり、子孫から自動完了しない。

1. ready な Group を `start` すると activation gate が開く。
2. 配下の active frontier が `ready` / `triage` に現れる。
3. 全子孫が terminal になった後、Group 自身を `done` する。

Group の `release` は InProgress の子孫が 0 件のときだけ成功する。Group の claim は子孫を lock せず、Group と子孫を別 actor が同時に claim できる。

`show` は Group 自身の保存済み状態と、直下・全子孫の kind / Progress / terminal 集計、現在 done できるか、妨げている非 terminal 子孫を分けて表示する。

## 反復実行と履歴

| 分類 | command | 同じ入力の反復 |
| --- | --- | --- |
| 状態遷移 | `start` / `done` / `release` / `decide` / `when` | 失敗 |
| 設定 | `write` / `group set|unset` / `dep add|rm` | 成功 no-op |
| 追加 | `plan` / `capture` / `group plan` / `group capture` | 新規 Entity を追加 |

失敗した遷移と成功 no-op は状態、履歴、`updated_at` を変えない。`start` と `done` は reason を持たず、`release` の任意 reason は進行履歴、`decide` / `when` の任意 reason は判断履歴に保存する。

## 原子性と deadlock 防止

`start` は ready の検査と claim の取得、Group の `done` / `release` は子孫条件の検査と進行更新を、それぞれ同じ write transaction で行う。

dependency、`AfterEntity`、包含を activation wait graph と completion wait graph に射影する。relation の追加・置換は両 graph の非循環と包含の不変条件を同じ transaction 内で検査してから保存する。Group を待機元にした dependency / `AfterEntity` は Group 自身と全子孫へ展開する。既存データの循環を解消できるよう、辺を除く `dep rm` / `when clear` / `when at` / `group unset` は他の循環が残っていても実行できる。

Ended Group は完了宣言を後から無効にしないため、親変更、subtree の出入り、依存元としての dependency 変更を拒否する。Ended Group 配下の terminal Entity を非 terminal に戻す Disposition 変更も拒否する。

## 出力と管理 root

help、一覧、詳細、成功確認は stdout、エラーは stderr に出す。一覧が空なら stdout を空に保ち、案内だけを stderr に出して成功する。

Git 配下では common Git directory の親を管理 root とし、全 worktree で `.axon/axon.db` を共有する。Git 外ではカレントから祖先へ最も近い DB を探す。Issue と Group は同じ `<prefix>-<ランダム 6 文字>` namespace を使い、完全 ID または一意な suffix で解決する。
