# 更新系診断の棚卸し（2026-09-06）

対象は axon-1ag24h、着手時ソース `1dda2f8`。実データを変更する障害注入は行わず、
コードの保存境界と既存 Rust tests を確認した。以下の「現行」はこの時点の診断。
成功/no-op の変更有無は axon-1x115z、option 形式の値と parser tip は axon-nnb208、
外部参照 snapshot の説明は axon-50qc1t が所有する。

| 更新系・代表経路 | 確認根拠・現行診断 | 適用保証と処分 |
| --- | --- | --- |
| plan / capture / group plan / group capture | main::cmd_create の title/body/parent 検査、core::Insert。空 title、存在しない親、包含制約を拒否 | 保存前の入力拒否は未適用。理由維持、操作 context と入力 file path を追加。作成は非 idempotent |
| write title/description | main::cmd_write は別々の store.apply。固定宣言は DeclarationFixed | 拒否理由維持。title 成功後の description 失敗が先行成功を示さないため修正。no-op 判定は別 Issue |
| dep add/rm | core::dependency と validate_structure。自己依存入力が InvalidState → DbError::InvalidSchema | 通常入力の自己依存だけ専用エラーに修正。保存 snapshot の不正は schema エラーを維持。循環・固定宣言・包含の理由維持 |
| group set/unset | core::Change/validate_structure/validate_relations。NotGroup、Containment、Cycle | 未適用の拒否。既存の制約説明を維持し操作 context 追加 |
| start/done/release | core の Progress/ready/子孫条件、SQLite immediate transaction / file lock | 同値遷移・成立条件未達・既存 claim の拒否を維持。診断のための評価を追加しない |
| decide / when at, after, manual, command, clear | core::unchanged_transition、main::cmd_when date parse、関係検査 | 同値は非0、履歴・updated_at 不変。理由維持。自由記述 reason は表示しない |
| note add | read_description、core::AddNote、adapter の保存 | 空白本文は未適用。非 idempotent な追記。原因維持、操作 context と file path 追加 |
| import prepare | declaration::prepare/atomic_write | DB 不変。一時 file 作成・sync・rename の失敗は元宣言置換前。path/phase と未適用範囲を追加 |
| import apply 入力・stale・固定・循環・外部評価 | declaration::validate_against、parse_canonical、transactional_import | 全体を commit 前に拒否。既存理由維持。外部参照固有の案内は axon-50qc1t |
| import apply 保存失敗 | db::transactional_import、FileStore::publish | 保存先/phase を追加。SQLite commit 前は未適用、commit エラーは保守的に不明。file replace 前は未適用、replace 後は不明を維持 |
| import apply 保存後の宣言 refresh | declaration::apply。現行は I/O 原因だけ | 保存済みと宣言未更新を区別する。既存の最終値一致による再 apply 仕様を案内。axon-cvx8f4 の診断候補を本件で扱うが採否は変更しない |
| 全通常 mutation の共有保存境界 | db::mutate、FileStore::mutate/publish、storage::open | SQLite/file の保存 path・phase・適用範囲を明示。破損データと入力誤りを分ける。lock/open/read 失敗で新規追加を再実行する案内をしない |
| init | storage::init_with/create。pending/state/config の順に公開 | incomplete init、設定不一致、既存状態保持の案内維持。state 成功後 config 失敗の段階を補足。backend/初期化の原子性は変更しない |
| migrate | db::migration::Failure は database/stage/version/applied/backup を保持 | 構造と元データ保護を維持。旧 automatic migration の復旧案内と途中出力があっても操作未実行とする断定を修正 |
| merge prepare/check | merge_cli の workspace 作成、frozen/compute/check。check も候補・reportを書き込む | workspace の部分生成はあり得る。入力/出力 drift、conflict の理由維持。prepareは作成済みdirectoryとartifact完成度不明、checkはartifact更新結果不明、両者でcandidate未公開を明示 |
| merge apply/driver | merge_cli::apply_with/atomic/run | replace 前の destination 未適用、後の不明を保持。driver は失敗時 conflict marker を出すことがあるため全未適用とはしない。path/phase を追加 |
| merge setup | git config 3項目と .gitattributes の順に更新 | 後段失敗は前段設定済み。設定済み項目を保持して診断、勝手な rollback/retry はしない |
| 保存後の stdout 失敗 | main::write_output_to | BrokenPipe は成功維持。その他 I/O は既に保存した結果を失敗と混同しないよう保存済み確認の出力失敗と区別 |
| Clap 構文検査 | cli_command/get_matches | stderr/exit 2 を維持。自由記述 option の tip 修正は axon-nnb208 |

新しい保存方式、原子性、復旧機能を必要とする採用 finding はない。障害範囲の表示は
既存の保証に限定し、durability 不明を未適用と断定しない。merge workspace の cleanup や
init の自動修復は追加しない。形式モデルの状態意味は変更せず Rust で検証する。

## 修正と回帰検証

同日の隔離 Rust/CLI tests で次を確認する。新しい Fault 注入はこの表の保存境界だけに限定した。

