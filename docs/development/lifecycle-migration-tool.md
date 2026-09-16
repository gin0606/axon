# 単一lifecycleへの専用移行ツール（設計案）

［公開用編集：外部プロジェクトの識別情報・運用詳細を一般化しています。］


2026-09-13の専用ツール検討に基づく設計案。手元の旧Axonデータを一度移行するためのもので、通常のAxon CLIが旧形式を開く仕様にはしない。操作名・artifact名・crate配置は実装上の仮定であり、公開済みのコマンドではない。

この設計に基づく独立crateの操作・artifact・復旧手順は [専用ツールのREADME](../../tools/lifecycle-migration/README.md) を参照する。

## 合意済みの目的と制約

- 本体外の専用ツールで、backup、変換、検証、明示適用を扱う。対象は利用者が指定する管理rootとする。
- fileとSQLiteの両方を扱い、backendは維持する。現行schemaの汎用backend変換を検討する `axon-x08t3h` とは別の作業。
- 全EntityのID、kind、タイトル、本文、作成日時、包含、dependencyを保持する。既存NoteのID、本文、日時、記録者、因果関係も保持する。
- 新モデルへ直接対応しないRevision・判断履歴・進行履歴・Baseline等はEntityごとの移植元Noteへ原文を残す。新logは移植時の状態設定だと明示する。
- 状態を完全に対応できない場合は意味と関係を調べて合理的に補正し、元値、採用値、理由を残す。関係の削除や架空の完了で検証を通さない。
- 既存の作成入力・表示・lifecycle契約を変えず、通常CLIに旧構文のaliasや旧schema読取を追加しない。

2026-09-13、ユーザーは対象をdotfiles、external-project-a-docs、external-project-a、axon、cacheexec、ediro、external-project-b、sutologの8管理rootと確定した。SQLiteのlinked worktreeは共有する一つの正本を対象にする。Axon自身はこのworktreeの既存移行結果と、その後の追加記録を保持する。

正式移行そのものの目的・完了条件は `axon-hqsc5s` にある。専用ツールの実装と、実際の管理root・共有binaryを切り替える操作は区別する。

## 構成案

`tools/lifecycle-migration` に独立したRust crateと固定dependencyを置く。通常のAxon配布binaryには含めない。新形式の検証・生成は現行 `axon` libraryの公開codecとfile/SQLite adapterを使い、lifecycleの意味を移行ツールへ複製しない。

旧入力の読取と所有Entityへの原文分類は専用ツールに置く。対象はSQLite schema 13/14と旧file format 1・schema 13/14。未知schema、未知field、重複ID、参照切れ、型違反を黙って捨てない。旧コードは読取・検証契約の根拠として使うが、通常Axonへ再接続しない。

schema 13の日付解釈は旧13→14変換と同じUTC午前0時を使う。ただし変換前の履歴・payloadを原文保全してから解釈する。schema 14へ正規化した値だけを「移植元原文」として保存しない。SQLiteの原文相当は、所有Entityと結び付く全table rowの型・全column値を保持した表現とする。元DB自体も整合backupとして残す。

## 操作とartifact

操作案は `prepare`、`apply`、`restore`。対象root・backend・正本pathを明示した入力と、移行対象外に置くjob directoryを使う。

### prepare

1. 対象path、実backend、schema、store、Git worktree/common directory、旧新版の実体・digestを確認する。SQLiteの共有先を重複登録しない。
2. fileはlock下の元bytes、SQLiteはbackup APIによるWALの確定情報を含む整合snapshotを取得する。正本に通常の旧CLIを実行してschemaを更新しない。
3. 移行ID、移行時刻、新store/record ID対応をjobに一度だけ固定する。再開時に同じIDへ別の時刻・本文を割り当てない。
4. 変換候補、Entityごとの対応・補正理由、旧原文の所有関係を生成する。必要な明示overrideは元入力digestと結び付け、候補生成前に適用する。
5. 現行codecで全体を検証する。SQLite出力は公開adapterで未使用DBを作り、再読した論理snapshotを照合する。
6. 全field・全record・関係の集合を機械照合し、reportと候補digestを固定する。件数一致だけで成功にしない。

jobは対象ごとのsource backup、candidate、ID対応、状態・条件対応、原文保全の照合結果、binaryの由来、適用状況を保持する。内部artifactの差し替えを検知し、未完了のjobを成功したものとして扱わない。

### apply

writer停止を調整した切替窓で実行する。OS lockの取得や保存claimの有無だけを、全writer・旧接続の終了証明にはしない。

- fileは旧 `write.lock` と新 `state.lock` の双方を固定順に取得し、元bytes、backend、Git indexを再照合する。同期済みtemporaryから置換し、directory sync後に再読する。Git/editorは同じlockを使わないため、切替窓ではそれらの書込も止める。
- SQLiteは全旧接続を終了した状態で最新の論理snapshotをbackupと照合する。旧DB・WAL・SHMを対応するセットとして保全し、旧WALを新DBへ混在させない。切替の各段階を記録し、途中終了後もどの世代が正本か判定できるようにする。
- 入力が変わっていたら上書きせず、新しい入力で候補を準備し直す。同じjobの候補が既に適用済みであると立証できた場合は再移行しない。
- 複数root全体を一つのatomic transactionとは説明しない。rootごとの未適用・適用済み・結果不明を記録し、途中失敗では後続rootへの適用を止める。

