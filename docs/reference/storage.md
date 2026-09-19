# 保存と統合の契約

保存形式はJSONL一つとし、保存先は管理rootの `.axon/state.jsonl` に置く。保存adapterは共通コアの状態操作と全体検査を通し、保存方式に依存する部分だけを閉じる。実装の手順と失敗境界は [file保存とGit統合](../development/lifecycle-file.md) と [CLIと保存の接続](../development/lifecycle-cli.md) に定める。形式を一つにした理由は [設計判断](../design/decisions.md#保存形式を一つにし配置を-git-の運用に委ねる理由) にある。

## 共通モデルが持つもの

共通モデルは、保存方式からの状態操作の分離に加えて、記録の安定ID、現在値と分岐した記録の分離、因果関係と統合記録を持つ。Gitで分岐した保存先を統合する要件は、保存adapterだけでは満たされない。

## 分岐の統合で保持するもの

分岐の統合では、両側で実際に発生した状態変更履歴・Noteを保持する。分岐した操作を一本の操作列へ並べ直さず、統合後の現在値を選んだことは、通常のlifecycle遷移と区別した統合記録に残す。記録の同一性と分岐間の先後関係は安定した記録IDと先行関係で決め、保存順の連番や日時を根拠にしない。

同じ進行中のEntityから、一方が完了、他方が作業を解放して未着手になった場合、統合時には未完了側を明示採用してよい。完了した側の履歴も残し、何を選んだかを統合記録から確認できるようにする。これは入力にある分岐の現在値を選ぶ操作であり、通常操作に`Completed`からの再開経路を追加するものではない。選択だけで包含・dependency・終了済みGroupの構成に関する制約を免除しない。候補全体の整合性と、採用する終了状態に付随する構成の固定を検査する。

統合後の現在値は統合記録も根拠に含むため、通常遷移の列へ還元しない。統合を通すために、通常操作の完了固定と履歴の追記専用性を緩めない。

## 保存先と初期化

`axon init [PREFIX]` は保存先を新規作成する専用の操作とする。配置や形式を選ぶoptionは持たない。既存の正本があれば、内容が有効でも再実行を拒否し、修復・暗黙の変換を行わない。

prefixはEntity IDの先頭に使い、ASCIIの小文字英数字とハイフンだけを許す。空文字と、先頭・末尾のハイフンは受け付けない。明示した値は変換せずに検証する。省略した場合は管理rootのdirectory名のASCII大文字を小文字にした結果を使い、それがこの規則に合わなければ保存先を作らずに失敗し、明示指定を求める。

`axon init` が保存先として作るのは正本 `.axon/state.jsonl` だけで、ほかに残すのはOS lock用のfileに限る。Gitに関するfileは作らない。storeの識別子とprefixは正本の中に保持する。

### 無視する運用と追跡する運用

Gitが保存先をどう扱うかにAxonは関与しない。`axon init` の直後、正本はGitからuntrackedに見える。Git内での保存先の使い方は二つあり、利用者のGitの運用だけで決まる。Axonは二つを区別せず、設定にも持たない。一つのrepositoryでは、どちらか一つに揃える。

- 無視する運用。利用者が `.git/info/exclude` やglobalのignoreなどで `.axon/` をGitに無視させる。linked worktreeには `.axon` が現れないため、下の探索順によって全worktreeがmain worktreeの保存先を共有し、worktreeごとに`axon init`を繰り返す必要はない。
- 追跡する運用。利用者が正本だけを追跡対象にする `.axon/.gitignore` を作り、`.gitattributes` に `merge=axon` を宣言し、Git merge driverを登録する。各worktreeはcheckoutした自分の正本を持つ。変更はGitで取り込むまで他のworktreeから見えず、同じIssueに別々に着手できる。分岐した正本は [自動統合](#自動統合の範囲) と [明示的な解決](#解決の入口) で統合する。

`axon init` はGit内では二つの運用の手順を表示するだけで、`.gitignore`、`.gitattributes`、Git configを作成も編集もせず、実効merge属性も検査しない。stage・commitは利用者が行う。

Gitはuntrackedなfileをcheckout・mergeの上書きから保護するが、無視されているfileは保護しない。無視されている正本がある作業directoryで、`.axon/state.jsonl` を追跡しているcommitをcheckout・mergeすると、Gitは警告なしに正本を置き換え、記録が失われる。これは無視する運用と追跡する運用を一つのrepositoryで混ぜた場合にだけ起きる。Axonはこの混在を検出しない。

merge driverの登録が漏れると、Gitは正本をtextとして統合する。その結果も次の読み取りで全体検査を受けるため、conflict markerや不整合を含む正本は拒否され、壊れた状態のまま使われることはない。離れたEntityへの変更どうしはtextとして統合でき、両側の記録を併せて整合していれば検査を通る。この場合、設定漏れは同じEntityを両側で変更して衝突するまで表に出ない。

### 探索

Git内では現在のrepositoryを探索境界とし、次の順で保存先を選ぶ。

1. 現在のworktree rootの `.axon`。
2. main worktreeの `.axon`。現在のworktreeがlinked worktreeで、Git common directoryがmain worktree直下の `.git` directoryである場合だけ探す。bare repositoryに付けたworktreeとsubmoduleでは探さない。

各段では、正本または初期化途中のmarkerがあればその保存先に確定する。空の `.axon` やlockだけでは確定せず次へ進む。どの段でも確定しなければ未初期化とする。確定した保存先が破損・読取不能・初期化途中であれば停止し、別の保存先へfallbackしない。

追跡する運用で、正本を持たないbranch（`axon init` より前に分岐したbranchなど）のlinked worktreeから操作すると、2によってmain worktreeの追跡対象の正本を書き換える。変更はmain worktreeの差分として見え、記録は失われないため、この副作用は許容する。main worktreeで正本を持たないbranchをcheckoutした場合は未初期化になる。既存のstoreを使うには正本をGitで取り込む。そこで`axon init`を実行すると別のstoreの新規作成になり、後で統合できない。

通常操作のOS lock、Git indexのunmerged検査、管理directoryが通常のdirectoryであることの検査（symlinkの拒否）は、現在のworktreeではなく、確定した保存先と、それを含むworktreeに対して行う。

Git外では最寄りの正本または初期化途中のmarkerを持つ祖先を管理rootとし、空の `.axon` やlockだけでは探索を止めない。

### 初期化の対象

`axon init` は探索の2を使わず、Git内では現在のworktree root、Git外では現在directoryを対象とする。Git外では既存管理root内の入れ子初期化を拒否する。

探索の2が働く構成のlinked worktreeでの `axon init` は、main worktreeに正本または初期化途中のmarkerが既にあれば拒否し、そのpathを示す。無視する運用では手前に保存先ができて読む先が気づかないまま切り替わり、追跡する運用では別のstoreができて後で統合できなくなるためである。main worktreeに正本がない場合と、探索の2が働かない構成では拒否せず、作成した保存先が他のworktreeからは見えないことを表示する。main worktreeを判定できなかった場合は、未初期化として扱わずにエラーとする。それ以外の取り違え（読む先の思い違い、worktreeの削除による保存先の消失）は利用者の運用に委ね、`axon init` は既存の保存先を壊さないことだけを保証する。

`axon init` はOS lockの下で既存の正本を確認し、初期化途中を示すmarkerと同期済みの一時fileを作り、既存の正本を上書きせずに公開して、最後にmarkerを除く。初期化の中断を自動修復や自動rollbackで隠さず、保存済みartifactと失敗箇所を示す。成功時は作成した正本のpathを示す。

## 自動統合の範囲

自動統合はEntity単位で現在値を選ぶ。片側だけの変更は変更側、両側が同じ現在値ならその値を採用する。同じEntityを両側で異なる現在値へ変更した場合は衝突とし、異なる項目の変更でも自動で混ぜない。項目ごとに混ぜると、たとえば本文変更と完了を合成して、変更後の仕事が完了したと推定してしまうためである。

別Entityの変更は組み合わせ、Note・実際の状態変更履歴は両側を保持する。その結果も候補全体で検証し、各入力が有効でも、依存の循環や終了したGroupの構成に関する違反が生じれば適用しない。

## 解決の入口

`axon merge prepare` は入力を保全して衝突内容と解決用ファイルを用意し、正本を変更しない。`axon merge check` は明示的な選択・修正から候補を計算し、計画全体の整合性を検査する。`axon merge apply` は検査した入力と保存先が変わっていないことを再照合して適用する。出力先は探索で確定した保存先の正本に限る。未解決・不正な候補を部分適用せず、古い検査結果で変更後の入力や保存先へ適用しない。Git driverも同じ自動統合・全体検証を使い、解決できなければ明示的な解決へ渡す。

解決用ファイルの具体的な形式は [file保存とGit統合](../development/lifecycle-file.md#明示的な統合) に定める。通常操作と異なる現在値の選択、全記録の保持、通常の修正操作の制約を区別する。統合の検査・修正で外部条件コマンドを実行しない。
