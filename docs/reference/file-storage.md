# Backend の選択と file 保存

`axon init --backend file [prefix]` は active root の `.axon/state.jsonl` を作る。
`axon init [prefix]` の既定は SQLite。Git では現在の worktree root、Git 外では
init は現在 directory、通常操作は最寄りの `.axon/config.json` を持つ祖先を使う。
既存管理 root の配下で Git 外の入れ子を暗黙には初期化しない。

設定 schema 1 の例:

```json
{
  "schema": 1,
  "backend": "file",
  "store_id": "store-0123456789abcdef0123456789abcdef"
}
```

store ID は init が生成した正本の ID と一致する必要がある。prefix は設定へ複写しない。
設定・正本が欠落、不正、異なる store ID の場合は他 root、SQLite、空 state を開かない。
Git 外でも途中生成の正本・pending marker がある root を飛ばして祖先へ進まない。
backend は通常操作の flag では変更しない。既存データの採用・切替は手動移行で扱う。
旧 `.axon/axon.db` は通常 open/init で変換しない。

SQLite は Git common directory の `axon/state.db`、Git 外では `.axon/state.db` を使う。
別 worktree の file と共有 SQLite は共存できる。別 worktree から既存 valid SQLite を
登録するときは `axon init --backend sqlite` が設定だけを作る。DB を再初期化しない。

file の設定と正本を Git へ追加し、`.axon/write.lock`、`.axon/init.pending`、
`.axon/.*.tmp` は ignore する。init は Git の設定・index を変更しない。
clone 済みの設定と正本はそのまま利用でき、共有 binding は不要。
同じ Issue を別 worktree で start でき、変更はその worktree 内だけに保存される。

## 保存の保証と失敗時の確認

file writer は stable sidecar の OS lock を取得してから、正本読取、core 操作、
temporary file 書込と sync、元 bytes と設定の再照合、atomic replace、directory sync
を行う。成功はその後に返す。no-op は空白などの非 canonical な bytes も保持する。
lock file は replace/unlink しない。終了した process の lock は OS が解放する。

置換前の失敗は未適用。置換後の同期に失敗すると「result unknown」を返す。
この場合は writer の終了を確認し、config と state、対象 Entity・記録 ID を読み、
変更が入ったか照合してから次の操作を判断する。Note 追加を推測で繰り返さない。
残った temporary file は正本ではない。調査・退避してから削除する。

Git index の設定または正本が unmerged の間は、内容が valid でも通常操作を拒否する。
解決済みの内容を確認して stage してから操作する。
Git/editor は sidecar lock に従わず、再照合後の非協調書込を完全には防げない。
同じ worktree の checkout/merge、editor 保存と Axon 書込を同時に行わない。
分散 lock、network filesystem の透過的保証、永続 SQLite cache は提供しない。

## init の中断からの復旧

init は既存正本を上書きしない。新規生成は pending marker を残し、完全な正本を先に、
設定を最後に公開する。設定と正本が有効なら再実行は no-op。
片側しかない場合や、設定公開前に中断した場合は自動で空 state を作り直さない。

まず writer を停止し、表示された root、設定、正本、pending marker と temporary file を
まとめて保全する。正本が valid な場合は、その backend と store ID に一致する
保全済み設定を復元する。設定しかない場合は一致する正本を backup から復元する。
不完全な正本を使わない。入力が復元できない場合は既存物を保全したまま、別の空 directory
で新規 init する。設定だけを書き換えて別 backend のデータを流用しない。
有効な config/state が揃った後の残存 pending marker は調査後に削除できる。

## 検証と測定

2026-09-06、ローカル release build、Git 外、一 Entity あたり Accepted Revision 1、
判断記録 1、Note 3 の合成 fixture を使い、各 CLI を別 process で5回実行した中央値。
時刻・生成 ID・外部評価 context を固定した core 契約テストでは、SQLite/file の
状態・全履歴・error・no-op を直接比較する。process 終了、置換前後の fault、入力 drift、
並行 writer、init 中断、worktree/clone と index の統合テストは別途行う。

| Entity 数 | 正本 bytes | ready | show（1 Entity） |
| --- | ---: | ---: | ---: |
| 100 | 245,041 | 23.40 ms | 23.76 ms |
| 1,000 | 2,449,141 | 393.12 ms | 390.24 ms |

これは上記条件の観測値であり性能上限の保証ではない。read は完全な snapshot を検査する。
merge 性能は merge engine の実装段階で測定する。

## 三方向統合 engine

共通 engine (`src/merge.rs`) は base / ours / theirs の完全 bytes を所有し、
入力 digest と Entity ごとの選択 ID を返す。入力を差し替えた古い選択、重複選択、
存在しない選択元を拒否する。Entity 選択の一覧には自動選択も含まれ、
循環などの全体競合を解決するときは自動選択も明示的に変更できる。

選択は入力 digest を指す。通常修正は共通 core の Operation と、artifact 内で固定した
日時・actor・reason・ID allocator、必要な評価 context を渡す。
同じ artifact の再計算では同じ ID 列を再現する。engine は修正候補を外へ返さず、
検証済みの結果または競合診断を返す。CLI workspace と Git driver、原子的な publish は
この API の呼び出し側が所有する。

Ended と未終了の選択でも全記録を保持し、MergeRecord は元の両候補、入力 digest、
親先端、選択元と結果を保存する。取り込み済み候補は因果的に到達できる MergeRecord
から判定し、後から入力側に加わった draft 変更まで取り込み済みとはみなさない。
Note と Revision の表示順は因果順を優先し、並行記録だけを ID 順に並べる。

2026-09-06、ローカル release test build、各 Entity に作成 baseline 1件の合成 snapshot、
両側で別 Entity の Resurface を変更、完全な三入力 decode・prepare・resolve・encode を
5回計測した中央値。ファイル I/O と Git process は含まない。

| Entity 数 | base bytes | merge |
| --- | ---: | ---: |
| 100 | 100,237 | 9.72 ms |
| 1,000 | 1,001,137 | 312.05 ms |

再現: `cargo test --release --bin axon merge::additional_tests::measure_merge -- --ignored --nocapture`。