toolはGitのmerge・stage・commit、PATH・plugin・共有Git driverの変更を実行しない。正式切替の実施手順でそれらを管理し、全利用先とbinaryが対応するまで通常利用を再開しない。

### restore

復旧前にwriterを止め、現在の正本も保全する。適用直後の候補と現在値が一致する場合に、対応するbackupを復元する。移行後に新しい記録が増えていれば自動復元を拒否し、その情報の引継ぎ判断を返す。自動rollbackやbackupの自動削除は行わない。

## 状態と条件の対応

基本対応は以下とし、候補全体で現行コアの制約を検査する。

| 旧状態 | 新状態 |
| --- | --- |
| Accepted / Ended | `Completed` |
| Accepted / NotStarted | `NotStarted` |
| Undecided / NotStarted | `Undecided` |
| 非RejectedのInProgress | `InProgress`。祖先の着手状態も照合する |
| Rejected | `Cancelled` |

`Cancelled`へ対応するGroupの未終了子孫は、既存の合意に従い包含を保持して`Cancelled`へ対応させる。`Completed`の子孫は保持する。その他の組合せや、新しい採否・目的判断が必要な不整合は対象と具体的な補正案をreportに残す。

| 旧条件 | 対応案 |
| --- | --- |
| Always | 条件未設定 |
| Manual | `exit 1`。解除は通常の`axon condition unset` |
| AtDate | 同じinstantを判定するshell条件。schema 13はUTC午前0時として解釈する |
| AfterEntity（新`Completed`を参照） | `Completed`は通常再開しないため、条件を解除した理由を記録する |
| その他のAfterEntity | 参照先の終了を新CLIで調べるshell条件。`Completed`と`Cancelled`を区別したうえで旧条件の浮上意図へ対応する |
| Command | 元文字列を保持し、旧CLI・path・schema依存を静的に確認する。対応が一意でなければ明示overrideを必要とする |

条件文字列の生成は移行専用binaryへの恒久依存を残さない。移行後に利用可能な新CLIと実行環境で完結させる。評価エラーを未成立へ落とさず、移行中の変換・構造検証ではshellを実行しない。実行検査が必要な条件は作用を確認して個別に扱う。

## fileの分岐と移行済みデータ

同じ旧storeの分岐snapshotを、それぞれ異なる新storeやEntityの起点へ変換して合流できると仮定しない。初回の運用は、現役の旧形式の変更を保全・集約して移行し、その移行結果を新形式のbranchの共通起点にする案とする。旧形式が残るbranchとの合流が必要なら、対象のbase・両側を固定して別途対応を決める。過去のGit履歴を自動書換えしない。

Axon自身のこのworktreeには、既に移行後の追加記録がある。そのsnapshotを移行前backupから作り直さず保持する。mainへの反映直前に祖先関係、両側の未commit変更、移植元、移行後の追加記録を再確認し、既存の移行成果を採用する経路を使う。既に新形式であるという形式判定だけで適用済みと断定せず、対象store・期待した記録・候補との一致を照合する。

## 検証の範囲

- file/SQLite × schema 13/14で、全情報保持と現行コアの検証を行う。
- `InProgress`、`Cancelled` Groupの子孫補正、日付、未成立AfterEntity、Manual、Commandを独立fixtureで扱う。
- 原文保全前にschema正規化していないこと、Noteの因果関係、任意metadataの数値・型を失わないことを検査する。
- 再実行でID・記録が増えないこと、source/candidate/jobの改変、途中終了、置換前後の失敗、SQLite WAL、移行後の追加記録を伴う復旧拒否を検査する。
- 対象外rootと共有binaryを変更しないこと、必要な全rootの結果が揃うまで切替完了にならないことを確認する。

## 2026-09-13の調査結果

過去の移行Issueの7rootと、`~/.ghq/src` 直下のhost/owner/repository階層にあるAxon正本を調べた。ホーム全体や任意のGit外directoryを網羅する調査ではない。調査で見つかった以下の8rootをユーザーが今回の対象として確定した。以下は移行済みという報告ではない。

| 管理root | 観測した旧backend/schema |
| --- | --- |
| dotfiles | SQLite 13 |
| external-project-a-docs | 非公開 |
| external-project-a | 非公開 |
| axon main | file 14。このworktreeは単一lifecycleへ移行済み |
| cacheexec | SQLite 13 |
| ediro | SQLite 13 |
| external-project-b | 非公開 |
| sutolog | file 13 |

SQLiteは調査時点でWAL/SHMがないことを確認し、元bytesの前後一致を確認した独立コピーを読み取った。全コピーのquick_checkは成功。この調査コピーは正式切替時のbackupの代用にしない。

外部プロジェクトで観測した個別のタスクID・状態・依存関係は非公開とする。cacheexecのCommandはcacheexecとghを呼ぶ文字列で、実行していない。sutologにはManualがあった。観測した状態・関係に基本マッピングと既知の子孫補正を当てた静的試算では、新たな状態判断を必要とする不整合は見つからなかった。全record変換と現行codecによる受理の検証は実装時に行う。

Axonのmain HEAD `b24fbee` はこのworktreeのHEAD `0ed62c6` の祖先で、mainの正本に未commit変更はなかった。この観測は将来の自動合流の保証には使わず、切替直前に再確認する。
