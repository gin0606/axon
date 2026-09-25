# CLIと保存の接続

`src/main.rs` は [共通コア](lifecycle-core.md) と保存 adapter の `src/file.rs`・`src/location.rs` を使う。未知の format は読み込まず、自動変換しない。保存と統合の契約は [保存と統合の契約](../reference/storage.md)、入出力の契約は [CLIと表示の契約](../reference/cli.md)。保存先の初期化・探索と publish の手順は [file保存とGit統合](lifecycle-file.md) にある。

## 独立した保存先で使う

```sh
cargo build
AXON_BIN="$PWD/target/debug/axon"
FIXTURE_DIR="$(mktemp -d)"
cd "$FIXTURE_DIR"
"$AXON_BIN" init demo
"$AXON_BIN" capture --kind group --accept --title '画面を実装する' -m '完了条件を記載する'
"$AXON_BIN" capture --accept --title 'フォームを作る' --parent GROUP_ID --file body.md
"$AXON_BIN" start ISSUE_ID
"$AXON_BIN" note add ISSUE_ID -m '成果と検証結果'
"$AXON_BIN" complete ISSUE_ID
"$AXON_BIN" show GROUP_ID
"$AXON_BIN" note list ISSUE_ID
"$AXON_BIN" complete GROUP_ID
```

`GROUP_ID`・`ISSUE_ID` は作成時に返る完全なIDまたは一意なsuffixへ置き換える。Group の `axon complete` は計画全体の最終確認済みという明示入力であり、子の完了だけで親を自動完了しない。

## 公開操作

登録は `axon capture` の一つで、`--kind issue|group` が種別、`--accept` が初期 lifecycle を選ぶ。`--kind` の既定は `issue`、`--accept` 省略時は `Undecided`、指定時は `NotStarted`。タイトルは `--title`、本文は `-m/--description` または `-F/--file`、`axon write ID` も同じ本文 option を使う。Note は `axon note add ID -m/--message` または `-F/--file`。ファイル引数 `-` は UTF-8 の標準入力を一度読む。本文 option 同士は排他。位置引数は対象 ID、親は `--parent`、依存先は繰り返し可能な `--needs`。入力や操作の拒否は非ゼロで終了する。

`axon accept|withdraw|start|release|complete|cancel|reconsider|reopen ID` は `-r/--reason` を履歴へ保存する。Group への `axon start`・`axon release` は共通コアが拒否し、配下の Issue の着手で実効値が `InProgress` になることを診断で示す。`axon parent set ID --parent G` / `axon parent unset ID` と `axon dep add|rm ID --needs B` は共通コアの関係制約を使う。

`axon list` と `axon show --skip-conditions` は保存情報だけを読む。行は ID・種別・状況・タイトル、作成日時順（同時刻は ID 順）。`axon show` は本文、Note 件数、所属、直接の未充足前提（詰まっている Group では `Stalled` の理由）と Group の全子孫のツリー・終了数（Group・Issue を含み、対象自身を除く）を表示する。一覧と詳細の状況欄は `axon::read::View` が保存値と構造から導出し、`axon tasks` と既定の `axon show` は `axon::read::detail_with` と同じ `Surfacing` を通して評価した再浮上条件を Group の `Ready`・Issue の `Unsurfaced`・詰まっている理由に使う。`axon show` の評価範囲は [候補と外部条件](../reference/candidates.md#評価契約) に定める。Note 本文は `axon note list`、状態変更の前後・理由・統合結果は `axon log` で読む。記録は共通コアの因果順、並行記録のみ ID 順であり、時刻順への並べ替えはしない。直前の記録と先後関係がない箇所には`Concurrent branch` と表示し、逐次操作と区別する。通常表示では端末制御文字をエスケープする。

候補の `axon proposals|tasks` と `axon condition` の設定・評価は [候補と外部条件](../reference/candidates.md) を参照する。記録者は独立crateから取得できた任意情報を添える。`axon merge` と Git driver は [file保存とGit統合](lifecycle-file.md#明示的な統合) を参照する。

## mutationと出力の境界

通常 mutation は保存先の OS lock を取ってから最新の snapshot を読み、共通コアへ渡して検査し、一回の公開で保存する。異なる Entity の変更や Note 追加も同じ境界で直列化する。出力は保存の後。出力失敗は `storage applied; output failed` と明示するため、再実行の前に保存済み状態を照合する。stdout の BrokenPipe は成功として扱う。保存側の `Not applied` と `Result unknown` の区別は [保存の保証](lifecycle-file.md#保存の保証) にある。

## 内部Git呼出し

内部 Git 呼出しは `GIT_*` の override を除外し、現在 directory を探索の起点にする。Git 内では最寄りの `.git` と Git が返す root を照合し、bare repository・壊れた marker から祖先へ fallback しない。repository が探索境界。探索の段と確定の規則、破損・初期化途中で fallback しないことは [保存と統合の契約](../reference/storage.md#探索) に定める。

## 検証境界

`cargo test --workspace --lib --bin axon --test smoke` は共通コア、登録から Group 完了、入力・表示・出力失敗、並行 `axon start` と Note・別 Entity の保存、分岐統合済み snapshot の往復、未対応 format と破損の拒否、初期化途中 marker の拒否、Git 外の探索・境界、実 linked worktree の共有、並行 `axon init` を独立 fixture で検証する。実管理データも installed `axon` も使わない。

process監督・file保存・統合・記録者の検証は `tests/smoke.rs` と記録者crateで実行する。I/O・CLI の接続はコアの lifecycle・包含・dependency の意味を変更しないため、Quint の状態を加えない。

記録者の自動取得・詳細参照は [記録者連携](lifecycle-recorder.md)、初回の試用手順は [使い始める](../guide/getting-started.md) を参照する。
