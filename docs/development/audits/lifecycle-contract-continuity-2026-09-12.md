# 単一lifecycle移行の契約照合

2026-09-12、旧実装 `367030ce` と移行後 `979ace3` を比較した。旧コード・テスト・referenceは判断理由と契約の確認に用いる。新しい状態・構造・情報モデルは `spec/lifecycle_proposal.md` を正本とし、旧Revision・claimを復活させる根拠にはしない。この文書は時点付きの監査記録であり、現況一覧ではない。

## 判断基準

今回合意された変更は単一lifecycleとそれに伴うコマンド体系、一覧内容、show内容の整理。影響しない既存契約の欠落は復元する。旧契約が新モデルに対応する場合は目的を保って適応する。廃止・縮小、新たな公開機能の選択、権限境界の変更は根拠と案を提示してユーザー判断へ返す。既にspecやユーザー判断で明記された置換を再承認の対象にはしない。旧新の互換入口は設けない。迷うものは確定した作業の完了後に具体案と根拠を返す（2026-09-12のユーザー指示）。

## 比較結果

「復元」は本作業の実装方針であり、この表だけでは実装完了を意味しない。実施結果と検証条件は末尾へ記す。

| 契約・理由 | 旧根拠 | 移行後の差と判断 |
| --- | --- | --- |
| prefix＋ランダム6文字。順序・優先度をIDへ混入させない | design/decisions.md D-2、旧domain.rs | 128bit表記への拡大にコア上の必然性なし。復元。保存済みIDは維持し衝突は再生成する |
| Issue/Group共通namespace、一意なsuffix参照 | D-2、旧db.rs resolve_id | 全操作・関係先で復元。曖昧な候補は列挙し変更しない |
| 管理rootから既定prefixを導出、initで上書き | D-2、reference/cli.md | axon固定は欠落。復元。内部Record/Store IDとは区別する |
| CLI生成文は英語、利用者の本文は原文 | 旧main.rs、旧CLI統合テスト、ユーザー再確認 | 日本語のhelp・状況・logラベルを英語へ復元 |
| TTYだけ意味を持つ装飾、NO_COLOR、非TTYと同じテキスト | reference/cli.md「出力の装飾」、旧main.rs OUTPUT_* | 消失。復元。タイトル・本文は着色しない。追加の色無効化optionは別課題 |
| C0/C1/ESCの可視escape、Unicode・改行保持 | reference/cli.md、旧表示テスト | human_textに存続。すべての新表示にも適用する |
| human時刻はlocal時刻＋UTC offset | reference/cli.md、display.rs | 存続。保存のUTCとは分ける |
| bare/help/-h/--helpは共通の簡潔なroot help、用途別配置 | reference/cli.md、旧HELP_SECTIONS | bareは終了2、階層が平板化。新コマンドへ対応して復元 |
| help/docs/version/completionは保存先不要 | reference/cli.md、旧main.rs | docs/completion消失。新モデルを説明する同梱docsとcompletionを復元 |
| versionはビルド時のcommitとsource状態 | reference/cli.md、build.rs | build.rsは存続するがCLI未接続。再接続 |
| 作成titleは位置引数、本文は-m/--message・-F/--file・stdin | reference/cli.md、旧Create | ユーザーが意図した変更と再確認。新--title/--description/--description-fileを採用し、旧位置引数・旧本文long optionは残さない |
| 初期Command条件は作成と同時保存し実行しない | 旧作成CLI、候補の契約 | --command欠落。新条件モデルに沿って復元 |
| timeoutは正整数＋ms/s/m/h、既定30s | reference/cli.md、旧parse_condition_timeout | 秒数f64への無理由変更。単位付き入力を復元 |
| listのkind・保存状態・terminal絞り込み | reference/cli.md、旧ListFilters | 無filter化。kind/terminalを復元し旧2軸を--lifecycleへ適応 |
| literal・case-sensitive検索、title/body/全Note、AND、該当箇所表示 | reference/cli.md、旧ListFilters | --search欠落。通常list内容を保ち検索時だけMatchedを付記 |
| 専用候補のkind/search絞り込み後に必要な祖先だけ評価 | 旧ListFilters、旧候補テスト | 対象外条件の実行を防ぐため新候補意味論に適応 |
| 空stdoutはレコード0件、案内はstderr、終了0 | reference/cli.md、旧一覧テスト | 案内欠落。候補が全inventoryではないことを短く説明 |
| 成功は完全ID先頭、実際の効果を短く確認 | reference/cli.md「mutation」、旧監査 | Updatedだけ等へ退行。保存transaction内の実変更から確認文を生成 |
| 成功no-opと実変更を区別し履歴を増やさない | 同上 | 判定なくupdatedと表示。write/parent/dep/conditionで結果を明示。旧同値の状態遷移拒否は維持 |
| app Error:、終了1、構文終了2、stderr診断 | 同上 | Error:と操作対象の文脈欠落。復元 |
| BrokenPipeは成功、保存後の出力失敗はAppliedと区別 | 同上、旧統合テスト | BrokenPipeまで失敗化。復元。その他出力障害は保存済み範囲を明示 |
| SQLite commitの失敗は再読まで結果不明 | reference/cli.md、旧mutation監査 | raw SQL errorだけ。commit境界にResult unknown診断を戻す |
| tasks/triage/list、作成日時順、状態から導出する状況 | lifecycle_proposal.md「CLIと表示」 | 合意された置換として維持。ready/claims、旧状態filterは復活させない |
| showは本文・直接の未充足前提・直属の子だけ | 同上 | 合意された簡素化として維持。孫や充足済み依存は通常表示へ戻さない |
| 保存条件・全直接関係・逆依存の明示参照 | 旧show、旧skillの影響調査 | 通常show簡素化と別に参照入口が必要。ユーザーは必要情報を重複なく取得する入口を指定。show --detailsで待ち理由節を置き換え、全直接関係を一度ずつ表示 |
| Note一覧は全文、日時・actor、詳細dataは明示指定 | 新spec「Noteと履歴」 | 合意済み。旧summary indexへ戻さない |
| 安定Note IDから個別Noteを取得 | 旧note show、旧add-note workflow | coreにpoint lookupがあり衝突しない。CLI入口を復元 |
| logは状態変化と統合、分岐の順序を捏造しない | 新情報spec、lifecycle-core.md | 合意済み。旧判断/進行の別logやRevision表示は戻さない |
| recorderは任意、actorは所有権・認可ではない | 新spec、旧conventions | 存続。actorの読取utilityを戻し、未取得は—で明示。必須actor/claim推論は持ち込まない |
| skillはplugin単位で持ち出せる | 旧plugins、旧conventions references | checkout相対docs参照と絶対binary指定必須が混入。配布内referenceと対象rootでのCLI発見へ復元 |
| kitは操作意味論、個人workflowは協業と権限 | 旧plugins/axon-kitとaxon | 層の境界を保ち、新状態へ適応。状態操作から実装・commitへ権限を拡張しない |
| 登録は重複・採用根拠・完了条件を検査 | 旧register、creation.md | 目的・範囲・kind・構造の照合と一意再利用を復元 |
| 結果不明時は保存済みpayload・ID・記録を照合、非冪等操作を盲目的反復しない | 旧mutations.md、add-note | 薄い一般論から具体手順を復元。任意recorderを必須条件にしない |
| Group doneは全計画の最終確認、子の終了だけで自動完了しない | 新spec、旧work-state | 存続させ、直属表示からの再帰的確認手順をskillに明記 |
| Git/fileはworktree別、SQLite共有、artifact境界でfallbackしない | 旧file-storage、新location/file tests | 新実装で存続。共有worktreeは独立テストfixtureではない |
| init途中・置換前/後・merge drift・indexの保存境界 | 旧file-storage、新lifecycle-file | 新実装で検査継続。skillに配布内で復旧手順を持たせる |
| 旧schemaの自動移行・旧declaration export/import | 新specの非要求・一括編集の扱い、Group axon-rcc08mの対象外 | 今回のコア再構築の実装範囲外。未対応機能をskillだけで実行可能と説明しない。正式移行計画は既存Entityに保存済み |
| 対応中schemaのSQLite snapshotから明示backend変換 | 旧migrate、reference/migration.md | 旧schema自動移行とは別機能。廃止の合意を確認できず、単なる保存型変更だけでは不要にならない。新仕様で機能を戻すか判断待ち。未実装のまま境界を明示 |

