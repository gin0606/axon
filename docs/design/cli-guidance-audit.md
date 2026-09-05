# CLI 操作案内の棚卸し

2026-09-05 の `src/main.rs`、`src/db.rs`、`src/declaration.rs` と CLI 契約を対象とした
ソース調査。全コマンドを成功・失敗・空結果・help の観点で確認した。
この表は調査時点の判断であり、実装進捗の一覧ではない。
現在の方針は [CLI 契約](../reference/cli.md#操作を理解するための案内) を参照する。

## 共通の観点

成功確認の対象ID、追加・設定・遷移の区別、生成出力の分離は維持する。
DB不一致には番号だけでなく互換buildと復旧の制限を案内する必要がある。
ID不明・曖昧、kind不一致、固定宣言、成立条件違反には確認先を付けられる。
I/OやSQLiteの一般エラーだけからDB未変更や再実行の安全性を断定してはいけない。
診断のためにDBを開き直したりCommand条件を実行したりしない。

## コマンドごとの判断

| コマンド | 成功・空結果の評価 | 失敗・helpで説明すべき点 |
| --- | --- | --- |
| plan / capture | IDと採否の確認を維持 | 毎回新規作成、Acceptedは宣言固定、parentの制約 |
| group plan / capture | kindを含む作成確認を維持 | 明示的な計画単位、作成だけでは配下を開かない |
| ready | 空は完了や全件不存在を意味しない | active scope、Accepted、条件成立、依存解決という抽出条件とlistへの経路 |
| triage | 空は全体に判断事項がないという意味ではない | active frontierと全件listの違い。採否を選ばない |
| start | claim確認を維持 | 失敗対象のshowと成立条件。親のstartやacceptを無条件に指示しない |
| done | Ended確認を維持 | InProgressとGroup全子孫terminal。未完了子孫のRejectを勧めない |
| release | claim解除確認を維持 | NotStartedへ戻る効果、Groupの進行中子孫制約。古いclaimを自動判定しない |
| show | 保存情報と導出状態、Group subtreeを維持 | ID解決の確認先。Command条件失敗で表示できない場合の条件訂正経路 |
| list | 全状態を表示する性質を維持 | kind filterとCommand評価。空結果から採用や新規作成を勧めない |
| claims | 保存されたclaim、空案内を維持 | 経過時間はreleaseの根拠を代行しない |
| log | 判断履歴と空案内を維持 | Progress履歴とは別であることとID解決 |
| note add | Entity-local番号の保存確認を維持 | 追記専用、反復は重複、非空本文、declarationを変更しない |
| note list / show | 索引と本文を維持 | 存在しない番号にはそのEntityのlistを案内 |
| revision list / show / diff | 索引、全文、差分を維持 | Entity-local番号、判断時の固定記録であること。存在しない番号の確認先 |
| write | 実変更とno-opの区別を維持 | 固定解除・編集・全文確認・再判断の効果。Noteとは用途が異なる |
| group set / unset | 包含変更の確認を維持 | 所有者、固定宣言、Ended scope、循環。どこへ移すかは選ばない |
| dep add / rm | 関係変更の確認を維持 | 依存元が所有、Rejectedは前提喪失、循環、辺除去の効果 |
| decide accept / reject / undecide | Disposition確認を維持 | 宣言固定・Revision、Undecidedでの編集、Progressを変えない。再採用を決めない |
| when at / after | 条件保存の確認を維持 | 日付形式、AfterEntityはEndedまたはRejectedで成立、dependencyとの違い |
| when manual / command / clear | 条件保存の確認を維持 | Group gateへの影響、clearはAlways、Commandの終了コードと評価タイミング |
| export | YAML以外をstdoutへ混ぜない | selectorの範囲、DB互換性が必要。完全なDBバックアップではない |
| import prepare | file更新確認を維持 | fileを変更しDBは変えない、新規ID割当。最小入力例への経路に改善余地 |
| import check | 構造・導出差分を維持 | checkは適用しない、競合や固定宣言の診断。Command評価失敗を区別 |
| import apply | 適用結果とfile更新確認を維持 | DB失敗とcommit後のfile失敗、再試行条件を区別する診断が必要 |
| init | 保存先確認を維持 | 既存DBのresetやupgradeではない。管理rootとworktree共有 |
| completion | scriptのみを維持 | shell選択は既存helpで説明できる。作業案内の追加不要 |
| docs | DB不要の概念説明を維持 | 障害時にも利用可能な復旧説明の入口 |
| help / 引数なし / --help | rootの分類と個別helpを維持 | 詳細な意味は個別helpとdocsへ。構文の生成元はClapのまま |

## 説明だけでは解消しない不足

DBの旧版から新版へデータを保持して移す公開手段と、schema番号から互換buildを
特定する手段がない。最新版への更新だけで全不一致が直るとは説明できない。
移行対象の版、情報保存の範囲、backupと失敗時の復旧を含めた設計が必要である。
宣言applyの適用結果診断と入力例の到達性は既存の課題とも重なる。

今回の棚卸しは、状態モデルの変更や全エラーパターンの実機再現を含まない。
