# 計画全体の取得と一括編集

この文書は、指定したIssue・Groupを一つのYAMLのdeclarationとして取得し、一括で登録・編集するときの契約を定める。コマンドの入出力は [CLIと表示の契約](cli.md)、保存境界の扱いは [保存と統合の契約](storage.md) に従う。

## 目的と用途

指定したIssueと、指定したGroupおよびその全子孫を一つの declaration として取得し、登録・修正できるようにする。主な用途は二つある。新しく大きめの計画を一括で登録すること、登録後に不備が見つかったときに計画全体を取得して context に入れ、役割・完了条件・依存の矛盾を直してまとめて反映することである。Issue 単位の操作だけでは、エージェントが他の Issue の内容を見落とし、矛盾に気付けない。取得した declaration はファイルとして別のレビューにも渡せる。

満たす安全性は、計画全体と変更差分を保存前に検査できること、取得後に編集の前提が変わっていたら競合として止めること、部分適用を作らないこと、障害後の再実行で新規 Entity を重複作成しないことである。

この機能は lifecycle・包含・dependency・終了構成の制約を迂回せず、通常操作と同じ共通コアの検査を通す。衝突中の Entity か構造の違反がある保存先では `prepare`・`check`・`apply` を拒否し（途中で止まった反映の [再試行](#再試行) だけは残りの反映で消える違反を除く）、先に `axon resolve` と通常操作で直す。通常操作は無関係な違反があっても進むが、一括反映は多数の Entity に触れ、違反を増やさない判定を Entity ごとに説明しにくいため、違反のない保存先だけを対象にする（[保存と統合の契約](storage.md#衝突と通常操作)）。`axon export` は選んだ Entity か参照先が衝突中なら拒否し、違反では拒否しない。再浮上条件と Note は扱わない。既存 Entity の lifecycle 遷移と種類の変換も扱わず、`axon accept|start|complete|cancel|reopen`・`axon convert` などの通常コマンドに任せる。

## 対象範囲と編集集合

`axon export ID...` は一つ以上の ID を受け取り、Group ならその Group と全子孫、Issue ならその Issue を選ぶ。引数の ID は他のコマンドと同じく完全 ID または一意な suffix を受け付けるが、declaration 内の `id` と参照は完全 ID だけを使い、suffix は未解決として拒否する。複数指定は和集合とし、重複は除く。`Completed`・`Cancelled` の子孫も含める。lifecycle が読み取り専用で見えるため固定済みと分かり、除外すると「載せていないので触らない」と「終了済み」がファイル上で区別できないためである。保存先全体を一括で取得する selector は設けない。

declaration の `issues` と `groups` に載っている Entity だけが編集集合である。載っていない Entity は変更しない。編集集合を広げるには対象を足して再度`axon export`する。既存 Entity の record を手で書き足しても `base` は計算できず、`base` が null の record は新規 Entity とみなされて既存 ID との衝突が競合になる。ファイルから Entity を消すことは、削除、取りやめ、所属解除、依存解除のいずれも意味しない。Group から外すには record に `parent: null` を宣言し、作業自体をやめるには通常の`axon cancel`を使う。編集集合の各 Entity については、title、description、label、親、outgoing dependency を完全に宣言する。親の不在は `parent: null`、outgoing dependency の不在は `needs: []` で表す。

## declaration の形式

形式は strict YAML とし、schema label は `axon-declaration/v2` とする。v2 は v1 の record に必須の `label` field を加えた形式である。次は既存 Group の subtree に新規 Issue を一件追加し、subtree 外の Issue へ依存を張る、`prepare` 前の例である。fingerprint は例示値である。

```yaml
schema: axon-declaration/v2
groups:
  - id: demo-k3m7pq
    key: null
    base: "blake3:1111111111111111111111111111111111111111111111111111111111111111"
    lifecycle: not-started
    title: 検索画面を実装する
    description: |-
      ## 目的

      検索フォームと結果表示を揃える。
    label: feat
    parent: null
    needs: []
issues:
  - id: demo-8bxw2r
    key: null
    base: "blake3:2222222222222222222222222222222222222222222222222222222222222222"
    lifecycle: completed
    title: 検索 API を実装する
    description: レスポンス形式を固定した。
    label: feat
    parent: { id: demo-k3m7pq }
    needs: []
  - id: demo-c9d4ts
    key: null
    base: "blake3:3333333333333333333333333333333333333333333333333333333333333333"
    lifecycle: not-started
    title: 検索フォームを作る
    description: ""
    label: feat
    parent: { id: demo-k3m7pq }
    needs:
      - { id: demo-8bxw2r }
  - id: null
    key: results
    base: null
    lifecycle: not-started
    title: 検索結果を表示する
    description: フォームの送信結果を一覧に出す。
    label: feat
    parent: { id: demo-k3m7pq }
    needs:
      - { id: demo-8bxw2r }
      - { id: demo-zz01ab }
references:
  - id: demo-zz01ab
    kind: issue
    lifecycle: not-started
    title: 一覧の共通コンポーネントを作る
```

root は `schema`、`groups`、`issues`、`references` の 4 field だけをこの順で持つ mapping とする。すべて必須で、空の list も `[]` として書く。Group の下に子を入れ子で書く形は採らない。

`groups[]` と `issues[]` の要素は次の field をこの順で必ず持つ。kind は格納された list から決まるため、要素には書かない。

| field | 型 | 編集可否 | 契約 |
| --- | --- | --- | --- |
| `id` | string または null | identity | 既存 Entity と `prepare` 済みの新規 Entity では公開 ID。`prepare` 前の新規 Entity だけ null。ASCII 小文字英数字とハイフン以外を含む値は拒否する |
| `key` | string または null | file 内のみ | file-local の別名。`^[a-z][a-z0-9-]{0,63}$` に一致し、`groups`・`issues` を通じて file 内で一意。保存先には保存しない |
| `base` | fingerprint または null | 読み取り専用 | `axon export` 時点の値の fingerprint。未適用の新規 Entity だけ null |
| `lifecycle` | `undecided` / `not-started` / `in-progress` / `completed` / `cancelled` | 既存は読み取り専用 | 保存された lifecycle（保存値）。Group の実効値は書かないため、Group は `in-progress` にならない。新規 Entity は `undecided` か `not-started` のどちらかを書く |
| `title` | string | 編集可 | 空または空白だけの値、改行や制御文字を含む値、200 文字を超える値を拒否する。それ以外は保存値をそのまま扱う |
| `description` | string | 編集可 | Markdown 本文。空文字は本文なし。trim・正規化をしない |
| `label` | string | 編集可 | [label](lifecycle.md#label) の固定集合の値のどれか（`bug`・`feat`・`chore`・`docs`・`test`・`refactor`・`spike`）。必須で、null・空文字・集合外の値を拒否する。新規 Entity は登録時の label を書く |
| `parent` | 参照 または null | 編集可 | 親 Group。Issue を親にする参照は拒否する |
| `needs` | 参照の list | 編集可 | outgoing dependency。空なら `[]` |

`base` が null の record を新規 Entity、non-null の record を既存 Entity とみなす。`id` が null なら `base` も null でなければならない。新規 Entity の record は `key` を持たなければならず、`prepare` 後に `id` を得ても `key` を消してはならない。既存 Entity に `key` を付けて別名で参照してもよい。同じ Entity を二度宣言してはならない。既存 Entity の kind は`axon export`時点の現在値である。kind は`axon convert`で変わるが、declaration は変換を扱わず、record を `issues` と `groups` の間で移すことは kind の書き換えとして拒否する。`axon export` の後に変換された Entity は保存先の kind と一致しないため拒否され、再度`axon export`が必要になる。この拒否は record を移した場合と同じ kind の不一致として診断され、保存先側の変換とは区別しない。lifecycle の綴りは`axon list --lifecycle`と同じにするが、値は `axon list` が示す実効値ではなく保存値である。例の Group は完了済みの子を持つため実効値が `InProgress` で、`axon list --lifecycle in-progress` に当たるが、declaration には保存値の `not-started` を書く。

参照は `{ id: demo-8bxw2r }` または `{ key: results }` のどちらか一方だけを持つ mapping とする。一つの参照 mapping への両方の併記、どちらもない mapping、未解決の ID・key はエラーである。素の文字列は使わない。解決後に同じ Entity を指す重複した `needs` はエラーとし、自己依存も拒否する。

親は child が、dependency は dependent が所有する。record に書く `parent` と `needs` はその Entity の所有物だけであり、外部から編集集合への incoming な包含・dependency は record に現れないため編集できない。declaration は incoming edge を表示しない。必要なら`axon show ID --details --skip-conditions`で確認する。

## 外部参照

`references` には、編集集合の `parent`・`needs` が指す編集集合外の Entity を、`id`、`kind`（`issue` / `group`）、`lifecycle`、`title` の順で一件ずつ読み取り専用として載せる。description、label、key、関係は持たない。レビュー側が subtree 外の依存先を ID の突き合わせなしに読めるようにするための context である。

`references` は `prepare` と `apply` 後の canonical rewrite で保存先の現在値から再生成する。`check`・`apply` は `references` について、要素の集合が編集集合の `parent`・`needs` が指す編集集合外の Entity の集合と一致すること、および file に書かれた値どうしの形式と ID 順が canonical であることを検査し、kind は保存先の現在値との一致を要求する。不一致は、file の書き換えと`axon export`後の保存先での変換を区別できないため、参照先の kind が保存先と一致しないことを対象 ID とともに診断して拒否し、`prepare` での再生成を案内する。`references` の `lifecycle` も保存値である。title や lifecycle の値が保存先の現在値と一致することは要求しない。過不足があれば `prepare` で再生成する。参照先の title や lifecycle が`axon export`の後に変わっても競合にせず、参照先が存在しない場合、kind が一致しない場合（参照先が`axon export`の後に変換された場合を含む）、および共通コアが拒否する状態（終了した Group を親に指定する、着手済み・完了済みの Entity を、自身か祖先が採用済みでない Group の下へ移す、外部を経由して循環を作るなど）だけを止める。`Cancelled` の Entity への依存は共通コアが通常操作で許すため、declaration でも拒否しない。編集者はそれらの値を見て編集したのではなく、参照先の値の変化まで競合にすると、大きな計画ほど無関係な変更で止まるためである。

## canonical 形式

canonical serializer は次の規則で出力する。

1. mapping の key はこの文書の表と例に示した順序で出す。alphabetical にしない。
2. `groups`・`issues` の要素は、`base` が non-null の要素を保存された作成日時順（同時刻は ID 順）、その後に `base` が null の要素を key 順で並べる。この節の ID 順・key 順はすべて bytewise UTF-8 昇順とし、locale を使わない。`axon list`と同じ並び方の規則であり、乱数 ID 順にしない。作成日時は declaration に書かず保存先から取るため、canonical 形は保存先から読んだ記録の集合に対して定まる。`check`・`apply` は検証に使うのと同じ記録の集合で canonical 形を判定し、`references` の要素は file 内の値の形式と順序だけを判定する。作成日時は保存後に変わらない。
3. `needs` は解決後の ID 順に並べ、`prepare` 前で ID を持たない参照先はその後に key 順で並べる。`references` は ID 順に並べる。canonical 規則は `prepare` 前後の file、`apply` 後の rewrite、`axon docs declaration --example`の出力のすべてに適用する。
4. duplicate な Entity、key、解決後の依存は許可せず、sort で潰さない。
5. block context の string のうち、改行を含み、改行が LF だけで、非空行が 1 行以上あり、末尾の改行が 0 個または 1 個、最初の非空行が空白で始まらず、空白文字だけからなる行や末尾空白を含まないものは literal block で出す。長さ 0 の空行は含んでよい。末尾改行がなければ `|-`、1 個なら `|` とする。flow context（参照 mapping の中）では literal block を使わない。改行を含まない string は plain scalar とし、出力する context（block か flow か）で plain scalar として同じ string に戻らないもの（空文字、null・`~`・真偽値・数値・日時に読める文字列、先頭・末尾の空白、YAML の指示子で始まるか `: ` や ` #` を含む文字列、flow 内では `,` や括弧を含む文字列など）は double quote する。上記のどちらでも無損失に表せない string（CR や制御文字、2 個以上の末尾改行、空白で始まる最初の非空行、flow context の改行など）は escape 付きの double quote で出す。保存値は共通コアが受け入れる任意の文字列であり、`axon export` と parse の往復で一文字も変えない。fingerprint は常に double quote する。
6. 参照 mapping は `{ id: demo-8bxw2r }` のように一行の flow style で出す。すべての field で null は `null`、空の list は `[]` と綴る。参照先の record が `key` を持てば `{ key: ... }`、持たなければ `{ id: ... }` で出す。
7. LF 改行、2 space indent、document marker なし、末尾 newline 一つとする。
8. comment は意味に含めず、canonical rewrite では保持しない。

parser は strict とし、unknown field、重複 key、anchor、alias、merge key、独自 tag、複数 document、mapping 以外の root、要求型と異なる scalar を拒否する。schema label が `axon-declaration/v2` 以外の file は変換せずに拒否する。Entity の `label` field を持たない `axon-declaration/v1` の file もこれに当たり、v1 の `base` は v2 の fingerprint と一致しないため、診断は、既存 Entity の record を `axon export` で v2 として取り直すことと、保存先にまだない record は各 record に `label` を加えて `axon-declaration/v2` と宣言することを案内する。v1 の file の既存 Entity の record には `label` を足さず `base` も手で書き換えず、未使用の別 file へ `axon export` で取り直して未適用の編集を移す。v1 の file で `base` が null の record も、`prepare` で得た `id` が保存先に存在すれば以前の `apply` で保存済みなので、取り直す側に入れる。保存先にまだない record は、`base` が null で、`id` が null か、`axon show ID --skip-conditions` がその `id` を存在しないと診断する record である。保存先にまだない record だけの v1 の file は、各 record に `label` を加えて schema 行を v2 に書き換えてよい。取り直す record も含む v1 の file は、取り直した v2 の file へ保存先にまだない record を `label` を加えて移す。`axon export` が出す既存 Entity の record は `key` が null なので、移した record や編集がそれらを `{ key: ... }` で参照していれば `{ id: ... }` に書き換える。

## 競合検知

fingerprint は `blake3:` に続く lowercase 64 桁の hex string とする。各 Entity について、次の token 列を順に encode して BLAKE3 へ渡す。

1. schema label `axon-declaration/v2`
2. kind（`issue` / `group`）
3. ID
4. lifecycle の綴り
5. title
6. description
7. label の綴り
8. parent の presence（`none` / `some`）。`some` なら続けて解決済み parent ID
9. outgoing dependency の件数を符号なし 64 bit big-endian integer で表した 8 byte
10. prerequisite の解決済み ID を bytewise UTF-8 昇順に並べた各 token

一つの token は、UTF-8 byte 長を符号なし 64 bit big-endian integer で表した 8 byte と token 自体の UTF-8 byte を連結して encode する。件数だけは固定長整数として直接 encode する。`base` は`axon export`の時点で計算し、`check`・`apply` は保存先の現在値から再計算して一致を要求する。不一致は、後述の再試行の適用済み判定に該当しない限り競合として file 全体を拒否し、対象 ID を列挙する。

fingerprint には declaration が見せる項目だけを含める。記録 ID ではなく値から計算するため、`axon export` の後に変更され元の値へ戻った場合は記録が増えていても競合にしない。lifecycle は読み取り専用でも含める。`Completed` になっていれば編集自体が不可能で、それを競合として先に知らせる。再浮上条件、Note、記録者、作成日時、履歴、incoming edge、`references` の値は含めない。編集者が見ていない項目の変更は、その編集の意図と無関係である。編集者の意図はその値を見て作られており、現在値が同じなら適用して安全である。

## `axon export`

`axon export ID...` は一貫した記録の集合から selector の和集合を編集集合として選び、canonical YAML を stdout へ出す。編集集合の各 Entity は現在値と `base` を持ち、`key` は null になる。取得した直後は `key` を復元できないため、参照はすべて ID 参照になる。`references` は編集集合外の参照先から計算する。参照先が保存先に存在しない（親の不在・依存先の不在の違反）場合はその参照を `references` に載せずに出し、`prepare`・`check`・`apply` はその不在を診断して拒否する。選んだ Group の子が衝突中なら、その子を含めずに出すのではなく拒否する。保存先を変更せず、再浮上条件を実行しない。

新規計画の雛形は`axon docs declaration --example`で取得する。新規 Group 一件、その子 Issue 二件、子 Issue 間の dependency 一本を含む完全な YAML だけを stdout に出し、`id` と `base` は null、`lifecycle` は `not-started` とする。`axon docs declaration`は field、新規と既存の違い、`prepare` → `check` → `apply` → 再 `check` の手順を説明する。両者は保存先を開かず、管理 root、ネットワーク、ソース checkout に依存しない。引数なしの`axon docs`は状態モデルと基本の workflow を説明し、declaration の説明へ案内する。

## prepare、check、apply

取得と適用は三つの段階に分ける。ID を `apply` 前に確定させることが、再試行で重複作成しない唯一の単純な方法である。

`axon import prepare FILE` は保存先を変更せず、file だけを置き換える。strict YAML と局所規則（field、key、参照の形、重複、自己依存、親が Group であること）を検証し、既存 Entity の record の ID が保存先に存在し kind が一致すること、編集集合外の参照先が保存先に存在することを確認し、`id: null` の各 Entity に保存先と file の両方で衝突しない最終 ID を割り当てる。いずれかに失敗すれば file を変更しない。`base: null` の record の割り当て済み ID が保存先に存在する場合は後述の適用済み判定を行い、該当すればその ID を保持し、該当しなければ `conflict: ID: new id already exists` として file と保存先を変更せず拒否する。適用済みの可能性があるため、別 file へ `axon export ID` で取り直して編集を移す。意図して新規作成する場合は、その新規 record に `id: null` を明示する。保存先で未使用の割り当て済み ID は保持する。Entity の記録がなく Note だけが残る ID は予約済みとして再割り当てする。key 参照は保持する。`key` はそのまま残し、`base` は null のままにする。`references` を現在値から再生成し、canonical 形式で元の場所へ atomic に置き換える。置き換えは記録 file の公開と同じ手順に従い、temporary への書込と sync の後、file の bytes が読み取り時と同じであることを再照合してから rename し、directory を sync する。再照合前の失敗と bytes の変化は file を変更せず、rename 後の sync 失敗は file の結果不明として報告する。再照合後の非協調な editor 書込は、記録 file の保存契約と同じく対象外とする。Note だけが残る予約済み ID を除き、すでに全 ID が割り当て済みなら新しい ID を発行せず、同じ入力への `prepare` は同じ内容を返す。既存 Entity の `base` の競合判定は `prepare` の対象ではない。成功時は新規 record の `key -> 完全ID` の対応を一行ずつ表示する。

`prepare`・`check`・`apply` の `new id already exists` 診断は、対象 ID と、`axon export` での取り直し・編集の移行、および意図した新規作成には `id: null` を使う対処を示す。

`axon import check FILE` は file と保存先を変更しない。全 Entity に ID があり、file が canonical 形式であることを要求し、違えば `prepare` が必要であることを報告して暗黙に書き換えない。検証は次の順に行う。

1. strict schema、identity、参照の局所検証と、既存 Entity の ID が保存先に存在し kind が一致すること。`references` に記載された外部 Entity の存在と kind の一致も、再試行の適用済み判定より前に検証する
2. `base` と保存先の現在値の照合、および新規 Entity の割り当て済み ID が保存先に存在しないことの確認。不一致または存在があれば後述の再試行の適用済み判定を行い、該当すれば以降の検証を省いて適用済みとして扱い、該当しなければ競合とする。`base` が一致するのに file の `lifecycle` が現在値と異なれば、読み取り専用項目の書き換えとして拒否する
3. 参照先の存在
4. 検証に使う記録の集合へ編集を共通コアの通常操作として仮に適用し、包含・dependency・終了構成・固定された文面と label の制約を通常操作と同じ意味で検査
5. 作成、title の変更前後（改行を `\n`、制御文字を可視 escape とする一覧と同じ一行表示）、description の変更有無、label の前後、parent の前後、`needs` の増減を Entity ごとに表示。label だけが異なる場合も差分として表示する。差分がなければその旨を表示
6. 適用後の状況欄を保存情報から導出できる範囲で表示

description の全文差分は file 自体の git diff に任せ、CLI では変更の有無に留める。再浮上条件は実行しない。一件でも error があれば適用可能とは表示しない。

`axon import apply FILE` は保存先の書き込み lock を取得したあとに file の bytes を読んで digest を保持し、その内容と新しい記録の集合に対して `check` と同じ検証を同じ優先順位で再実行し、差分を共通コアの通常操作の列に変換して、一回の lock の下で記録として反映する。一件でも拒否されれば全件適用せず、保存先を変えない。保存する記録は変更のある Entity ごとに一つで、新規 Entity は最終値を持つ登録の記録、既存 Entity は title・description・label・parent・needs の最終値をまとめて持つ一つの記録（`axon log` では declaration の適用として示す）とし、検証に使った通常操作の列を個々の記録にはしない。記録は 1 件 1 file なので、反映は検証済みの全記録の一時 file を書いてから順に公開する形になり、公開の途中で rename が失敗するか process が失われた場合だけ一部の Entity の記録が公開された状態になりうる。公開は新規 Entity の登録を親と依存先が先になる順に、次いで既存 Entity の変更の順に行う。Entity ごとに記録が一つなので、各 Entity は最終値か元の値のどちらかにある。この場合は結果不明として扱い（rename の失敗なら診断に示し、process の喪失なら利用者が結果不明として扱う）、[再試行](#再試行) で残りを反映する。操作の適用順は実装が決め、有効な最終状態を中間状態の循環や前提不足で弾かないようにする。たとえば親の解除と依存の削除を追加より先に行う。新規 Entity は `lifecycle` が示す初期状態と `label` の値で作成し、架空の採用履歴を作らない。再浮上条件は既存 Entity では現在値を保持し、新規 Entity では未設定とする。同じ値の再指定は差分ではなく成功した no-op とする。

保存成功後に同じ file を canonical rewrite する。成功出力には、base の更新前に新規（`base: null`）だった record の `key -> 完全ID` の対応を一行ずつ含める。この `apply` が保存した記録の集合から、新規 Entity と既存 Entity の `base` と `lifecycle` を置き換え、`references` を再生成し、record と `needs` の並びを含む canonical 形を file 全体に再適用する。`key` と key 参照は残す。lock 解放後に保存先を読み直して他者の変更を `base` に取り込まない。rewrite は `prepare` と同じ手順で元の場所へ置き換える。rename の直前に file の bytes が読み取り時の digest と一致することを再照合し、変わっていれば file を変更せず、保存先には適用済みで declaration は更新していないことを報告する。rename 後の sync 失敗は declaration の結果不明として報告する。保存先の失敗は保存契約に従い、記録 file の rename 後の同期失敗は結果不明として扱う。保存成功後に declaration の更新だけが失敗または結果不明になった場合は、保存先が適用済みであることを明示する。

## 再試行

結果不明、または保存成功後の file 更新失敗のあとは、同じ file を再度 `apply` できる。`check` と `apply` は、`base` の不一致や新規 Entity の割り当て済み ID の存在を競合と判定する前に、Entity ごとに適用済み判定を行う。編集集合の各 Entity について、保存先の現在値が declaration の最終値（title、description、label、parent、outgoing dependency、kind、lifecycle。新規 Entity は割り当て済み ID で存在し宣言した初期 lifecycle であること）に完全一致すればその Entity は適用済みとして扱い、`base` に一致すれば（新規 Entity ならその ID が存在しなければ）未適用として残りの差分を適用し、どちらでもなければ競合として file 全体を拒否する。全 Entity が適用済みなら保存先を no-op とし、file の rewrite だけを完了する。公開の途中で process が失われた場合に一部の記録だけが公開されていても、この判定で残りを反映できる。途中の状態が違反を含むことがある（片方だけ反映された dependency の入れ替えなど）ため、この再試行に限り、`check` と `apply` は違反のある保存先の拒否を残りの反映で消える違反には適用せず、最終値の候補が違反を持たないことを同じ検査で確認して反映する。全 Entity が適用済みの再試行は保存先に記録を足さないため、他の writer が後から作った違反があっても拒否せず、file の rewrite を完了する。`prepare` で ID を確定しているため、再実行で同じ Entity が二度作られることはない。

## 拒否する入力と失敗の区別

次はいずれも file 全体を拒否し、部分適用しない。診断は原因の分類、対象 ID、固定されている項目名を示す。

- strict YAML 違反、schema label の不一致、unknown field、重複 key、anchor・alias・merge key・tag、`label` の欠落・null・固定集合の外の値
- ASCII 小文字英数字とハイフン以外を含む `id`、一つの参照 mapping での id と key の併記、`id` が null で `base` が non-null の record、`base` が null の record の `key` 欠落、未解決の ID・key、同じ Entity の二重宣言、解決後の重複した `needs`、自己依存、Issue を親にする参照、新規 Entity の `lifecycle` が `undecided`・`not-started` 以外
- 既存 Entity の `lifecycle` の書き換え、既存 Entity の record を `issues` と `groups` の間で移す kind の変更
- `base` の不一致、または新規 Entity の割り当て済み ID が保存先に存在すること（再試行の Entity ごとの適用済み判定に該当する場合を除く）。保存先の変更と入力側の `base` の改変・null 化は区別せず、いずれも競合として扱う
- `Completed`・`Cancelled` の title・description・label の差分、`Completed` の `needs` の差分、終了した Group の構成を変える所属変更、終了した Group を親にする作成・所属変更、実効値が `InProgress` か `Completed` の Entity を、移動先の Group 自身かその祖先に採用済みでないものがある場合に、その下へ移す変更、包含の循環、dependency の循環など、共通コアが通常操作でも拒否する変更。新規作成の親は終了していない Group であればよく、採用済みや着手済みである必要はない

終了した Entity を別の読み取り専用セクションに分けない。共通コアは`Cancelled`の dependency 編集を許し、終了していない Group の間での終了 Entity の所属変更を許す（`Completed` の Entity の移動先は、所属なしか、自身と全祖先が採用済みの Group に限る）ため、終了 Entity にも編集できる項目が残る。同じ list に置き、項目単位で拒否する。

保存境界の失敗は [CLIと表示の契約](cli.md#mutationの結果) と同じく Applied、Not applied、Result unknown を区別する。参照先の存在しない ID は保存先の不存在として、file 内で未解決の key とは別の診断にする。

## CLI と skill の責務

CLI は形式の検証、競合検知、共通コアによる制約検査、原子的な保存、再試行の判定を担う。skill は何を変えるかの判断、declaration の編集、check 結果の読み取り、apply の実行判断を担う。skill は保存 file を直接編集せず、一括操作を逐次の通常 CLI で代用しない。file 作成・レビューだけの依頼を保存済み Entity の変更へ広げず、`apply` の直前で同じ権限を再確認しない。個人 workflow の判断境界は[設計判断](../design/decisions.md#一括-declaration-の設計判断)に置く。
