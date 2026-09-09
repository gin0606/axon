# 更新結果の横断確認（2026-09-06）

対象は axon-2b4xyp。共通契約は [CLI 契約](../../reference/cli.md#更新結果の共通契約)、
比較対象は契約 commit `1dda2f8`、失敗側 `880a997`、成功側 `19b4262`。
同 worktree の clean `19b4262e00d7a8884c5f91292307660dba7cc3ee` から `cargo build` した
`target/debug/axon --version` は `axon 0.1.0 (commit 19b4262e00d7a8884c5f91292307660dba7cc3ee; source clean)`。
以下はこの時点・条件での確認であり、将来の実装状態を表すものではない。

## 契約と代表経路

| 結果 | 代表経路と根拠 | 出力・保存との対応 |
| --- | --- | --- |
| 実変更 | `tests/integration/cli.rs::setting_confirmations_follow_saved_changes_and_preserve_noop_storage`、`main::tests::write_uses_description_save_outcome_after_title_changes_storage` | stdout / 0、完全ID先頭、実際に保存した Title / Description のみ列挙。削除は Description removed。dep / Parent は相手IDまたは最終値 |
| 新規追加 | `tests/integration/cli.rs::mutation_confirmations_begin_with_the_affected_entity`、`cmd_create`、`cmd_note` | stdout / 0、Created または Note の安定ID + recorded。非idempotentな追加で、同題・同文はno-opではない |
| 成功no-op | 上記設定matrix、`tests/integration/cli.rs::settings_are_noops_but_transitions_reject_repetition` | stdout / 0、No changes。関係は already present/absent または Parent 最終値を添える。両backend・Issue/Group・固定宣言の同値設定で保存bytes不変 |
| 適用前拒否 | `tests/integration/mutation_diagnostics.rs::self_dependency_is_an_input_rejection_and_preserves_both_backends`、同値遷移の既存検査 | stderr / 1、Error: 対象 操作: 原因。自己依存は cannot depend on itself、保存schema異常とは別型。通常depと宣言経由の原因が一致。保存bytes不変。同値遷移は No changes にならず必要なHelpを末尾に表示 |
| 実行失敗・未適用 | 同 `file_append_io_failure_names_storage_phase_and_keeps_all_saved_bytes` / `sqlite_import_readonly_failure_preserves_storage_and_declaration` | stderr / 1、操作・保存path・失敗phaseと Not applied。置換前失敗で正本bytes不変。入力file失敗もID・操作・fileを特定 |
| 部分適用 | `main::tests::write_reports_saved_title_when_description_transaction_rolls_back`、`tests/integration/declaration.rs::apply_retry_repairs_the_file_after_a_post_commit_rewrite_failure` | 後段失敗でも全体は非0。writeは保存済みTitleと後段未適用、importは storage declaration values と宣言file未更新を区別。原因→段階結果→Help。init/migration/mergeの段階は既存[診断棚卸し](mutation-diagnostics-2026-09-06.md)の境界表へ対応 |
| 結果不明 | `storage::tests::failure_boundaries_and_drift_do_not_partially_publish`、`main::tests::unknown_result_guidance_uses_valid_note_commands_and_initialization_files` | file置換後障害は Result unknown。完全な新snapshotが残っても同期の成功は保証しない。SQLite commitエラーも不明を維持。Note ID・本文の照合を案内し、無条件append retryを勧めない |
| 複数EntityとDB/file | `tests/integration/declaration.rs::prepare_check_and_apply_create_mixed_entities_and_dependencies` と上記import refresh検査 | Plan is valid. → Changes: → Derived changes → Applied <path>。Changes: none はDB no-op、Appliedはfile refreshも含む完了。checkの予測とapplyの保存成功を区別 |

失敗時の適用範囲は、原因を記述する本文に続く段階結果で説明する。単純な拒否に空の
Applied欄や定型Helpを増やさず、保存済み範囲・未適用・不明が混在する場合だけ列挙する。
診断生成が追加mutationや外部Command評価を行わないことをmainのcontext生成と各保存境界で照合した。
helpの同値遷移拒否・設定no-op・保存復旧の説明も契約と一致する。
Clapのoption形式の自由記述tipと宣言の外部参照案内は、それぞれ後続Issueの担当を維持する。

## 追加した端末条件の確認

2026-09-06、macOS上の隔離した一時管理rootで、SQLite/fileのそれぞれについて次の5経路を
pipe、stdout/stderrを別々のPTYに接続、同PTY + `NO_COLOR=1` の3条件で実行した（計30条件）。
PTYでは端末ドライバの改行変換を無効化し、`TERM=xterm-256color`、ANSI条件ではNO_COLORを解除した。
各条件は同じ隔離snapshotから開始し、操作終了後に比較した。

| 経路 | 終了コード | 出力先・文字構造 |
| --- | --- | --- |
| `write ID --title updated -m body` | 0 | stdoutのみ、`ID  Title updated  Description updated` の1行 |
| 同値 `write ID --title original` | 0 | stdoutのみ、`ID  No changes` の1行 |
| `dep add ID --needs ID` | 1 | stderrのみ、自己依存原因の1行 |
| Undecidedに `decide undecide ID` | 1 | stderrのみ、Error行と末尾Helpの2行 |
| `write ID --not-an-option` | 2 | stderrのみ、Clapのerror・tip・Usage・help |

全30条件で期待する終了コードと出力先が一致した。ANSI条件では実際のANSI escapeを確認し、
それを除去するとpipeとNO_COLORの文字・空白・改行・順序が完全一致した。
no-op・拒否・parser errorの24条件では正本bytesも不変だった。
最初のPTY harnessは子終了後にslaveを閉じたことでmacOSの未読出力を失ったため、
slaveを読み取り完了まで保持するよう修正してから全条件を検査した。製品側の変更ではない。

## 検証根拠と制約

先行Issueの保存検査を無理由に再実行せず、Note
`axon-1ag24h / note-de9874317fc0653a10a2d7d3c3a6ed0a` と
`axon-1x115z / note-6b4a9b786d95b5ca1ea081268c04b7d2` を一次記録として参照した。
後者は両実装を含む `19b4262` のcommit前後で217 tests成功・3 ignored、fmt、clippy
（all-targets/all-features）、commit hookの検査成功を記録している。
本確認は上記の新しい端末条件とソース照合を追加した。続く独立レビューで下記の障害時表示の不整合を確認し、修正した。

追加のPTY検査は代表的な成功・拒否・parserの実行経路を対象とし、全障害のPTY再現ではない。
部分適用・結果不明の保存根拠は先行の隔離障害注入と共通rendererの照合に基づく。
全syscall、OS crash、SQLite commit durability、並行writer、全stdout障害の保証へは広げない。
状態意味・原子性・保存方式の変更はなく、形式モデルの追加検証は対象外。

## 独立レビューで確認した表示の修正

- 成功確認のstdout書込が非BrokenPipeエラーになった場合、stdout用に装飾した結果を
  stderrのAppliedへ流用していた。結果を出力時にrenderし、失敗時の段階結果はPlainで
  再生成する。利用者が保存した文字列をANSI stripにかけず、付与した装飾だけを切り替える。
  `main::tests::confirmation_failure_keeps_stdout_decoration_out_of_applied_result` は
  ANSI出力writerへ障害を注入し、plainのAppliedと利用者文字列の保持を確認する。
- mergeのatomic置換後同期エラーに埋め込まれたHelpが、prepare/check/setupの外側の
  適用範囲より前に出ていた。driverも内側のHelpに続けてconflict markerのAppliedと
  別Helpを追加していた。ArtifactFailureで原因・範囲と案内を分離し、最上位の操作に
  対応するHelpを最後に一度だけ出す。setupはGit configと.gitattributesの確認を案内する。
  `merge_cli::tests::published_file_failure_keeps_guidance_out_of_nested_stage_details` は
  実file置換後のdirectory sync障害を注入し、保存bytes、Result unknown、外側の
  段階結果→Helpを確認する。`tests/integration/merge_cli.rs` の
  `relative_driver_output_preserves_original_conflict_diagnostic` と
  `setup_reports_saved_configuration_when_attributes_cannot_be_read` は実CLIの重複なし・末尾Helpと
  保存済み範囲を確認する。

いずれも保存順序、原子性、判定結果、追加mutation/Command評価を変更しない。
低位の原因・段階結果を保持したまま、出力先の装飾と操作固有の案内の位置を修正した。

最初の修正後に `cargo test`（219 passed、3 ignored）、`cargo fmt --check`、
`cargo clippy --all-targets --all-features -- -D warnings`、`git diff --check` が成功した。
renderer変更の再確認として端末30条件も再実行し、同じ期待値・保存不変条件を満たした。


再レビューでは、merge driverのconflict marker処理が追加で失敗すると、それ以前の
原因と保存済みworkspaceの範囲が消える経路を隔離CLIで確認した。
marker前のdestination読取とmarker公開の両エラーを、元の失敗を保持した追加段階として
表示するよう修正した。`driver_marker_write_failure_retains_original_failure_and_workspace` は
oursの親directoryを読取専用にし、元の入力不備・保存済みworkspace・marker未適用・末尾Helpと
実際に保存されたours/theirsを照合する。`driver_destination_read_failure_retains_the_original_context`
は読取段階の障害で元の診断が保持されることを確認する。

追加修正後の `cargo test` は221 passed・3 ignoredで終了0。
`cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、
`git diff --check` も成功した。

## merge の失敗処理中にある追加操作の棚卸し

追加の独立レビューで、checkの入力driftを記録するreportの保存失敗が、元の原因を
置き換えることを確認した。merge_cli全体のErr分岐・map_err・report保存・marker処理・
cleanup・確認出力を以下の単位で照合した。局所修正済み箇所の再発ではなく、未被覆の
report経路を追加で閉じたものであり、検査範囲と保持する原因が増えている。

| 元の結果 → 追加のfallible操作 | 原因・範囲の保持と検証 |
| --- | --- |
| prepareの入力保存失敗 → 残りの入力保存 | 全入力を試し、各失敗の入力名/path/原因を蓄積。読めた入力を保持。既存missing-input CLI検査を維持 |
| 蓄積した入力保存失敗 → invalid report保存 | `save_failure_report` は元の原因をsourceとして保持し、reportの失敗path・未適用/不明を追加。`prepare_report_failure_keeps_input_preservation_errors` はreport pathをdirectoryにし、ours/theirs保存と元のmissing-base原因を照合 |
| checkの入力/計算/保存失敗 → invalid report保存 | 同じ関数で元エラーと後段エラーを結合。`check_report_failure_retains_the_input_drift_cause` はbase snapshot欠落とreport pathのdirectory化を組み合わせ、両原因とdestination不変を確認 |
| 計算済みの未解決conflict → choices読取・conflict report保存 → 失敗report保存 | 未解決という計算結果を先に保持し、reportの失敗を追加。既存のreport保存順序を変えない。`conflict_report_failure_retains_the_unresolved_result` でconflictとreport保存拒否の両方、末尾Help、destination不変を確認 |
| driverのprepare/apply失敗 → destination再読取 → marker公開 | `driver_conflict` と `driver_followup_failure` が元エラーをsourceとして保持。上記の読取障害・実CLI permission検査で両経路を検証 |
| setupの各Git config保存 → 後続config/attributes/確認出力 | 確認済みconfig項目を保持し、attributes失敗はArtifactFailure、確認出力失敗はwrite_mutation_output。元エラー処理中にGitの追加操作はない。attributes実CLI検査を維持 |
| atomicの置換前操作 → directory sync | 置換前はNot applied、後はResult unknown。失敗後にcleanup/retryは行わない。directory sync注入で新bytesと結果不明を確認 |
| prepare/apply/setupの成功 → 確認出力 | write_mutation_outputは既知のAppliedを付ける。BrokenPipeを除くI/O失敗は成功としない。元のエラーを受けてさらに出力する分岐はない |
| check開始時のchecked.json削除・lock/file drop | checked削除は検証前で、処理中の元エラーはまだ存在しない。dropには追加の明示fallible復旧操作がない。失敗後にcheckedを作らないことを上記report検査で確認 |

errorの合成は表示と案内だけを変え、操作の採用判断、保存順序、復旧の自動化は追加しない。
report障害は実際のfile操作で検査したが、全syscallや並行変更の完全網羅を意味しない。

棚卸し後の最終検査は `cargo test` 224 passed・3 ignoredで終了0。
fmt、clippy（all-targets/all-features）、diff checkも成功した。
