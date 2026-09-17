# Backend の選択と file 保存

`axon init [prefix]` の既定は SQLite。`--backend file` は file を選ぶ。
backend 設定ファイルは持たず、所定の正本の存在から判別する。

| 環境 | SQLite | file |
| --- | --- | --- |
| Git | Git common directory の親の `.axon/axon.db` | 現在の worktree root の `.axon/state.jsonl` |
| Git 外 | 管理 root の `.axon/axon.db` | 管理 root の `.axon/state.jsonl` |

Git では現在の repository が境界。SQLite は linked worktree 間で共有し、追加の init は不要。
Git 外の通常操作は最寄りの正本または `init.pending` を持つ祖先が管理 root。
空の `.axon` や lock だけでは探索を止めない。Git 外の init は現在 directory を使うが、
既存管理 root 配下の入れ子 init は拒否する。

観測する二つの正本が両方あれば混在エラー、どちらもなければ未初期化。
破損・読取不能・途中生成はエラーにし、別 backend や祖先へ fallback しない。
store ID は正本内に保持する。旧 config/state.db の互換探索や暗黙移行は行わない。
既存データの採用・切替は[手動移行](migration.md)で扱う。

一つの Git repository に一つの backend を使う。worktree ごとの異種 backend 混在は非対応。
全 worktree は走査しないため、別 worktree だけにある file との混在は検出を保証しない。
file は Git で取り込んだ snapshot だけを読み、同じ Issue を別 worktree で start できる。
正本がない branch では未初期化となる。既存 store を使うならその正本を取り込む。

## init と Git integration

init は新規作成専用。既存正本が valid でも再実行は拒否し、修復・backend 切替はしない。
SQLite init は ignore や attribute、Git config を変更しない。ignore 方法は利用者が選ぶ。
file init は Git 内外とも、次を生成・補完する。

`.axon/.gitignore`:

```gitignore
*
!.gitignore
!state.jsonl
```

root の `.gitattributes`:

```gitattributes
/.axon/state.jsonl merge=axon
```

既存の無関係な行を保持し、同じ必要行を重複させない。直接対象を指定する競合行、
読取不能、通常 file でない編集先ではエラーにする。
親/global ignore が `.axon/` 全体を隠しても変更・拒否しない。
実際に追跡するかは利用者の責任で、init は Git driver 登録、stage、commit を行わない。
既に追跡された file は ignore で追跡解除されない。
成功時は state path の後に `.axon/.gitignore` と `.gitattributes` の絶対 path を毎回列挙し、保存処理の結果を `Created:`、`Appended:`、`Unchanged:` で示す。SQLite init の成功出力は正本の path だけを示す。

### Git で共有・公開される情報