## 検証方針

新モデルの制約を保ったまま、公開CLIから両backendでID参照、入力、絞り込み、出力、履歴、失敗境界を検証する。既存テストの日本語ラベル期待値は新しい英語表示に更新するが、ユーザー本文の日本語fixtureは残す。skillはplugin配布物だけを参照して動けることと、未対応旧コマンド・旧モデルへの依存がないことを検査する。

## 残るユーザー判断

旧 `migrate --source current-schema.db --output NEW_DIRECTORY --backend sqlite|file` は、対応中のschemaの保存済みsnapshotを明示的に別backendへ出し、backup・manifest・照合結果を残す機能だった。「旧schemaの自動移行をしない」という新方針だけでは、この明示変換機能の廃止までは導けない。

新モデル専用の明示変換として戻す案を推奨する。旧schema互換は持たせず、元DBを切り替えない。正式採用時の旧data移行は既存の移行Issueと分けて扱う。今回は未提供と明記しており、ユーザー判断前にCLIや変換器を追加しない。

## 実施結果と検証

2026-09-12、konseputo-minaosiの修正したソースと同梱pluginを検証した。復元・適応としたCLI/skill契約を実装し、意図されたタイトル・本文optionは新仕様だけを残した。旧位置引数・旧本文long optionは構文エラーとなる。判断待ちの明示backend変換は未提供のまま、Undecided Issue `axon-x08t3h` に観測・具体案・正式移行Issueとの境界を記録した。

