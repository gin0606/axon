# Declaration と canonical export

`src/declaration.rs` は [正本spec](../../spec/lifecycle_proposal.md#計画全体の取得と一括編集) の declaration を扱う純粋な module です。共通コアの immutable な `Snapshot` を入力として、Issue 単体・Group 全子孫・複数 selector の和集合と外部参照を計算します。SQL、filesystem、外部コマンドは呼びません。保存先の探索、一貫した snapshot の読取り、stdout への出力は CLI と adapter の責務です。

`parse` は strict YAML と file-local の型・field・key・参照構造・重複を検査します。granit-parser の token 検査で anchor・alias・tag を拒否し、serde-saphyr で重複 key・merge key・unknown field・型の不一致を拒否します。nullable field も省略できず、null を空 list に変換しません。未知 schema と旧モデルは変換しません。保存先での ID 解決、存在・kind・競合・関係の制約検証は、snapshot と照合する操作側が担います。

`Declaration::serialize` は既存 record の作成日時を同じ snapshot から読み、canonical 順序と scalar 表記で出力します。`fingerprint` は見える値だけを spec の token encoding で BLAKE3 に渡します。再浮上条件・Note・履歴は declaration に取り込みません。`example` は保存先に依存しない新規 Group と子 Issue 二件の雛形です。

単体テストは spec の canonical example、文字列の完全な往復、拒否入力、並び順、fingerprint の境界を検査します。`tests/lifecycle/declaration.rs` は両 backend の独立 fixture で subtree・Issue・和集合、外部参照、保存先非変更と条件未実行を確認し、壊れた管理 root でも docs と雛形が取得できることを検査します。
