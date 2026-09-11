# 単一 lifecycle の SQLite CLI

正本は [literate spec](../../spec/lifecycle_proposal.md)。`src/main.rs` は新しい [共通コア](lifecycle-core.md) と `src/sqlite.rs`・`src/location.rs` を使う。旧 module と旧 schema は読み込まず、自動変換しない。既存の管理データにはこの binary を向けない。

## 独立した保存先で使う

```sh
cargo build
AXON_BIN="$PWD/target/debug/axon"
FIXTURE_DIR="$(mktemp -d)"
cd "$FIXTURE_DIR"
"$AXON_BIN" init demo
"$AXON_BIN" group plan --title '画面を実装する' -m '完了条件を記載する'
"$AXON_BIN" plan --title 'フォームを作る' --parent GROUP_ID --description-file body.md
"$AXON_BIN" start GROUP_ID
"$AXON_BIN" start ISSUE_ID
"$AXON_BIN" note add ISSUE_ID -m '成果と検証結果'
"$AXON_BIN" done ISSUE_ID
"$AXON_BIN" show GROUP_ID
"$AXON_BIN" note list ISSUE_ID
"$AXON_BIN" done GROUP_ID
```

`GROUP_ID`・`ISSUE_ID` は作成時に返る完全な ID に置き換える。Group の `done` は計画全体の最終確認済みという明示入力であり、子の完了だけで親を自動完了しない。

登録は `capture` / `plan` と `group capture` / `group plan`。タイトルは `--title`、本文は `-m/--description` または `-F/--description-file`、`write ID` も同じ本文 option を使う。Note は `note add ID -m/--message` または `-F/--file`。ファイル引数 `-` は UTF-8 の標準入力を一度読む。本文 option 同士は排他。位置引数は対象 ID、親は `--parent`、依存先は繰り返し可能な `--needs`。入力や操作の拒否は非ゼロで終了する。

`accept / withdraw / start / release / done / cancel / reconsider ID` は `-r/--reason` を履歴へ保存する。`group set ID --parent G` / `group unset ID` と `dep add|rm ID --needs B` は共通コアの関係制約を使う。

`list` と `show` は保存情報だけを読む。行は ID・種別・状況・タイトル、作成日時順（同時刻は ID 順）。`show` は本文、Note 件数、所属、直接の未充足前提と Group の直属子・終了数を表示する。Note 本文は `note list`、状態変更の前後・理由・統合結果は `log` で読む。記録は共通コアの因果順、並行記録のみ ID 順であり、時刻順への並べ替えはしない。直前の記録と先後関係がない箇所には「並行する分岐」と表示し、逐次操作と区別する。通常表示では端末制御文字をエスケープする。

候補の `triage/tasks` と `when` の設定・評価は [候補一覧と外部条件](lifecycle-candidates.md) を参照する。記録者は独立crateから取得できた任意情報を添える。file 保存・merge は [file保存とGit統合](lifecycle-file.md) を参照する。同じ通常CLIを利用できる。

## 保存と失敗の境界

SQLite は `application_id`・`user_version`・schema・整合性と共通 snapshot を検査する。prefix と、[canonical codec](../../src/lifecycle/codec.rs) の全 snapshot を単一行へ保持し、SQL へ lifecycle の意味を複製しない。分岐の状態記録・Note・統合記録・任意の記録者情報は同じ byte 表現を通して往復する。

通常 mutation は `BEGIN IMMEDIATE` の後で最新状態を読み、共通コアへ渡して検査し、全 snapshot を一 transaction で保存する。最大30秒の SQLite busy timeout を使う。異なる Entity の変更や Note 追加も同じ境界で直列化する。出力は commit の後。出力失敗は `storage applied; output failed` と明示するため、再実行の前に保存済み状態を照合する。SQLite の保存エラーを出力エラーとして扱わない。

内部 Git 呼出しは `GIT_*` の override を除外し、現在 directory を探索の起点にする。Git 内では最寄りの `.git` と Git が返す root を照合し、bare repository・壊れた marker から祖先へ fallback しない。repository が探索境界。SQLite は common Git directory の親の `.axon/axon.db` を共有する。Git 外では最寄りの正本または `init.pending` を持つ祖先を選び、空の `.axon` と lock だけは無視する。混在・破損・不明 schema・初期化途中は別の保存先へ fallback しない。

`init [prefix]` は既定 `axon` prefix の新規作成で、SQLiteが既定。`--backend file` は [file保存](lifecycle-file.md) を参照する。再実行・Git 外の入れ子初期化を拒否し、ignore・attributes・Git config を編集しない。backend 共通の OS lock 下で存在を照合し、同期した pending marker と一時DBを作り、hard link で正本を上書きせず公開する。公開後に directory を同期し、marker を除く。途中失敗は artifact の path を報告して保持する。writer を止め、marker・一時DB・正本を保全して手動で確認する。init を修復として再実行しない。

## 検証境界

`cargo test --lib --bin axon --test smoke` は共通コア、SQLite commit 拒否時の原子性、登録から Group 完了、入力・表示・出力失敗、並行 start と Note・別 Entity の保存、分岐統合済み snapshot の往復、旧・未知・破損 schema、混在・marker 拒否、Git 外の探索・境界、実 linked worktree 共有、並行 init を独立 fixture で検証する。実管理データも installed `axon` も使わない。

旧 process 監督・file crash/merge・記録者・旧 CLI のテストは `archive/three-axis/tests` にある過去の検証資料で、新 binary のテストとして実行しない。process監督・file/merge・記録者の検証は新smokeと記録者crateで実行する。今回の I/O・CLI 接続はコアの lifecycle/包含/dependency の意味を変更しないため、新しい Quint 状態を加えない。

記録者の自動取得・詳細参照は [記録者連携](lifecycle-recorder.md)、初回試用と手動持込みは [使い始める](../guide/getting-started.md) を参照する。
