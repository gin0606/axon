# 宣言ファイルの契約

この文書は、`axon export` と `axon import prepare/check/apply` が扱う plan 宣言ファイルの形式契約を定める。状態モデルの意味は [状態モデル](state-model.md) に従い、この形式から新しい状態、関係、永続的な plan entity は導入しない。

宣言ファイルは DB の全量 dump ではない。`issues` と `groups` に列挙した Entity だけを編集対象にし、それ以外の Entity は変更しない。編集対象については title、description、親、outgoing dependency を完全に宣言する。ファイルから Entity 自体を消しても、その Entity の削除、不採用化、所属解除を意味しない。

## 1. Canonical YAML

次は既存と新規の issue / group、内部と境界をまたぐ関係を含む、`prepare` 前の完全な例である。fingerprint は例示値であり、そのまま適用できない。実在 snapshot の取得手順は §6.1.1 を参照する。新規 DB で使える完全 YAML は `axon docs declaration --example` で取得する。

```yaml
schema: axon-plan/v3
issues:
  - id: demo-4d5e6f
    key: null
    base: "blake3:1111111111111111111111111111111111111111111111111111111111111111"
    title: 既存 CLI を共通 Entity API へ移す
    description: |-
      issue と group に共通する操作を先に揃える。
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: at_date
        at: "2026-10-01T00:00:00Z"
  - id: null
    key: api
    base: null
    title: import API を実装する
    description: |-
      prepare、check、apply を一つの検証経路で実装する。
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
  - id: null
    key: storage
    base: null
    title: 宣言の原子的な保存を実装する
    description: null
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
groups:
  - id: demo-1a2b3c
    key: null
    base: "blake3:2222222222222222222222222222222222222222222222222222222222222222"
    title: 宣言ファイル
    description: |-
      plan を一枚の編集面で扱えるようにする。
    observed:
      progress: in_progress
      claim:
        actor: codex
        worktree: /work/axon-declaration
        at: "2026-09-03T09:00:00Z"
      disposition: accepted
      resurface:
        kind: always
  - id: null
    key: import
    base: null
    title: import
    description: null
    observed:
      progress: not_started
      claim: null
      disposition: accepted
      resurface:
        kind: always
relations:
  editable:
    parents:
      - child: { id: demo-4d5e6f }
        parent: { id: demo-1a2b3c }
      - child: { key: api }
        parent: { key: import }
      - child: { key: import }
        parent: { id: demo-1a2b3c }
      - child: { key: storage }
        parent: { key: import }
    dependencies:
      - dependent: { id: demo-4d5e6f }
        prerequisite: { id: demo-7g8h9j }
      - dependent: { key: api }
        prerequisite: { key: storage }
      - dependent: { key: import }
        prerequisite: { id: demo-ab12cd }
  readonly:
    parents:
      - child: { id: demo-ef34gh }
        parent: { id: demo-1a2b3c }
    dependencies:
      - dependent: { id: demo-jk56mn }
        prerequisite: { key: api }
references:
  entities:
    - id: demo-7g8h9j
      kind: issue
      base: "blake3:3333333333333333333333333333333333333333333333333333333333333333"
      title: serde-saphyr の strict mode を確認する
      observed:
        progress: ended
        claim: null
        disposition: accepted
        resurface:
          kind: always
    - id: demo-ab12cd
      kind: group
      base: "blake3:4444444444444444444444444444444444444444444444444444444444444444"
      title: Entity model の実装
      observed:
        progress: in_progress
        claim:
          actor: human
          worktree: /work/entity-model
          at: "2026-09-02T03:30:00Z"
        disposition: accepted
        resurface:
          kind: after_entity
          entity: { id: demo-7g8h9j }
    - id: demo-ef34gh
      kind: issue
      base: "blake3:5555555555555555555555555555555555555555555555555555555555555555"
      title: 宣言ファイルの利用手順を書く
      observed:
        progress: not_started
        claim: null
        disposition: accepted
        resurface:
          kind: always
    - id: demo-jk56mn
      kind: group
      base: "blake3:6666666666666666666666666666666666666666666666666666666666666666"
      title: リリース準備
      observed:
        progress: not_started
        claim: null
        disposition: accepted
        resurface:
          kind: always
```

