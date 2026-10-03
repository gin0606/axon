---
name: declaration
description: Axonのdeclarationを`axon export`し、`axon import prepare`・`axon import check`・`axon import apply`で計画を一括登録・編集する操作契約。対象選択や適用判断は呼び出し側が与える。
---

# Declarationを取得・検査・反映する

`axon-kit:conventions` に従い、呼び出し側が指定したbinaryと管理rootを固定する。`axon export --help`、`axon import prepare|check|apply --help`、`axon docs declaration` で構文と形式を確認する。非対応ならversionと不足する操作を返し、別binaryへの切替、記録fileやheaderの直接編集、逐次の通常CLIによる代用をしない。

## 対象と入力を固定する

呼び出し側から計画の対象、declaration fileの作成・編集・書戻し、active storageへの適用の範囲を受け取る。既存fileは内容と出典を読み、元bytesとdigestを独立snapshotへ保存する。適用前には対象root、保存先、完全ID、`axon import prepare`後の入力bytesとdigest、確認した差分を保持し、結果不明の照合が終わるまで破棄しない。同じfileや保存先への未調整のwriter・editor・Git操作を直列化する。衝突か違反のある保存先では`axon import prepare|check|apply`が拒否される。`axon-kit:resolve`での修復が依頼の範囲になければ、拒否と理由を呼び出し側へ返す。途中で止まった反映の再試行は、残りの反映で消える違反では拒否されないので、そのEntityを通常操作で直さない。