| 経路 | 検証根拠 |
| --- | --- |
| 自己依存 | `tests/mutation_diagnostics.rs::self_dependency_is_an_input_rejection_and_preserves_both_backends`: SQLite/file の dep add と import prepare/apply が同じ理由・exit 1、state bytes/宣言 file 不変 |
| SQLite 保存不可 | 同 `sqlite_import_readonly_failure_preserves_storage_and_declaration`: 隔離 DB の read-only 権限、path/操作/未適用、DBと宣言 bytes 不変 |
| file Note保存不可 | 同 `file_append_io_failure_names_storage_phase_and_keeps_all_saved_bytes`: directory permission、置換前未適用、state bytes 不変 |
| 入力 file | 同 `input_file_errors_identify_operation_and_file`: Note の操作・ID・入力 pathを保持 |
| write 後段失敗 | `main::tests::write_reports_saved_title_when_description_transaction_rolls_back`: open後の隔離SQLite triggerでdescriptionを拒否。title保存、description/判断履歴/Note不変、段階表示一致 |
| import refresh | `tests/declaration.rs::apply_retry_repairs_the_file_after_a_post_commit_rewrite_failure`: directory permissionで宣言置換を拒否、保存値適用済みとfile未更新、同一file再applyで復旧 |
| SQLite transaction rollback | `db::tests::sqlite_failure_rolls_back_every_imported_entity` / `publication_failure_rolls_back_control_revision_and_history`: triggerで複数Entity・Revision・履歴の保存途中を拒否、未適用表示とrollback一致 |
| file replace 後 | `storage::tests::failure_boundaries_and_drift_do_not_partially_publish`: before-write/after-sync/after-replaceの注入、後者は結果不明で完全な新snapshotが残る |
| init | `storage::tests::initialization_fault_diagnostics_match_published_stages`: 両backendでstate/config公開後の注入、config公開時I/O失敗のsource保持、診断と生成済みfileの一致 |
| merge setup | `tests/merge_cli.rs::setup_reports_saved_configuration_when_attributes_cannot_be_read`: 属性pathをdirectoryにし後段を拒否、3項目のGit config保存と診断一致 |
| init SQLite一時DB失敗 | `tests/mutation_diagnostics.rs::init_size_limit_failure_reports_retained_marker_and_unpublished_state`: 子processだけにRLIMIT_FSIZE（SQLite=1024、file=100）/SIGXFSZ無視を設定。pendingと一時fileが残り、最終state/config未公開、保存段階と復旧案内を確認 |
| merge prepare後段 | `tests/merge_cli.rs::prepare_destination_resolution_failure_reports_preserved_workspace`: 存在しないoutput親を指定し、入力3snapshot保存済み・candidate未公開・新workspace案内を確認 |
| merge publish | 既存 `merge_cli::tests::publish_faults_preserve_inputs_and_allow_reconciliation` と CLI suiteで置換前後・drift・conflict marker保持を確認 |

SQLite commitそのもののdurability障害、disk full、OS crash、全stdout I/O失敗を網羅した
検証ではない。commitのエラー表示は実装が成功を確認できないことから不明を選び、未適用を
保証しない。確認出力のBrokenPipe契約は既存CLI testで維持する。保存方式・原子性の変更や
失敗時の自動mutationは追加していない。axon-cvx8f4の保存先/phase/適用結果の診断候補は
この修正で扱ったが、そのIssueのDispositionやscopeを変更していない。


## init の marker 公開後の保存境界

2026-09-06 の再確認では、SQLite 一時DB失敗の局所補足が file 作成・SQLite 公開経路を
覆っていないことを追加再現した。修正済み経路の再発ではなく、未被覆の隣接経路だった。
成功が確認された marker/state/config を一つの適用段階リストで保持し、marker の削除が
成功した時だけその項目を外す。区間内の全エラーは共通 wrapper を通る。保存順序は変えない。

| marker 公開後の fallible operation | エラーの適用範囲 | 検証 |
| --- | --- | --- |
| state の存在確認による拒否、file state の encode/temp作成・write・sync・hard-link・temp削除・directory sync | marker を Applied。最終stateの置換前は Not applied、公開後の同期は Result unknown | after-marker / before-state-publication、file の実 RLIMIT_FSIZE失敗、create の公開前後境界確認 |
| SQLite temp作成・init_at・temp sync | marker を Applied、最終stateを Not applied | before-state-publication、SQLite の実 RLIMIT_FSIZE失敗 |
| SQLite hard-link・temp削除・directory sync | marker を Applied。hard-link前は Not applied、公開後の同期は Result unknown | before-state-publication / state-published、共通wrapperの全return経路確認 |
| 公開済みSQLiteのopen/readback、prefix整合、after-state確認 | marker/state の確認済み公開を Applied | state-published / after-state |
| config のID取得・serialization・公開 | marker/state を Applied。config公開前は未適用、公開後の同期失敗は不明 | before-config、config-publication-failure（I/Oのsource保持） |
| config公開後の確認、marker削除 | marker/state/config を Applied、cleanup失敗の原因を保持 | after-config / before-marker-cleanup |
| marker削除後のdirectory sync | state/config を Applied、markerをAppliedへ残さずcleanupを Result unknown | after-marker-cleanup |
| 最終prefix取得 | 確認済みstate/config を Applied | 通常成功と共通wrapperの全return経路確認 |

`initialization_fault_diagnostics_match_published_stages` は上記8 checkpointとconfig公開障害を
両backendで計18条件検査し、診断にある Applied の3項目と実fileの有無を比較する。
全syscallの障害注入ではない。実I/Oについては別のCLI size-limit testで両backendのmarker・
一時file残存と最終state/config未公開を確認する。marker自体の公開結果が不明なら、
成功したmarkerとしてリストへ追加せず、その保存境界の Result unknown を保持する。
