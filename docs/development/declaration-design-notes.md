# 一括declarationの再設計資料

2026-09-13に、`axon-kit:declaration` と `axon:declaration` の設計用referenceを開発資料として統合した。2026-09-15に一括declarationの契約を [lifecycle spec](../../spec/lifecycle_proposal.md#計画全体の取得と一括編集) の「計画全体の取得と一括編集」へ確定した。契約はspecを正とし、この資料は設計時に検討した目的・保存境界、個人workflowの判断境界、旧契約との対応、採らなかった案を残す。

## 目的と保存境界

一括編集の目的は、新しく大きめの計画を一括で登録することと、登録後の計画全体をcontextに入れて役割・完了条件・依存の不整合を見つけ、レビュー可能なartifactとして扱い、修正をまとめて反映することである。設計では以下を検討し、specへ反映した。

- 取得する計画境界、包含、外部参照を明確にし、一覧の非表示を削除指示と取り違えない。
- 完全な宣言と部分変更を区別する。省略が維持・解除・削除のどれかを曖昧にしない。
- 変更前後の計画全体と差分を、保存前にreview/checkできるようにする。
- 取得後に前提が変わった場合は競合として扱い、意図を再照合する。IDやdigestの再生成で競合を隠さない。
- 全体の包含・dependency・終了構成を検査し、部分的な採用で不整合を残さない。
- 新規IDを固定して再実行時の重複作成を防ぐ。コマンド名やfile形式は目的と安全性を確定してから決めた。
- DB/fileへの適用とdeclaration artifactへの書戻しを別の保存境界として扱い、部分適用・結果不明を区別する。
- 入力・backup・途中artifactを保全し、未適用が立証できる操作だけを安全に再試行する。

既存artifactを受け取る場合は、元schema、内容、要求された変換と必要な保存結果を把握する。機能が非対応であれば、その事実と入力を呼び出し側へ返し、無断の逐次CLI操作や保存ファイルの直接編集で代用しない。

## 個人workflowの判断境界

依頼が指定する計画範囲、declaration fileへの反映、active storageへの反映を区別する。file作成・レビューだけの依頼から保存済みEntityの変更へ広げない。明示された適用の直前で同じ権限を再確認しない。

ユーザー所有の既存fileは内容と出典を調べてから扱い、都合のよいexportで上書きしない。呼び出し側が固定したsnapshot・候補・digestを保全し、取得後の編集をartifactの再生成で消さない。エージェント所有の作業fileでも、結果不明の照合に必要な間は保持する。

合意済みの目的・scope・完了条件から一意に決まる構成整理は自律できる。新しい目的、採否、独立した完了単位、代替前提の選択はユーザーの判断に返す。一括編集という形式は、それらの判断権限を与えない。

本文編集・lifecycle・構造の責務を守る。複数段階になる場合は前段の適用結果を保持し、後段失敗後の状態を説明する。自動rollbackや補償判断を発明しない。

競合では新しい保存情報と元の意図を比較し、同じ効果へ解決できる訂正だけを進める。意味上の選択が変わる場合は具体的な差と案を返す。必要な入力や判断が揃うまではartifactを保持し、未対応機能を実行可能と報告しない。

## 旧契約との対応

[旧契約](../reference/declaration-file.md)は旧三軸モデル向けの形式で、比較材料として残す。旧実装 `archive/three-axis/src/declaration.rs` は旧Store・Revision型に結合しており、コードは流用しない。

流用した部分: 編集集合と省略の意味（載せないEntityは触らない）、childとdependentによる関係の所有、`key` によるfile内別名と `prepare` でのID確定、`base` fingerprintによる競合検知とtokenのbyte encoding、strict parserの規則、`prepare` → `check` → `apply` → 再 `check` の流れ、保存成功後のfile更新失敗に対する再試行判定、拒否入力の分類。

変えた部分:

- `observed`（progress・claim・disposition・resurface）を廃止し、`lifecycle` 一つを読み取り専用で置く。現行モデルにProgress・Disposition・claim・Revisionはない。
- Dispositionによる編集固定（Undecidedだけ編集可）を廃止し、共通コアの規則（Completed・Cancelledの文面固定、Completedのdependency固定、終了Groupの構成固定、親は終了していないGroup）に従う。
- 新規Entityの初期状態をaccepted/not_started/alwaysに固定せず、`undecided` か `not-started` を宣言する。`capture` と `plan` の使い分けを写す。
- 関係の中央集約（`relations.editable/readonly`）をやめ、各recordの `parent`・`needs` に埋め込む。旧契約は一つの関係をfile内に一度だけ現し、外部からのincoming relationを同じfileで確認しつつ編集範囲を広げないことを理由に集約していた。今回はレビューで各Issueの依存先を一目で読めることを優先し、incoming edgeの表示は初回では見送った。外部Entityの読み取り専用snapshotである `references` はrootに残す。
- `references` の値の一致を要求しない。存在と共通コアの制約だけを検査する。旧契約の全拒否は大きな計画ほど無関係な変更で止まる。
- descriptionのnull正規化とtitleのtrimをやめ、現行CLI契約と同じく保存値をそのまま扱う。descriptionは常にstringとし、空文字を本文なしとする。
- selectorの「直下だけ」と `--recursive` の二段構えをやめ、Groupは常に全子孫とする。
- Entityの並びをID順から作成日時順（未適用の新規はkey順）に変え、`list` と同じ並び方の規則にする。
- schema labelを `axon-plan/v3` から `axon-declaration/v1` に変え、旧形式を変換せずに拒否する。
- 再浮上条件をdeclarationから外す。旧契約は `observed.resurface` として読み取り専用で載せていた。

## 採らなかった案

- 全体取得と一括applyを別の契約に分ける案。exportはapplyの前提でもあり、同じ形式を二度定義することになるため、一つの契約にして実装計画でexportを先行させる。
- prepareをapplyまたはcheckに吸収する案。applyに吸収すると結果不明時にIDが分からず、checkに吸収すると検証のつもりでfileが書き換わる。
- Noteをdeclarationに読み取り専用で同梱する案、および配下の全Noteをまとめて読む入口を同時に作る案。同梱するとNoteの追加による競合、file内Noteの書き換え、apply後のrewriteの扱いを決める必要が増える。今回の用途は計画の登録と修正であり、Noteは含めない。まとめて読む入口の再検討は引き続き保留する。
- 再浮上条件を編集対象または読み取り専用として載せる案。考慮事項を増やさないため今回は扱わず、applyは現在の条件を保持する。
- JSONまたは独自のMarkdown形式。JSONは複数段落のdescriptionの改行を潰し、独自形式はparserとcanonical化を自作することになる。
- Groupの下に子を入れ子で書く形式。親を入れ子と `parent` の二通りで表せてcanonical形が一意でなくなり、subtree外に親を持つEntityの扱いも複雑になる。
- 素の文字列による参照。keyの文字種はIDの形と重なり得るため、見分け規則を増やすより明示する。
- 元の値へ戻った変更を競合として検出する案。値が同じなら編集の意図はそのまま適用できる。
- declarationによるlifecycle遷移。accept・start・doneの前提検査をdeclaration用に二重化することになり、計画workflowがstart・doneへ広げない境界とも一致しない。
- 保存先全体を一括で取得するselector。top-levelを全部渡せば同じ結果が得られ、Completedが積み上がるとartifactとして扱いにくい。

### 2026-09-16の比較評価

現行実装（commit `d2caf1d`）と、同じ契約目的を別の設計で実装した案を比較し、現行実装を土台にすることにした。上記のNote・再浮上条件・外部参照の扱いも含め、取り込まなかった設計と理由を以下に残す。

- 全Noteと全直接関係をcanonical JSONのbase64として `base` に埋め込み、取得時の観測から前提一致と結果一致を独立に判定する方式。同じGroupをexportした2026-09-16の計測では、現行の10.5KBに対して約186KB（約18倍）になった。Note本文がYAML側と `base` 側に二重に入り、計画をcontextとして読み編集するartifactが大きくなるため採らなかった。この値は当該Groupの比較結果であり、一般的な倍率ではない。
- Note・condition・外部からのincoming edgeを同梱し、編集集合と外部参照へのNote追記や外部参照の変化も競合として止める方式。エージェントがNoteを頻繁に追加する運用では、計画の編集意図と無関係な変化による停止が増え、復旧にfresh exportへの手作業の移し替えを要する。Noteと再浮上条件を含めず、外部参照は存在と共通コアの制約を検査する既存の判断を維持した。
- 共通コアに一括編集専用の候補構築と差分guardを置く方式。specはapplyを通常操作の列と定めており、専用guardは `write`・`set_parent`・`add_dependency`・`create` のguardを二重化する。比較時の適用順序とguardの突き合わせでは、有効な最終状態を誤拒否する経路は見つかっておらず、専用の検査経路を増やす根拠がなかった。この調査結果を全経路の証明とは扱わない。
- declarationに `store` 識別子を持たせる方式。現行は既存recordのIDが適用先に存在しなければ拒否する。新規Entityだけで外部参照を持たないdeclarationはstoreをまたいで使えるため、artifactを一つのstoreに結び付ける制約は加えなかった。これは、同じIDを持つstore同士を識別する保証ではない。
- Groupの直下だけを選ぶselector。計画全体をcontextとして読む目的に合わせ、Groupは常に全子孫を選ぶ判断を維持した。直下と再帰で取得境界を切り替える操作は増やさない。
- JSON path形式の診断。現行のYAML入力に対する診断は行・列と抜粋で位置を示すため、別の位置表現は取り込まなかった。

## 比較資料

旧実装と個人workflowの比較根拠はcommit `367030ce` にある。再実行時の新規ID固定は旧prepareが担っていた目的として保持した。旧モデル固有のRevision・採否変更による編集解放・claim操作は、現行モデルへ導入する根拠にしない。