- 既存計画は `axon export ID...` のstdoutを未使用のfileへ保存する。Groupは自身と終了済みを含む全子孫、Issueは単体、複数selectorは和集合。`axon export`は保存先を変更せず条件を実行しない。既存の編集fileへリダイレクトして上書きしない。
- 新規計画は `axon docs declaration --example` から作る。これは保存先を開かない。`id: null`、`base: null`、一意の `key` を持つrecordに、呼び出し側が決めた初期状態 `undecided` または `not-started` とlabelを書く。雛形の初期状態だけから採用判断を、雛形の `label: feat` だけから仕事の種類を推測しない。
- schemaは `axon-declaration/v2`。`axon-declaration/v1` のfileは拒否される。v1のbaseはv2のfingerprintと一致しないため、v1のfileの既存Entityのrecordは`label`・baseを手で足したり書き換えたりせず、別fileへ`axon export`し直して編集意図を移す。baseがnullでも、`axon import prepare`済みのidが保存先に存在するrecordは以前の`axon import apply`で保存済みなので、既存Entityとして取り直す。保存先にまだないrecordは、baseがnullで、idがnullか、`axon show ID --skip-conditions`が`no such Entity`で拒否するrecordに限る。他の失敗では判定せず止める。保存先にまだないrecordは`axon export`で取り直せないので、各recordに呼び出し側が決めたlabelを足す。schema行を`axon-declaration/v2`に書き換えてよいのは保存先にまだないrecordだけのfileに限る。取り直すrecordも含むfileは、取り直した別fileへ保存先にまだないrecordをlabel付きで移す。`axon export`が出す既存Entityのrecordはkeyがnullなので、移したrecordや編集がそれらを`{ key: … }`で参照していれば`{ id: 完全ID }`に書き換える。fieldはすべて必須。編集集合は `groups` と `issues` のrecordだけで、載っていないEntityは触らない。recordを消しても削除・`axon cancel`・所属解除・依存解除にならない。各recordはtitle、description、label（[モデルと参照](../conventions/references/model.md#情報を混同しない)の固定集合の値）、親、outgoing dependencyの完全な宣言で、親解除は `parent: null`、依存なしは `needs: []` と書く。
- 既存のid、base、lifecycle、kindは変えない。`lifecycle`は`references`を含めて保存値で、`axon show --details`・`axon list --lifecycle`が示すGroupの実効値とは異なりうる（保存値`NotStarted`で配下の仕事が始まったGroupも`not-started`のまま）。新規は`axon import prepare`後もkeyを保持する。参照は `{ id: 完全ID }` または `{ key: 別名 }`。`references` は編集集合外への参照の読み取り専用contextで、incoming edgeは含まない。必要なら `axon show ID --details --skip-conditions` で確認する。既存Entityを追加するにはselectorを広げて別fileへ`axon export`を再実行し、保全した編集意図を移す。baseを手作りしない。
- 再浮上条件・Note・履歴の取り込み、既存lifecycleの遷移、未知形式の変換には使わない。既存の条件とNoteは保持される。

## `axon import prepare`・`axon import check`・`axon import apply`

各mutationを単独で実行し、終了コードと保存結果を個別に確認する。

1. `axon import prepare FILE` は保存先を変えず、局所規則とID・kind・外部参照の存在を検査し、新規IDを確定して同じfileをcanonical rewriteする。keyと新規baseのnullを保持し、referencesを再生成する。コメントは保持しない。出力の `key -> 完全ID` を確認し、fileを読み直して内容を確認し、適用入力のbytesとdigestを固定する。`base: null`のrecordで割り当て済みIDの既存Entityが宣言の最終値と一致しなければ、`new id already exists`の競合としてfileと保存先を変更せず拒否する。`axon import check`・`axon import apply`の同じ競合も、適用済みの可能性があるため別fileへの`axon export`で取り直して編集を移す。意図して新規作成する場合だけ、その新規recordに`id: null`を明示する。未使用の割り当て済みIDと内容一致の適用済みIDは保持する。`axon import prepare`成功は既存Entityのbaseの競合や共通コアの制約を通過したことを意味しない。
2. `axon import check FILE` は全IDが確定したcanonical入力を検証し、Entityごとの作成、titleの前後（`axon list`と同じ改行・制御文字の可視化で一行表示）、descriptionの変更有無、labelの前後、親の前後、needsの増減と適用後の状況を示す。fileと保存先を変更せず、条件も実行しない。本文全文は保全した元fileとのdiffで確認する。拒否があれば原因を解決して再検査し、差分が依頼の対象・内容と一致することを呼び出し側で確認する。
3. 呼び出し側から適用権限がある場合に `axon import apply FILE` を実行する。CLIはlock取得後の入力と記録の集合で再検証し、全件を一つのlockの下で反映する。成功後、保存した記録の集合からbase・lifecycle・referencesと並びを更新し、keyを保持して同じfileを書き戻す。成功出力の `key -> 完全ID` はbase更新前に新規だったrecordの対応を示す。`axon import check`後の編集は`axon import check`を再実行し、古い結果で変更後のfileを承認済み扱いにしない。
4. 再 `axon import check FILE` で差分なしを確認し、必要な `axon show ID --details --skip-conditions`・logで完全ID、文面、label、関係、初期状態を照合する。差分があれば別writerによる変更も含めて調べ、完了と報告しない。保存先とfileそれぞれの結果、適用した対象と未解決事項を返す。

## 失敗・競合・結果不明

保存先とdeclarationは別の保存境界である。`Applied`、`Not applied`、`Result unknown` をそれぞれ読む。保存先成功後のfile更新失敗を全件未適用と扱わない。stdout失敗も未適用の根拠にしない。`axon import prepare`では保存先は未変更でもfileが適用済み・結果不明になり得る。

元processが終了してから、固定したrootと保存先で入力file、対象の現在値、記録を照合する。`axon storage check` で保存先の検査も行う。`axon import prepare`の結果不明ではfileの実際のID割当てと内容を調べ、確認できた割当てを保持してから続ける。`axon import apply`の結果不明または保存成功後の書戻し失敗では、適用時の確定IDと入力を保持したまま同じfileを`axon import check`する。fileが別途編集されていたら、その差分を保全し、固定した適用入力と混ぜずに照合する。

編集集合の全Entityが最終値（title、description、label、parent、needs、kind、lifecycle。新規は割当て済みIDで存在し宣言した初期状態）に一致する場合、`axon import apply`の再実行は保存先をno-opにしてrewriteだけを完了する。未適用を確認でき、同じ効果へ収束する場合も固定入力で再試行できる。Entityごとに最終値か`base`のどちらかに一致する途中までの反映は、同じfileの`axon import apply`で残りを反映する。どちらにも一致しないEntityや後続の別変更は競合として返す。照合不能なら結果不明を維持し、観測と入力を返す。

結果不明の解消前に`axon import prepare`を再実行して新規IDを振り直さない。baseの更新・null化や`axon export`の再実行による上書きで競合を隠さず、元artifactを保持して最新状態と意図の比較を呼び出し側へ渡す。エラーの後にfileだけを再生成して適用済みとみなさない。