`issues` と `groups` は同じ Entity envelope を使う。kind は格納された list から一意なので編集対象内には重ねて書かず、外部 Entity の `references.entities` だけが `kind` を持つ。group も issue と同じ公開 ID namespace、title、description、状態を持ち、slug や name は持たない。

## 2. Top-level と field の契約

root は次の 5 field だけをこの順で持つ mapping とする。すべて必須であり、空の場合も list または mapping 自体を省略しない。

旧 `axon-plan/v2` はメモリ内変換せず拒否する。対応する現行 store を通常起動で移行し、`axon export` を再実行して新しい v3 declaration を作る。

| field | 型 | 所有者・意味 |
| --- | --- | --- |
| `schema` | string | 必ず `axon-plan/v3`。形式と fingerprint の version label を兼ねる |
| `issues` | list | 編集対象の issue の完全宣言 |
| `groups` | list | 編集対象の group の完全宣言 |
| `relations` | mapping | 編集可能・読み取り専用の関係を一箇所に正規化したもの |
| `references` | mapping | 境界の外にある読み取り専用 Entity の snapshot |

### 2.1 編集対象 Entity

`issues[]` と `groups[]` の要素は次の field をこの順で必ず持つ。

| field | 型 | 編集可否 | 契約 |
| --- | --- | --- | --- |
| `id` | string または null | identity | 既存 Entity と prepare 済みの新規 Entity では公開 ID。prepare 前の新規 Entity だけ null |
| `key` | string または null | YAML 内のみ | file-local alias。DB へ保存せず、fresh export では null。新規 Entity は prepare 前に必須 |
| `base` | fingerprint または null | 読み取り専用 | 既存 Entity の export 時 snapshot。未適用の新規 Entityだけ null |
| `title` | string | 編集可 | 一行の非空文字列 |
| `description` | string または null | 編集可 | Markdown。null は本文なし |
| `observed` | mapping | 読み取り専用 | import が変更しない保存状態 |

`id: null` なら `key` は non-null でなければならない。`id` と `key` が両方 non-null でもよく、prepare/apply 後の同じファイルではこの形になる。key は既存 Entity に file-local alias を付ける用途にも使える。

key は `^[a-z][a-z0-9-]{0,63}$` に一致し、`issues` と `groups` を通じて file 全体で一意とする。ID も両 list を通じて一意で、同じ DB Entity を二度宣言してはならない。

title は前後の空白を prepare が除去し、空または改行を含む値を拒否する。description は YAML が返す文字列を Markdown として保存し、中身を解析しない。空文字または空白文字だけなら prepare が null に正規化する。non-null の内容は、YAML の改行正規化を除いて変更しない。canonical 出力では複数行 description を literal block で表し、保存値に末尾改行がなければ `|-` を使う。

既存 Entity の title、description、parent、outgoing dependency は、その Entity の
Disposition が `undecided` のときだけ実変更できる。`accepted` / `rejected` の Entity に
実差分があれば check / apply は file 全体を拒否する。同じ値や同じ関係の再指定は変更では
ないため成功 no-op とする。parent の owner は child、dependency の owner は dependent
(source) であり、反対側 Entity の Disposition はこの guard に使わない。

### 2.2 observed

`observed` は次の field をこの順で必ず持つ。

| field | 型 | 契約 |
| --- | --- | --- |
| `progress` | `not_started` / `in_progress` / `ended` | A Progress の保存値 |
| `claim` | mapping または null | progress が `in_progress` のときだけ non-null |
| `disposition` | `undecided` / `accepted` / `rejected` | B Disposition の保存値 |
| `resurface` | mapping | C Resurface condition の保存値 |

