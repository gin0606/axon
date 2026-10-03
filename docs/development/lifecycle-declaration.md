# Declaration と canonicalな`axon export`

`crates/axon-core/src/declaration.rs` は [計画全体の取得と一括編集](../reference/declaration.md) の declaration を扱う純粋な module です。共通コアが記録の集合から導出した現在値を入力として、Issue 単体・Group 全子孫・複数 selector の和集合と外部参照を計算します。SQL、filesystem、外部コマンドは呼びません。保存先の探索、記録の集合の読取り、stdout への出力は CLI と adapter の責務です。

`parse` は strict YAML と file-local の型・field・key・参照構造・重複を検査します。granit-parser の token 検査で anchor・alias・tag を拒否し、serde-saphyr で重複 key・merge key・unknown field・型の不一致を拒否します。nullable field も省略できず、null を空 list に変換しません。`label` は共通コアの `Label` の綴りの表で照合し、欠落・null・集合外の値を schema の拒否とします。未知 schema は変換しません。label を持たない `axon-declaration/v1` は、`base` も v2 の fingerprint と一致しないため、既存 record の `axon export` での取り直しと、保存先にまだない record への `label` の追加と v2 の宣言を案内して拒否します。保存先での ID 解決、存在・kind・競合・関係の制約検証は、操作の開始時に読んだ記録の集合と照合する操作側が担います。

`Declaration::serialize` は既存 record の作成日時を、現在値を導出したのと同じ記録の集合から読み、canonical 順序と scalar 表記で出力します。`fingerprint` は label を含む見える値だけを契約の token encoding で BLAKE3 に渡します。再浮上条件・Note・履歴は declaration に取り込みません。`example` は保存先に依存しない新規 Group と子 Issue 二件の雛形で、各 record は `label: feat` を持ちます。

declaration のために Quint の状態や action を追加しません。`axon import apply` は共通コアの通常操作の列であり、lifecycle・包含・dependency の意味を変えないためです。契約は Rust の独立 fixture で検証し、少なくとも次を対象にします。

- canonical example の parse と同値な serialize、strict parser の各拒否（label の欠落・集合外、v1 の schema を含む）、CR・制御文字・YAML の型に読める文字列を含む文面の完全な往復
- `axon export` の selector（Group の全子孫、Issue 単体、和集合、重複除去）と `references` の計算
- `prepare` の ID 割り当て、`key` 保持、`base` null 保持、再実行の同一性
- `check` の検証順序、競合の列挙、label の前後を含む差分の表示、file と保存先の不変
- label だけの変更が fingerprint を変え、`apply` で一件の declaration の適用の記録になること、新規 Entity が宣言した label で作られること
- `apply` の原子性（共通コアの拒否で保存先が変わらないこと）、`base` と `references` の rewrite、`key` の保持
- 終了 Entity の固定項目、終了 Group の構成、循環、Issue 親、自己依存の拒否
- 結果不明・file 更新失敗後の再 `apply` が最終値一致で no-op になること、部分一致で競合になること
- 再浮上条件の保持と未実行、Note の不変
- `axon docs declaration` と `--example` が保存先を開かないこと

単体テストは [契約文書](../reference/declaration.md) の canonical example、文字列の完全な往復、拒否入力、並び順、fingerprint の境界を検査します。`tests/lifecycle/declaration.rs` は独立 fixture で subtree・Issue・和集合、外部参照、保存先非変更と条件未実行を確認し、壊れた管理 root でも `axon docs` と雛形が取得できることを検査します。

`crates/axon-core/src/declaration/import.rs` は保存された記録の集合から導出した現在値とのidentity・base照合、適用済み判定、外部参照の再生成、共通コアの通常操作による候補の構築を共有する。適用済み判定はEntityごとで、違反のある保存先を許す「途中で止まった反映の再試行」は、編集集合のうち変更を伴うEntity（新規、または`base`と最終値が異なるもの）がすでに最終値を持つことで判定し、全Entityが適用済みなら候補を作らず通す。`prepare`は`base: null`のrecordの割り当て済みIDのEntityが最終値を持てばIDを保持し、持たなければ`check`・`apply`と同じ`new id already exists`の競合として拒否する。診断は対象IDと、別fileへの`axon export`での取り直し・編集の移行、意図した新規作成には`id: null`を使う対処を示す。未使用の割り当て済みIDは保持し、`id: null`とEntityの記録がなくNoteだけが残る予約済みIDには新しいIDを割り当てる。親解除・dependency削除、新規作成（宣言したlabelで作成）、文面変更、label変更、親設定、dependency追加の順で差分を検査する。`Checked` の候補は保存前のものであり、保存先を変更しない。`src/declaration_file.rs` は別のI/O境界として記録fileのpublish手順を再利用し、temporary書込・sync、入力bytes再照合、rename、directory syncを行う。

`src/declaration_file/relationship_tests.rs` の関係変更行列は、`Cancelled` Groupへの所属拒否、`Cancelled` Entityのdependency差替え、新規Groupへの既存Entityの移動、親子Groupの反転、着手中のIssueを持つGroupの採用済みGroup間の移動を検査する。recordの正順・逆順・巡回順で共通コアの適用結果を比較し、`axon import prepare`でcanonical化して適用した結果との一致、拒否時の保存先と入力の保持を確認する。

`src/declaration_file.rs` のprocess fixtureはlib test binaryを子processとして起動し、本番と共通の適用経路を実行する。記録fileのtemporary作成後rename前、保存後・declaration書戻し前、書戻し後でbarrierに到達した子を強制終了する。再openした保存先の記録の集合と入力bytes、同じfileの再度`axon import apply`による通常適用またはno-opへの収束を検査する。停止点はprivateな適用経路からcrate内の保存adapterへ渡し、公開APIと通常操作の意味は変えない。
