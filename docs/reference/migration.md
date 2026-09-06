# SQLite v11/v12/v13 から SQLite/file への手動移行

`axon migrate --source <DB> --output <未使用directory> --backend <sqlite|file>` は、
通常の root 探索を行わず指定 SQLite を読み取り専用で開く。元 DB・設定・root は切り替えない。
v11 は既存の安定 ID 変換で v12 にし、v12 の因果履歴変換で v13 にする。v13 は既存の
store/record ID、baseline、MergeRecord、因果参照を保持する。最終 SQLite は schema v13、
file は canonical JSONL format 1。未知の版・DDL は拒否する。v9/v10 は旧版で v11 にしてから使う。

## 入力の保全と変換

1. 旧版と候補版のバイナリを別 path に実体コピーし、版の由来と digest を残す。
   共有 PATH のバイナリを更新する場合は、全利用 root と linked worktree、その backend・設定・
   実際の DB path を棚卸しする。旧 SQLite は `.axon/axon.db`、現行 SQLite は Git common directory の
   `axon/state.db`（Git 外では `.axon/state.db`）。file の採用は root ごとに明示して決める。
2. 隔離したコピーで移行を試す。本切替ではすべての writer を止め、停止後の最新入力を使う。
   元 DB と WAL/SHM、設定、バイナリを一組で保全する。Axon 自身の DB を移す間は、
   実施台帳を対象 DB の外へ置き、旧 DB へ Note 等を追加しない。
3. 固定した新版の絶対 path と未使用の出力先で変換する。

   ```sh
   /absolute/path/to/new-axon migrate --source /root/.axon/axon.db \
     --output /backup/new-conversion --backend file
   ```

   SQLite を選ぶ場合は `--backend sqlite` とする。backend は省略できない。
   SQLite backup API が WAL 内の確定済み書込を含む一貫した入力を固定する。
   出力は以下を持つ。移行先は自動探索される管理 root ではない。

   | artifact | 内容 |
   | --- | --- |
   | `source-v11.db` / `source-v12.db` / `source-v13.db` | 固定した元入力。単独で整合した SQLite backup |
   | `staging/linear/` | v11 入力時の v12 変換と旧番号対応表 |
   | `staging/causal/` | v11/v12 入力時の v13 変換 |
   | `state.db` または `state.jsonl` | 選択 backend の正本 |
   | `config.json` | schema 1、backend、正本と同じ store ID |
   | `snapshot.jsonl` | 全最終状態の比較用 canonical snapshot |
   | `manifest.yaml` | 最後に公開する format 2 の完了 manifest |

   最上位 manifest は source/target schema、backend、store ID、全元 table と最終 table の件数、
   入力の論理 digest、backup・正本・設定・snapshot の BLAKE3、対応表、検証結果を持つ。
   各段階の manifest はその段階の記録であり、最上位 manifest の代用にはならない。
   全旧 field/row の比較、参照と型、因果整合、canonical 往復、最終 backend の再読取を検査する。

v11 の `mappings` は `table`・`entity`・`old_number`・`id` の組で旧参照を追跡する。
たとえば本文が「Note 2」を参照するときは、その Entity と `table: entity_notes`、
`old_number: 2` の行の `id` を使い、`axon note show <Entity> <id>` で読む。
Revision は `declaration_revisions`、判断/進行履歴は各旧 table の row key に対応する。
本文や legacy reason 自体を書き換えない。v12/v13 入力の `mappings` は空で、既存 ID を維持する。
先行移行の番号対応表が必要なら、その manifest も一緒に保管する。

各旧 stream 内の保存順だけを先行参照にし、別 table 間の時系列を推測しない。
v11 からの直出力も v12 を経由するため、同じ v12 から段階移行した出力と ID・payload・参照が一致する。
同じ固定 v11 入力は同じ ID を生成するが、別時点の v11 を独立変換したものを共通起点としない。
一度作った成果を branch/worktree へ配布する。v12/v13 は入力 digest が変わっても既存 ID を付け直さない。

## 検証して root を切り替える

1. 成功終了と最上位 manifest を確認し、記載された artifact の digest を照合する。
   `new-axon storage check /backup/new-conversion/snapshot.jsonl` でも全 snapshot を検査できる。
   別の Git 外の検証 directory に `.axon` を作り、`config.json` と選んだ `state.db` または
   `state.jsonl` をコピーする。その directory から候補版で一覧・show・Note・Revision・履歴を読み、
   コピーだけで通常更新も試す。更新した検証コピーを本番に配置しない。
   `list` 等が Command 条件を評価する場合は、保存された実行内容を事前に確認する。
   宣言 export は全情報 backup の代用にならない。
2. writer 停止を維持して、必要な新版の配布・build を行う。各利用先で実行する binary を照合する。
   旧 DB と WAL/SHM、既存設定を退避してから、検証済み・未更新の成果を次の場所に配置する。

   | backend | 正本の配置先 | 設定の配置先 |
   | --- | --- | --- |
   | file | active worktree root の `.axon/state.jsonl` | 同 root の `.axon/config.json` |
   | SQLite / Git | `git rev-parse --path-format=absolute --git-common-dir` が示す directory の `axon/state.db` | 各利用 worktree root の `.axon/config.json` |
   | SQLite / Git 外 | 管理 root の `.axon/state.db` | 同 root の `.axon/config.json` |

   正本を先、対応する設定を最後に配置し、ファイルと directory を同期する。元 WAL/SHM を新 DB と
   混ぜない。SQLite を共有する worktree は同じ store ID の設定を使う。旧 `.axon/axon.db` を新 CLI が
   通常利用することはない。既存 root の切替に `init` で空 state を作らない。
   file の設定と正本は Git で追跡し、`.axon/write.lock`、temporary file、merge workspace は ignore する。
3. 各利用 directory から新版で意図した root と全情報を読めることを確認する。全共有利用先が
   互換な組になってから writer を再開する。旧バイナリ・backup・対応表・台帳は保管する。

## 中断、再試行、旧データへの復帰

出力先が存在すると再実行は拒否する。失敗時も backup と staging は削除しない。
最上位 manifest 公開前の失敗は移行成果未適用、公開開始後の同期失敗は結果不明として診断する。
いずれも元 root の切替は行われていない。manifest がない、読めない、digest が合わない成果を正本にしない。
結果不明ならプロセス終了を確認し、出力・manifest・digest を照合して台帳へ記録する。
必要なら保全された整合 backup を入力に、別の未使用 directory へ再変換する。同じ出力先を消して再利用しない。

手動切替の中断は root ごとに未適用・適用済み・結果不明を記録する。writer を止めたまま現状を
保全し、旧バイナリ・旧設定・旧 DB の組を元の配置先へ戻す。新 DB やその WAL/SHM を旧 DB と混ぜず、
固定した単独 backup を復元するか、停止時に保全した DB/WAL/SHM 一式を整合した組で復元する。
file に切り替えた root では新 file 設定を退避し、旧版の探索・設定に戻す。
新 CLI での書込再開後に古い backup へ戻す場合は、先に新状態全体を保全し、新規変更をどう引き継ぐか決める。
旧バイナリを新 DB へ向けたり、`user_version` だけを変えて互換性検査を迂回しない。
