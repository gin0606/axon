# Declaration と canonical export

`src/declaration.rs` は [正本spec](../../spec/lifecycle_proposal.md#計画全体の取得と一括編集) の declaration を扱う純粋な module です。共通コアの immutable な `Snapshot` を入力として、Issue 単体・Group 全子孫・複数 selector の和集合と外部参照を計算します。SQL、filesystem、外部コマンドは呼びません。保存先の探索、一貫した snapshot の読取り、stdout への出力は CLI と adapter の責務です。

`parse` は strict YAML と file-local の型・field・key・参照構造・重複を検査します。granit-parser の token 検査で anchor・alias・tag を拒否し、serde-saphyr で重複 key・merge key・unknown field・型の不一致を拒否します。nullable field も省略できず、null を空 list に変換しません。未知 schema と旧モデルは変換しません。保存先での ID 解決、存在・kind・競合・関係の制約検証は、snapshot と照合する操作側が担います。

`Declaration::serialize` は既存 record の作成日時を同じ snapshot から読み、canonical 順序と scalar 表記で出力します。`fingerprint` は見える値だけを spec の token encoding で BLAKE3 に渡します。再浮上条件・Note・履歴は declaration に取り込みません。`example` は保存先に依存しない新規 Group と子 Issue 二件の雛形です。

単体テストは spec の canonical example、文字列の完全な往復、拒否入力、並び順、fingerprint の境界を検査します。`tests/lifecycle/declaration.rs` は両 backend の独立 fixture で subtree・Issue・和集合、外部参照、保存先非変更と条件未実行を確認し、壊れた管理 root でも docs と雛形が取得できることを検査します。

`src/declaration/import.rs` は保存snapshotとのidentity・base照合、適用済み判定、外部参照の再生成、共通コアの通常操作による仮snapshot構築を共有する。親解除・dependency削除、新規作成、文面変更、親設定、dependency追加の順で差分を検査する。`Checked` のsnapshotは保存前の候補であり、SQLやfileを変更しない。`src/declaration_file.rs` は別のI/O境界としてfile backendのpublish手順を再利用し、temporary書込・sync、入力bytes再照合、rename、directory syncを行う。

`src/declaration/tests.rs` の関係変更行列は、両backendでCancelled Groupへの所属拒否、Cancelled Entityのdependency差替え、新規Groupへの既存Entityの移動、親子Groupの反転、InProgress子孫を持つGroupのInProgress Group間移動を検査する。recordの正順・逆順・巡回順で共通コアの適用結果を比較し、prepareでcanonical化して各backendへ適用した結果との一致、拒否時の保存先と入力の保持を確認する。

`src/declaration_file.rs` のprocess fixtureはlib test binaryを子processとして起動し、本番と共通のapply経路を実行する。SQLiteのUPDATE後commit前、fileのtemporary作成後rename前、両backendの保存後・declaration書戻し前、書戻し後でbarrierに到達した子を強制終了する。再openした保存先の完全snapshotと入力bytes、同じfileの再applyによる通常適用またはno-opへの収束を検査する。停止点はprivateなapply経路からcrate内の保存adapterへ渡し、公開APIと通常操作の意味は変えない。
