# CLIと保存の接続

`src/main.rs` は [共通コア](lifecycle-core.md) と保存 adapter の `src/file.rs`・`src/location.rs` を使う。未知の format は読み込まず、自動変換しない。保存と統合の契約は [保存と統合の契約](../reference/storage.md)、入出力の契約は [CLIと表示の契約](../reference/cli.md)。保存先の初期化・探索と記録 file の作成の手順は [file 保存と Git 統合](lifecycle-file.md) にある。

## 独立した保存先で使う

```sh
cargo build
AXON_BIN="$PWD/target/debug/axon"
FIXTURE_DIR="$(mktemp -d)"
cd "$FIXTURE_DIR"
"$AXON_BIN" init demo
"$AXON_BIN" capture --kind group --accept --label feat --title '画面を実装する' -m '完了条件を記載する'
"$AXON_BIN" capture --accept --label feat --title 'フォームを作る' --parent GROUP_ID --file body.md
"$AXON_BIN" list --label feat
"$AXON_BIN" start ISSUE_ID
"$AXON_BIN" note add ISSUE_ID -m '成果と検証結果'
"$AXON_BIN" complete ISSUE_ID
"$AXON_BIN" show GROUP_ID
"$AXON_BIN" note list ISSUE_ID
"$AXON_BIN" complete GROUP_ID
```

`GROUP_ID`・`ISSUE_ID` は作成時に返る完全なIDまたは一意なsuffixへ置き換える。Group の `axon complete` は計画全体の最終確認済みという明示入力であり、子の完了だけで親を自動完了しない。

## 公開操作

登録は `axon capture` の一つで、`--kind issue|group` が種別、`--accept` が初期 lifecycle を選ぶ。`--kind` の既定は `issue`、`--accept` 省略時は `Undecided`、指定時は `NotStarted`。label は必須の `--label` で与え、`axon label set ID VALUE` で変える。値は共通コアの `Label` の綴りを clap の `ValueEnum` として受けるので、省略と集合外の値は保存先を開く前の構文エラーになる。タイトルは `--title`、本文は `-m/--description` または `-F/--file`、`axon write ID` も同じ本文 option を使う。Note は `axon note add ID -m/--message` または `-F/--file`。ファイル引数 `-` は UTF-8 の標準入力を一度読む。本文 option 同士は排他。位置引数は対象 ID、親は `--parent`、依存先は繰り返し可能な `--needs`。入力や操作の拒否は非ゼロで終了する。

`axon accept|withdraw|start|release|complete|cancel|reconsider|reopen ID` は `-r/--reason` を記録へ保存する。Group への `axon start`・`axon release` は共通コアが拒否し、配下の Issue の着手で実効値が `InProgress` になることを診断で示す。`axon parent set ID --parent G` / `axon parent unset ID` と `axon dep add|rm ID --needs B` は共通コアの関係制約を使う。`axon convert ID --kind issue|group` は共通コアの変換の前提（保存値が `Undecided`・`NotStarted` であること、Issue へ戻す Group に子がないこと）で種類だけを変え、`-r/--reason` を持たない。衝突中の Entity がある保存先では、`axon resolve` と `axon note add` 以外の mutation を共通コアが拒否し、衝突中の Entity を診断で示す。

`axon list` と `axon show --skip-conditions` は保存情報だけを読む。行は ID・種別・状況・label・タイトル、作成日時順（同時刻は ID 順）。`--label` は `--kind`・`--search` と同じく `Selection` の絞り込みで、`axon list|tasks|proposals` が共有する。`axon show` は本文、Note 件数、所属、直接の未充足前提（詰まっている Group では `Stalled` の理由）、衝突中なら head の一覧、違反に含まれるなら違反の一覧と、Group の全子孫のツリー・終了数（Group・Issue を含み、対象自身を除く）を表示する。一覧と詳細の状況欄は `axon::read::View` が記録の集合から導出した現在値と構造から導出し、`axon tasks` と既定の `axon show` は `axon::read::detail_with` と同じ `Surfacing` を通して評価した再浮上条件を Group の `Ready`・Issue の `Unsurfaced`・詰まっている理由に使う。`axon show` の評価範囲は [候補と外部条件](../reference/candidates.md#評価契約) に定める。Note 本文は `axon note list`、状態変更の前後・理由・変換・解決は `axon log` で読む。`axon log` の記録は共通コアの因果順で、並行する分岐は枝ごとにまとめ、枝の途中の分岐は直近の分岐点へ戻って続け、同じ分岐点から分かれた枝は ID 順、作成の記録からの枝は親記録が保存先にない記録より先であり、時刻順への並べ替えはしない。直前の記録と先後関係がない箇所（枝の切り替わり）には `Concurrent branch` と表示し、逐次操作と区別する。親記録が保存先にない記録には `parent missing` を示す。Note は日時順、同時刻は記録 ID 順。通常表示では端末制御文字をエスケープする。

候補の `axon proposals|tasks` と `axon condition` の設定・評価は [候補と外部条件](../reference/candidates.md) を参照する。記録者は独立crateから取得できた任意情報を添える。`axon storage check` と `axon resolve` は [file 保存と Git 統合](lifecycle-file.md#git-統合と検査) を参照する。

## mutationと出力の境界

通常 mutation は保存先の OS lock を取ってから記録の集合を読み、共通コアへ渡して検査し、一つの記録 file の作成で保存する。異なる Entity の変更や Note 追加も同じ境界で直列化する。出力は保存の後。出力失敗は `storage applied; output failed` と明示するため、再実行の前に保存済み状態を照合する。stdout の BrokenPipe は成功として扱う。保存側の `Not applied` と `Result unknown` の区別は [書込の保証](lifecycle-file.md#書込の保証) にある。

## 内部Git呼出し

内部 Git 呼出しは `GIT_*` の override を除外し、現在 directory を探索の起点にする。Git 内では最寄りの `.git` と Git が返す root を照合し、bare repository・壊れた marker から祖先へ fallback しない。repository が探索境界。探索の段と確定の規則、破損で fallback しないことは [保存と統合の契約](../reference/storage.md#探索) に定める。

## 検証境界

`cargo test --workspace --lib --bin axon --test smoke` は共通コア、登録から Group 完了、入力・表示・出力失敗、並行 `axon start` と Note・別 Entity の保存、衝突・違反・gap の報告と解決、未対応 format と破損の拒否、Git 外の探索・境界、実 linked worktree の共有、並行 `axon init` を独立 fixture で検証する。実管理データも installed `axon` も使わない。

process監督・file保存・統合・記録者の検証は `tests/smoke.rs` と記録者crateで実行する。I/O・CLI の接続はコアの lifecycle・包含・dependency の意味を変更しないため、Quint の状態を加えない。

記録者の自動取得・詳細参照は [記録者連携](lifecycle-recorder.md)、初回の試用手順は [使い始める](../guide/getting-started.md) を参照する。