`.axon/state.jsonl` には、利用者が入力した本文に加えて、操作主体（actor）、
作業場所の絶対パス、作成・更新・操作時刻が自動保存される。
actor は実行環境によってエージェント名や `$USER@作業ディレクトリ名` になり、
絶対パスには OS のユーザー名やローカルのディレクトリ構成が含まれる。
取得規則は [actor と作業場所](../development/architecture.md#actor-と作業場所) を参照する。

作業場所は claim だけでなく履歴にも保存されるため、`done` や `release` では
過去の絶対パスは消えない。公開 repository で管理する場合は、これらの自動記録情報も
公開されることを確認する。Git に commit 済みの情報は、現在の file から除去しても
過去の commit に残る。

## 保存の保証と失敗時の確認

file writer は stable sidecar の OS lock を取得してから、正本読取、core 操作、
temporary file 書込と sync、元 bytes と backendの再照合、atomic replace、directory sync
を行う。成功はその後に返す。no-op は空白などの非 canonical な bytes も保持する。
lock file は replace/unlink しない。終了した process の lock は OS が解放する。

置換前の失敗は未適用。置換後の同期に失敗すると「result unknown」を返す。
この場合は writer の終了を確認し、state と backend、対象 Entity・記録 ID を読み、
変更が入ったか照合してから次の操作を判断する。Note 追加を推測で繰り返さない。
残った temporary file は正本ではない。調査・退避してから削除する。

Git index の正本が unmerged の間は、内容が valid でも通常操作を拒否する。
解決済みの内容を確認して stage してから操作する。
Git/editor は sidecar lock に従わず、再照合後の非協調書込を完全には防げない。
同じ worktree の checkout/merge、editor 保存と Axon 書込を同時に行わない。
分散 lock、network filesystem の透過的保証、永続 SQLite cache は提供しない。

## init の中断からの復旧

init は backend 共通の OS lock 下で存在を確認する。pending marker を残し、
完成した temporary を正本として公開してから Git integration を整え、最後に marker を除く。
既存正本を上書きしない。途中失敗では path・操作・保存済み artifact を報告し、自動 rollback しない。

writer を止め、正本・marker・temporary・Git integration file を保全して診断を確認する。
正本が完成していれば検証し、必要な補助 file を手動で整えてから marker を取り除く。
JSONL は `storage check`、SQLite は整合性・schema を確認する。
不完全な正本を採用しない。作り直す場合は残存物を保全先へ移してから新規 init する。
init 再実行による修復や専用の復旧コマンドは提供しない。

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
merge の測定条件と結果は次節に記す。

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

## CLI workspace と Git

`axon storage check <snapshot>` は完全な JSONL の保存情報・参照・構造を検査する。
Command 条件は評価せず、Git index や backend discovery に依存しない。

```sh
axon merge prepare --base /tmp/base.jsonl --ours /tmp/ours.jsonl \
  --theirs /tmp/theirs.jsonl --output .axon/state.jsonl --workspace .axon/merge/manual
axon merge check .axon/merge/manual
axon merge apply .axon/merge/manual
```

workspace の親 directory は事前に用意し、workspace 自身は未使用の名前を指定する。
出力先は workspace の外に置く。原本や管理ファイルを含む workspace 内への出力は、
path の別名も解決して prepare/check/apply で拒否する。
prepare は正本を変更せず、未解決でも原本と診断を残して非0を返す。
`base.jsonl` / `ours.jsonl` / `theirs.jsonl` は原本、`preimage` は出力先の bytes、
`manifest.json` は入力の絶対 path/digest・出力先・active root と入力の store identity、`context.json` は
固定日時・actor・操作実行 directory・ID seed。これらは編集しない。
原本 snapshots は全 Entity の辺・Revision・Note・typed history・因果参照を含み、
`choices.json` は対象 Entity、stable choice ID、三側の bundle、自動選択 digest を示す。
`report.json` の status は valid / unresolved / input_drift / invalid を区別する。
候補が完全に valid の場合のみ `candidate.jsonl` と `checked.json` が公開可能になる。

エージェントは report と choices、必要な owner/record ID の原本行を読み、
`resolution.json` の choices と repairs を編集する。選択元は ours/theirs の文字列ではなく
manifest にある入力 digest。base は比較材料で、現在値の選択元にはしない。
自動選択済み Entity も明示選択できる。

```json
{
  "choices": [{"conflict": "<choices.json の id>", "input": "<ours または theirs の digest>"}],
  "repairs": [{
    "operation": {"op": "dependency", "source": "t-b", "target": "t-a", "present": false},
    "reason": "統合で生じた循環を解消する"
  }]
}
```

repair は `dependency`、`add_note` (`owner`, `body`)、`change` (`owner`, `change`) と
`start` (`owner`)。start の claim は固定 context の actor・日時・管理 root から作る。
change に自由な Start claim を渡すことはできない。
change は共通 core の JSON 表現、例 `{"SetTitle":"新題名"}`、
`{"Decide":"Undecided"}`、`{"SetParent":null}`、`"Done"`、`"Release"`。
決定済み declaration を編集する場合は Undecided 化と再判断を明示する。
固定 root は active file 出力の管理 root、それ以外は起動位置の通常管理 root
（管理外では起動 directory）。操作は core の guard と履歴生成を通り、必要な ready 検査だけが固定 root で Command
条件を実行する。原本履歴の編集や claim 単独の置換は提供しない。
編集後の check は候補を再計算し、失敗時は以前の適用許可を失効する。
apply は最後に検査した組と入力・backend・出力先 preimage を lock 下で再照合する。
出力 file またはその `.axon` directory が symlink の場合は拒否する。
入力の store identity 検査と通常 writer の lock を別 path への解決で外さないためである。
原本 path の bytes も drift 検査するため、一時ファイルを削除しない。
同じ候補が出力先にあれば再 apply は no-op。結果不明では原本と候補を保持して照合する。

driver は install 後に各 repository で通常の Git config に登録する。
`axon` が PATH 上に必要。clone には Git config が引き継がれないので登録が必要になる。
`--global` は必須ではなく、全 repository で使いたい場合の利用者の選択。
`axon merge setup` は提供しない。attribute と ignore は file init が用意する。
Git の内部祖先統合は binary driver を使い、空・不正・曖昧な祖先を推測して合成しない。
add/add や delete/modify も、完全な共通入力を構成できなければ手動解決へ返す。
Git が driver を呼ばない場合も通常 open は index の未解決を拒否し、snapshot を検証する。

```sh
# 一時 repository などの独立 fixture で試す例
axon init --backend file t
git config merge.axon.name "Axon validated snapshot merge"
git config merge.axon.driver "axon merge driver %O %A %B"
git config merge.axon.recursive binary
# 利用者: git add .axon/.gitignore .axon/state.jsonl .gitattributes; git commit ...
# 利用者: git worktree add ../feature -b feature
# feature 内の axon 操作は main の bytes を変えない
# feature の変更を利用者が commit した後、main で git merge feature
```

同一 Entity の競合では Git index を未解決のまま残し、driver は raw 三入力を
`.axon/merge/<id>/` へ保全する。表示された workspace の原本を明示 prepare の入力にし、
実際の `.axon/state.jsonl` を output とする新 workspace を作る。Git の marker は
出力 preimage として保持され、状態入力には使わない。driver の workspace は Git の
一時 `%A` を保存先としており、そのまま手動 apply する用途には使わない。
driver 未登録でも Git の stage 1/2/3 や保全した完全 snapshot を明示入力として使える。
解決後は storage check で確認し、利用者が stage/commit/rebase 継続等を行う。
Axon はそれらの Git 操作を自動実行しない。

2026-09-06、ローカル release binary、Git 外の一時 directory、100 Entity（各1 creation
baseline）、別 Entity の title を両側で変更した三入力で CLI prepare を5回別 process
実行した中央値は **108.89 ms**（base 104,317 bytes）。原本・context・候補・report の
書込と sync を含み、Git process と apply は含まない。上の engine 単体測定とは条件が異なる。