- `cargo fmt --check`、`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`、`cargo test --locked --workspace` が成功。Rustテストは106件（共通コア45、binary4、CLI統合54、recorder3）。状態モデルの意味は変えず、Quintの再実行は行っていない。
- 追加した `tests/lifecycle/contracts.rs` は、両backendの6文字生成と全Entity入力でのsuffix参照、曖昧なID拒否、保存済みID維持、Note個別参照、literal検索・候補filterの評価境界、詳細表示の非重複・非実行、no-opのsnapshot/bytes保持、DB不要utility、旧入力拒否、単位付きtimeout、Unicode prefixを検証する。
- 実PTYで装飾あり/NO_COLOR/非TTYのテキスト一致と、ユーザーtitleが着色されないことを確認。閉じたpipeは成功、切断したdatagram socketによる別種の出力障害はAppliedを伴う失敗として、保存結果が残ることを確認した。
- 15個のSKILL.mdをskill-creatorのquick_validateで検査。両plugin内のMarkdown相対参照が配布物内で解決することを検査した。
- 独立agentが、pluginとbinaryをコピーしたGit外の隔離directoryで、file初期化、採用済みIssue登録、同じ依頼による再登録、Note追記を実行した。既存Issueを再利用して重複を作らず、指定Noteは1件だけ保存し、storage checkが成功。Axonの開発checkoutやインストール済みskillを参照せず完結した。使った作成構文は--titleと-Fで、廃止した互換入口に依存しない。
- このworktreeの実データには判断待ちIssueとCreated記録の2行だけを追加し、既存全レコードが保持されていることをJSON内容比較で確認した。実データの改番やschema再移行は行っていない。

現行の契約は [CLI入出力契約](../../reference/lifecycle-cli.md) と各plugin内のreferenceへ書き戻した。旧三軸資料を現行コアの規範へ戻したものではない。

## 正式移行IssueのID補正

2026-09-13、ユーザーの指定により、このworktreeの正式移行Issue `axon-454e0188d65fbdee8090f4c245831b2a` を `axon-hqsc5s` へ改番した。prefix＋6文字の現行生成規則でIDを生成し、file backendの通常のlock・検証・保存処理を通じて反映した。一回限りのデータ補正であり、公開renameコマンドや旧IDのaliasは追加していない。

更新対象はEntityのID、StateとNoteのEntity参照、および `axon-x08t3h` のdescription内の参照。変更前snapshotを退避し、この4か所を逆変換した全JSONレコードが元snapshotと一致することを確認した。Note本文・記録ID・日時・lifecycle・関係構造は保持されている。補正後の `storage check`、`show hqsc5s --details`、`log hqsc5s` が成功した。
