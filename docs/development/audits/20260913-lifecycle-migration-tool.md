# 専用移行ツールの隔離検証（2026-09-13）

［公開用編集：外部プロジェクトの識別情報・運用詳細を一般化しています。］


対象は `tools/lifecycle-migration`。設計commit `59c1920` の後に実装した専用crateを、合意済み8管理rootのコピーで検証した。以下の件数はこの検証snapshotに限定する。実際の管理root・共有binary・plugin・Git driverは切り替えていない。

## 入力と経路

SQLite 6rootは同日調査時に取得した独立コピー、sutologは旧file schema 13のコピー、Axonはこのworktreeの現行file snapshotのコピーを使用した。すべてのapply/restore先は `/private/tmp/axon-lifecycle-rehearsal-196oqpu9` 配下。新しい正式移行jobでは、停止窓の最新入力をあらためてbackupする。

| コピー元 | Entity | 候補のNote | うち既存Note | 経路 |
| --- | ---: | ---: | ---: | --- |
| dotfiles | 5 | 5 | 0 | prepare → apply → restore |
| external-project-a-docs | 非公開 | 非公開 | 非公開 | 非公開 |
| external-project-a | 非公開 | 非公開 | 非公開 | 非公開 |
| cacheexec | 7 | 12 | 5 | prepare → apply → restore |
| ediro | 2 | 2 | 0 | prepare → apply → restore |
| external-project-b | 非公開 | 非公開 | 非公開 | 非公開 |
| sutolog | 401 | 802 | 401 | prepare → apply → restore |
| axon | 177 | 295 | 全Noteを保持 | 現行snapshotをbyte単位で保持 |
| 旧axon main（追加検証） | 164 | 263 | 99 | prepare → apply → restore |

旧axon mainの追加検証は旧file schema 14の実入力読取の確認に使った。正式採用ではその候補を使用せず、このworktreeの既存移行成果と追加記録を保持する経路を使う。

## 確認結果

- 全candidateについて現行公開codec/adapterの再読照合と、新CLIの `storage check` が成功。適用後の全rootを新CLIの `list` で読めた。
- 変換処理は全EntityのID・宣言・包含・dependency・状態対応、既存Noteの全fieldと因果関係、Entityごとの原文Noteを照合した。SQLiteの元table rowは列の型と値ごと原文Noteへ保存した。
- 移行後の条件評価が想定どおり動作することを確認した。保存済みcacheexec Commandは静的に文字列を確認し、実行していない。
- restore後のfileはbackupとbyte一致。SQLiteはschema/versionと全tableの全列値を独立したPython読取でも照合し、一致した。
- 引数不正はexit 2、writer停止確認なしのapplyはexit 1。両方ともstdoutは空、stderrに理由が出た。
- 最終ソースの `cargo test --locked --manifest-path tools/lifecycle-migration/Cargo.toml --offline` は15 tests成功。旧file/SQLite × schema 13/14、条件、包含補正、Note/履歴分岐、Merge、未知field/参照拒否、WAL、適用・復旧前後の障害、checkpoint後のプロセス終了、再実行、artifact/source改変、復旧後の再apply拒否、新記録を消さない復旧拒否を含む。
- 同crateの全targetに対するClippy（`-D warnings`）が成功。本体の公開コード・CLI契約の変更はない。

## 保持したartifactと再開

同temporary directoryの `config-final.json`、`job-final/`、`config-old-validation.json`、`job-old-validation/`、`frozen-bin/`、`rehearsal-report.json` を保持した。各jobにはsource/candidate/report/digestと適用・復旧journalがある。temporary artifactは恒久保管や正式移行のbackupの代用ではない。

jobは実行binaryを固定しているため、別buildでそのjobを再利用しない。実コピーの往復後、未公開のapplyをrestoreした場合にjobを閉じる処理を追加し、その境界を最終テストで確認した。正式移行は最終ソースから固定binaryと新しいjobを作る。

## sourceと候補の固定digest

BLAKE3。SQLiteのsourceは型付き全table dump、candidateは現行canonical file snapshotのdigest。fileのsource/candidateは元bytesのdigest。

| コピー元 | source | candidate |
| --- | --- | --- |
| dotfiles | `2ab6ffbfe61758b80542162c2146cd750a9569032873be25f0b6a7e805561d1f` | `9bbea53a10f40d45fc995c710497f1e7b3f630bd10a4d82a166c9a2dfca16939` |
| external-project-a-docs | 非公開 | 非公開 |
| external-project-a | 非公開 | 非公開 |
| cacheexec | `73d191f171ddb3bdeb0fa1cb6d43dac7745a87304bc973e4f2e4f8b038335f02` | `2f7000afe1394fafd6d77419d8982baf7797615f3154645aaf67da40e91e7ddf` |
| ediro | `dd07a4a6314bd34e1f1fead00fe01c4452ae4f5ead6098a36a48d800e6c3db09` | `f4a05212ced6ff4b730ae0adda5e534022684da6d301c7b40b7b359a64ab60d4` |
| external-project-b | 非公開 | 非公開 |
| sutolog | `696c513c7587c824c423b42e9c302b6d818d607d8ee0c73beac05d2b72263581` | `0eea1f4f4a5f458420be2402a69d11c4355cca7357df503fca7e35ee42c3a421` |
| axon | `ddd0b0d69b9d5c2f7ef6c68654666d4119aa9703deeeeee5df68e3617194eaec` | `ddd0b0d69b9d5c2f7ef6c68654666d4119aa9703deeeeee5df68e3617194eaec` |
| axon-old-validation | `25c0ecd4a4fa1145e9168cd25a78035fcbb8ea4a050bba3b8b79fb3df7b3ea4b` | `a4beca36ce4801303138773ea8e94133c16ef7ce7336c064cc25b4b8b8588b71` |
