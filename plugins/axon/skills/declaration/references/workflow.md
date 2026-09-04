# 宣言ファイルの個人用ワークフロー

## 内容判断と反映経路を分ける

宣言内容、新規Entityの採否、重複、分解は`axon:register`、既存Entityの判断と固定declarationの変更は`axon:triage`で確定する。独立した追加情報は宣言へ混ぜず、依頼範囲に応じて`axon-kit:add-note`を使う。artifactだけの依頼ではlive DBを変更しない。

固定declarationの実変更は、理由付きでUndecidedへ戻し、fresh exportからapplyし、全文と関係を確認した後に最終Dispositionを別の判断として反映する。Resurface conditionも宣言とは別に変更する。各phaseは一つのtransactionではないため、途中失敗では自動で巻き戻さず、反映済みと未反映を分けて報告する。

新規AcceptedかつNotStarted、AlwaysのEntityだけを宣言から直接作成する。それ以外の初期状態はcaptureで準備し、Undecidedの間にdeclarationを反映してから、Resurface conditionと最終Dispositionを別々に適用する。

## 編集対象と履歴を確認する

既存Entityは`axon show`の全出力、全Declaration Revision、表示された全Noteを読む。明示ID、`--group`、`--recursive`の和集合だけを編集対象とし、relation endpointが自動的に編集対象になると解釈しない。

最終判断を伴う場合は、prepare後に割り当てられたIDとkeyの対応を控える。Undecidedでapplyした中間fileを最終artifactにせず、最終判断後に控えたIDを明示してfresh exportし、差分なしのcheckを通したものを最終版にする。動的selectorを再利用して対象を増減させない。

## ユーザー所有fileを保護する

export、prepare、applyは、出力先と同じfilesystemに作ったエージェント専用working fileで行う。原本の内容とmetadataを控え、原本は全検証が終わるまで変更しない。symlinkまたはhardlinkは自動置換せず、扱いをユーザーへ確認する。

通常fileではmode、ownerとgroup、ACL、全xattr、macOSまたはBSDのfile flagsを保存する。canonical rewrite後にworking fileへ再適用して検証し、原本にdriftがないことを確認してから一度だけatomic replaceする。一項目でも保存、再適用、検証できなければ、失われるmetadataを示して許可を得るか、working fileを保持して停止する。mtimeとctimeは保持対象にしない。

## 準備、検査、反映

1. `axon import prepare <working-file>`を単独で実行し、commentが失われることを前提にcanonical YAMLを確認する。
2. 編集対象、割り当てID、parent、outgoing dependency、readonly境界、外部snapshotを確認する。
3. `axon import check <working-file>`を実行し、構造変更、導出状態への影響、warning、file digestを保持する。
4. canonicalizeだけなら原本のdriftとmetadataを再確認し、working fileで置換して`DB applied: no`として終了する。
5. applyする場合は直前にfile digestと原本のdriftを再確認し、`axon import apply <working-file>`をDB mutationとして単独実行する。
6. apply出力をcheck結果と比較し、再度checkして差分なしを確認する。変更した全Entity、全Revision、既存Note、関連frontierへの影響を確認する。

## 競合と結果不明

stale fileをprepareで上書きしない。stale file、現行DBのfresh export、意図した最終値を三者比較し、競合しない変更だけをfresh exportへ載せ直す。同じfieldまたはrelationが双方で変わった場合や導出状態への影響が変わる場合は、ユーザーの判断を得てから再度checkする。

DB commit後のfile rewrite失敗では同じapply fileを保持し、公式kitの復旧契約に従って同じapplyを再実行する。command出力を失って結果不明の場合もworking fileを変更せず、process終了と現在DBを照合してから、許可された同一fileの再実行だけを行う。

途中失敗、競合調査、結果不明、復旧に必要なfileは削除しない。保持するpath、staleか再実行可能か、DBへ反映済みのphase、次に有効なcommandを報告する。成功後は、永続成果物として残す合意がないエージェント所有一時fileだけを削除する。