claim mapping は `actor`、`worktree`、`at` をこの順で必須とする。`at` は timezone を含む RFC 3339 string を入力し、canonical form では UTC の `Z` 表記へ正規化する。小数秒が 0 なら省略し、0 でなければ末尾の 0 を除いた必要最小限の桁を残す。progress と claim の組み合わせが矛盾する入力は拒否する。

resurface は `kind` を先頭に持ち、kind ごとに次の shape だけを許す。

```yaml
resurface:
  kind: always
```

```yaml
resurface:
  kind: at_date
  at: "2026-10-01T00:00:00Z"
```

```yaml
resurface:
  kind: after_entity
  entity: { id: demo-7g8h9j }
```

```yaml
resurface:
  kind: manual
```

```yaml
resurface:
  kind: command
  command: "exit 0"
```

`manual` は付随 field を持たない。`command` はシェル文字列であり、他 kind の付随 field と混在させない。
観測結果・実行時刻・外部出力は observed に含めない。Control state は read-only なので、
条件の変更は `when` で行う。export と参照照合だけでは外部コマンドを実行しない。
check / apply が導出差分を作る場合は必要な条件を評価し、変更前後で同じ Entity の結果を共有する。
評価失敗時は書き込みを確定せず、commit 後の出力や再 export で追加評価しない。
共通の実行契約は [CLI 契約](cli.md#外部条件の評価) を参照する。

`at` は秒と UTC offset を持つ RFC 3339 string とする。小数秒は 9 桁まで許可し、canonical form は instant を UTC の `Z` 表記に正規化しつつ入力の小数桁数を保持する。`after_entity` の参照先は issue / group のどちらでもよい。ready、blocked、orphaned、surfaced、active scope、blocking cause、group 集計などの導出値はファイルに保存しない。時刻、別 Entity、外部観測の変更だけで snapshot が古くなるのを避け、`check` が実行時の DB から適用前後の導出差分を表示する。

新規 Entity の observed は必ず `not_started`、claim null、`accepted`、`always` とする。import から状態操作は行わず、異なる値は拒否する。

### 2.3 Entity reference

すべての Entity reference は次のどちらか一方だけを持つ object とする。

```yaml
{ id: demo-4d5e6f }
```

```yaml
{ key: api }
```

同じ object への id と key の併記、どちらもない object、未知 field、未解決 ID / key はエラーである。key が付いた編集対象への canonical reference は key を使い、それ以外は ID を使う。解決後に同じ Entity を指す重複関係は、表記が id と key に分かれていてもエラーにする。

key は import 中だけ参照を読みやすくする別名である。prepare は DB 登録前に実 ID を割り当てるが key を残す。apply は key を実 ID に解決して DB に実 ID だけを保存する。apply 後の同じファイルにも key と key reference を残すが、fresh export は key を復元できないためすべて ID reference になる。

### 2.4 base fingerprint

fingerprint は `blake3:` に続く lowercase 64 桁の hex string とする。

各 Entity の fingerprint は、次の token 列を上から順に encode して BLAKE3 へ渡す。

1. schema `axon-plan/v3`
2. fingerprint 構造の version label `axon-entity-fingerprint/v3`
3. kind (`issue` / `group`)
4. ID
5. title
6. description の presence (`none` / `some`)。`some` なら続けて description
7. parent の presence (`none` / `some`)。`some` なら続けて解決済み parent ID
8. progress (`not_started` / `in_progress` / `ended`)
9. claim の presence (`none` / `some`)。`some` なら続けて actor、worktree、canonical UTC timestamp
10. disposition (`undecided` / `accepted` / `rejected`)
11. resurface kind (`always` / `at_date` / `after_entity` / `manual` / `command`)。`at_date` なら続けて canonical UTC instant、`after_entity` なら続けて解決済み Entity ID、`command` なら続けてシェル文字列。評価結果は含めない
12. outgoing dependency の件数を符号なし 64 bit big-endian integer で表した 8 byte
13. prerequisite の解決済み ID を bytewise UTF-8 昇順に並べた各 token

一つの token は、UTF-8 byte 長を符号なし 64 bit big-endian integer で表した 8 byte と、token 自体の UTF-8 byte を連結して encode する。件数だけは上記 12 の固定長整数として直接 encode する。最後に得た 32 byte の digest を lowercase hex にして `blake3:` を付ける。文字列の比較と sort は locale を使わず UTF-8 byte 列で行う。

key、YAML の空白と表記、list の表示順、created/updated time、履歴、導出値、incoming relation は含めない。group と issue に同じ規則を使う。`base` は export した snapshot の競合検知値であって、編集後の YAML 内容から利用者が再計算する値ではない。

## 3. Relation の所有権と配置

すべての包含と dependency を `relations` の一箇所へ置き、Entity record 内には埋め込まない。一つの関係は file 内に一度だけ現れる。

```yaml
relations:
  editable:
    parents: []
    dependencies: []
  readonly:
    parents: []
    dependencies: []
```

4 list は空でも必須である。

### 3.1 Parent

parent relation は child が所有する。

```yaml
- child: { key: api }
  parent: { key: import }
```

- child は issue / group のどちらでもよい
- parent は group だけを許す
- 一つの child に最大一つ
- `editable.parents` は、`issues` または `groups` にある child の親を完全宣言する
- 編集対象の child が `editable.parents` に現れなければ、その child の parent は null である
- `readonly.parents` は、編集対象 group を親に持つが child が編集対象外である incoming containment を表す
- 編集対象外 child の親を変更、追加、削除してはならない

編集対象 child が外部 group を親に持つ relation は editable であり、その外部 group を `references.entities` に載せる。逆に、外部 child が編集対象 group を親に持つ relation は readonly である。配置は境界の内外ではなく owner である child が編集対象かどうかで決まる。

### 3.2 Dependency

dependency は dependent が所有する。

```yaml
- dependent: { key: api }
  prerequisite: { key: storage }
```

- dependent / prerequisite は issue / group の全組み合わせを許す
- `editable.dependencies` は、編集対象 dependent の outgoing dependency を完全宣言する
- 編集対象 Entity が dependent として一度も現れなければ、その outgoing dependency 集合は空である
- `readonly.dependencies` は、編集対象 Entity を prerequisite にするが dependent が編集対象外である incoming dependency を表す
- 編集対象外 dependent の relation を変更、追加、削除してはならない

編集対象 dependent から外部 prerequisite への relation は editable で、外部 prerequisite を `references.entities` に載せる。外部 dependent から編集対象 prerequisite への relation は readonly で、外部 dependent を `references.entities` に載せる。

親や dependency を空にするための null 要素や空 object は使わない。完全宣言の list に owner の relation が存在しないこと自体が「親なし」「outgoing dependency なし」を意味する。Entity を file から消すこととは意味が異なる。

### 3.3 references

`references` は次の形だけを持つ。

```yaml
references:
  entities: []
```

`references.entities` には relations または observed の `after_entity` から参照される編集対象外 Entity と、readonly relation の外部 owner を一度ずつ載せる。要素は `id`、`kind`、`base`、`title`、`observed` をこの順で必須とし、description、key、relation は持たない。

各 snapshot の値は読み取り専用であり、現在 DB と一致する必要がある。編集対象の関係を変える際、必要になった実在 snapshot は追加し、不要になったものは除く。必要集合は relation と編集対象の observed AfterEntity から始まり、外部 snapshot の AfterEntity 先を再帰的に含む。過不足は拒否する。外部 Entity の表示と状態、境界をまたぐ incoming relation を一枚の file で確認できる一方、import の編集範囲を暗黙に広げない。

## 4. 必須性、null、空、要素の不在

| 位置 | null / 空 / 不在の意味 |
| --- | --- |
| top-level field | 不在は常にエラー。list が空なら `[]` を書く |
| Entity `id` | null は prepare 前の新規 Entity。check/apply では禁止 |
| Entity `key` | null は alias なし。`id: null` と同時にはできない |
| Entity `base` | null は未適用の新規 Entity。既存・reference では禁止 |
| `description` | null は本文なし。空・空白だけの string は prepare が null にする |
| `claim` | null は claim なし。`in_progress` と組み合わせるとエラー |
| relation list | 空 list は、その区分の relation が 0 件という完全な宣言 |
| editable parent edge | child の edge 不在は parent null |
| editable dependency edge | dependent の edge 不在は outgoing set が空 |
| `issues` / `groups` の要素不在 | DB Entity を変更対象にしない。削除や関係解除を意味しない |

null を許可していない field へ null を書くことと、必須 field を省略することはエラーである。空 mapping を null や関係なしの代用には使わない。

## 5. Canonical order と表記

canonical serializer は次の規則を使う。

1. mapping key はこの文書の例と field 表に示した順序で出す。alphabetical sort にはしない。
2. `issues` と `groups` は ID がある要素を ID 昇順、その後に `id: null` の要素を key 昇順で並べる。prepare 後は全要素を割り当て済み ID 昇順に並べ直す。
3. `parents` は解決済み child ID、parent ID の順、`dependencies` は解決済み dependent ID、prerequisite ID の順に並べる。prepare 前の未割り当て Entity は ID の代わりに key を使う。
4. `references.entities` は ID 昇順に並べる。
5. duplicate Entity、key、解決後 relation は許可せず、sort で潰さない。
6. enum と field 名は lowercase snake_case、key は lowercase kebab-case とする。
7. YAML string は型が曖昧にならない形で出す。特に日付、timestamp、fingerprint は quote する。複数行 description は literal block とする。
8. LF 改行、2 space indent、document marker (`---` / `...`) なし、末尾 newline 一つとする。
9. comment は意味に含めず、prepare/apply の canonical rewrite では保持しない。

canonical reference は key があれば key、なければ ID を使うため、同じ意味の file に複数の canonical 表現はない。

## 6. export / prepare / check / apply

### 6.1 export

export は一貫した DB snapshot から selector の和集合を編集対象として選ぶ。関係を理由に編集対象を増やさない。

- 明示 ID はその Entity だけ
- `--group` は指定 group、直下 issue、直下 child group
- `--recursive` を付けた group selector は全子孫
- selector の重複は除く

編集対象は現在値と base を持ち、key は null になる。編集対象 owner の parent / outgoing dependency は `relations.editable` へ、境界の外から入る parent / dependency は `relations.readonly` へ出す。必要な外部 endpoint は `references.entities` へ出す。

### 6.1.1 新しい外部 dependency・親の snapshot を用意する

編集対象にまだ関係のない既存 Entity を参照するときも、対象を `issues` / `groups` に増やす必要はない。以下は実在 ID と DB の取得値が必要な手順であり、架空 fingerprint をそのまま適用する例ではない。

1. 新規 owner は `axon docs declaration --example > plan.yml` から始める。既存 owner は Undecided であることを確認し、意図した編集集合だけを `axon export <OWNER-ID> > plan.yml` へ取得する。固定 owner の変更には別途 Undecided 化が必要。
2. 別ファイルに `axon export <PREREQUISITE-ID> > prerequisite.yml` と `axon export <PARENT-GROUP-ID> > parent.yml` を実行し、各終了成功を確認する。これらは取得用であり、destination へ丸ごと連結しない。
3. 各 export の `issues` / `groups` から対象 record の `id`、`base`、`title`、完全な `observed` をコピーし、元 list に合わせて `kind: issue` / `kind: group` を加える。`key` と `description` はコピーしない。この record を `plan.yml` の `references.entities` へ一度だけ追加する。ID・base・observed を推測・捏造しない。
4. 新規例なら以下の edge を既存 `relations.editable` の各 list に追加する（他の edge は保持）。`<...>` は取得した完全 ID に置換する。既存 owner なら key の代わりに `{ id: <OWNER-ID> }` を使い、親は追加ではなく必要に応じて置換する。

```yaml
# 各 list に加える断片。完全な適用可能 YAML ではない。
parents:
  - child: { key: feature }
    parent: { id: <PARENT-GROUP-ID> }
dependencies:
  - dependent: { key: implement }
    prerequisite: { id: <PREREQUISITE-ID> }
```

5. 取り込む `observed.resurface` が AfterEntity なら、その参照先も編集集合にない限り snapshot を含め、再帰的に辿る。取得元 export の `references.entities` から必要 record をコピーできるが、取得元にある不要 record は取り込まない。
6. `relations.readonly` は destination の編集集合に入る、外部 owner の incoming relation を保持する。取得元 export の関係を丸ごとコピーしてはならない。外部 snapshot 自身の親・outgoing dependency は destination の readonly relation ではない。関係の削除等で不要になった snapshot は除き、AfterEntity を含む必要集合だけを残す。
7. destination に `prepare` → `check` →（差分を確認して）`apply` → `check` を各々実行する。最終 check は差分なしを要求する。参照先を再 export し、宣言と Control 値が不変であることを確認する。参照先への新しい incoming edge は再 export の readonly に現れ得る。

親は child、dependency は dependent が所有するため、外部の親・prerequisite が固定でも、その Entity の Undecided 化は不要である。snapshot を含めることは、外部 Entity の宣言・状態を変更する要求ではない。

内蔵 `axon docs declaration` にも同手順があり、import/export help から参照できる。参照不足は ID とファイル内 snapshot を、DB 不存在は active root と ID を確認する。base 不一致では元ファイルを保持して fresh export と比較し、競合を調整する。base が一致する snapshot 不一致は取得値を復元する。

### 6.2 prepare

prepare は DB を変更せず、file だけを atomic replace する。

1. strict YAML と全 field の型を検証する。
2. 一貫した DB snapshot で ID、key、base、observed、references、relation ownership を検証する。
3. `id: null` の各 Entity に、DB と file の両方で衝突しない最終 ID を割り当てる。
4. 新規 Entityの base は null、key と既定 observed はそのまま残す。
5. null / 空文字、reference、順序、scalar style を canonical form に正規化する。
6. 元 file と同じ場所へ安全に置き換える。

すでに ID が全件へ割り当て済みなら新しい ID を発行せず、同じ入力への prepare は同じ内容を返す。

canonical example の新規 Entity には、たとえば次の変化が起きる。

```diff
-  - id: null
+  - id: demo-pq78rs
     key: api
     base: null
```

relation は key reference のままなので、ID 発行後も人間とエージェントが役割名で読める。

### 6.3 check

check は file と DB を変更しない。すべての Entity に ID があり、file が canonical form であることを要求する。違う場合は prepare が必要であることを報告し、暗黙に書き換えない。

検証順序は次のとおり。

1. strict schema、identity、reference、relation の局所検証
2. base と現在の DB snapshot の競合検知
3. observed、references、readonly relations が現在値と一致することの検証
4. 編集後の仮 snapshot を組み立て、Entity ごとの declaration 固定と、axon 本体の包含、Ended group、dependency、待機 graph などの制約を検証
5. 作成・更新、title / description、parent、dependency の構造差分を表示
6. ready、blocked、orphaned、active scope、group 完了可能性など、適用前後で変わる導出値を表示

orphaned など axon で有効な結果は警告と差分として出すが、import 独自の禁止事項にはしない。一件でも error があれば成功扱いにせず、適用可能とは表示しない。

### 6.4 apply

apply は書き込み lock を取得したあと check と同じ検証を新しい一貫した snapshot で再実行し、全作成・更新・関係変更を Store の状態更新境界を通して一括反映する。一件でも検証に失敗すれば、宣言の変更は全件適用しない。

SQLite backend は一つの transaction で保存し、commit 前の失敗では rollback する。file backend は検証済み snapshot を atomic replace し、directory sync まで完了して保存成功とする。SQLite の commit 自体の失敗や file の置換後の同期失敗は結果不明として扱い、全件未適用とは断定しない。詳細は[保存契約](file-storage.md#保存の保証と失敗時の確認)と[CLI の診断の保存境界](cli.md#診断の保存境界)に従う。

保存成功後に同じ宣言 file を canonical rewrite する。

- 新規 Entity の base を現在の fingerprint に置き換える
- 既存 Entity と references の base / observed を保存後 snapshot に更新する
- key と key reference は残す
- relation と list を canonical order に並べる

fresh export では key を復元せず ID reference を使う。

保存成功後、宣言 file 更新だけに失敗した場合は、同じ宣言を再度 apply できる。base が古くても、Store の現在の所有値と読み取り専用値が宣言の最終値に完全一致すれば、直前の適用済み内容と判断して Store は no-op とし、宣言 file rewrite だけを完了する。一部だけ一致する場合や、その後に別の変更がある場合は競合として拒否する。

## 7. 拒否する入力

### 7.1 Strict YAML 違反

次は parse または schema error にする。

```yaml
# 未知 field
schema: axon-plan/v3
issues: []
groups: []
relations:
  editable: { parents: [], dependencies: [] }
  readonly: { parents: [], dependencies: [] }
references: { entities: [] }
priority: high
```

```yaml
# duplicate key
schema: axon-plan/v3
issues: []
issues: []
```

```yaml
# anchor / alias / merge key
defaults: &defaults
  observed: {}
issues:
  - <<: *defaults
```

独自 tag、複数 YAML document、mapping 以外の root、暗黙の型変換により要求型と異なる scalar も拒否する。schema に存在しない補助 field を comment 代わりに追加することはできない。

### 7.2 Identity / relation 違反

```yaml
# id と key を一つの reference に併記
dependent: { id: demo-4d5e6f, key: cli }
prerequisite: { key: storage }
```

未解決・重複 key、存在しない ID、issue を parent にする関係、同じ child の parent 二件、自己 dependency、解決後に同じになる duplicate dependency、待機 graph の cycle、Ended group の固定条件に反する変更は拒否する。

### 7.3 読み取り専用部分の変更

次のいずれも file 全体を拒否し、部分適用しない。

- 編集対象 Entity の `observed.disposition` を `accepted` から `rejected` に変える
- `in_progress` の claim actor、worktree、at を書き換える
- `references.entities` の title、observed、kind、base が現在 DB の snapshot と一致しない、または必要な参照集合に過不足がある
- `relations.readonly.parents` から外部 child の所属を消す
- `relations.readonly.dependencies` の endpoint を変える、追加・削除する
- 既存 Entity の base を null または別の fingerprint に変える
- export 後に DB が変わって base が stale になった file を適用する

reference の base が現在 DB と一致するのに kind/title/observed が違えば読み取り専用 snapshot の不一致、base 自体が違えば古いまたは不正確な fingerprint として区別する。後者だけでは DB の変更と入力側の改変を断定できない。ファイル内で未解決の ID は DB の不存在を意味せず、snapshot 付きで DB にない reference とは別診断にする。拒否時は DB へ部分適用せず、prepare の失敗では元ファイルを保持する。

## 8. 実装テストへ落とす fixture

少なくとも次を独立した fixture / test にする。

- canonical example の parse と同値な serialize
- 新規 issue / group の ID 割り当て、key 保持、base null 保持
- apply 後の base 更新と fresh export での key 消失
- Entity ごとの parent 不在と outgoing dependency 不在が null / 空集合になること
- issue→issue、issue→group、group→issue、group→group dependency
- editable owner から外部 endpoint への outgoing relation
- 外部 owner から編集対象 endpoint への readonly incoming relation
- observed の各 resurface shape と progress / claim 整合
- duplicate key、id/key 併記、解決後 duplicate relation の拒否
- unknown field、duplicate YAML key、anchor、alias、merge key、tag の拒否
- observed、references、readonly relation の変更拒否
- stale base の拒否と、commit 済み最終値に完全一致する場合だけの retry no-op
- relation を跨ぐ既存 axon 制約違反で transaction 全体が rollback されること
- check と apply が同じ validation result と構造・導出差分を使うこと
