# 宣言ファイルの個人用ワークフロー

## 依頼された終端を確定する

操作前に、artifactだけを扱うかactive storageまで反映するかを依頼から確定する。

- reviewまたはcheckは原本とactive storageを変更しない。必要なら専用working fileでprepareして検査する。
- exportは指定scopeを出力してartifactを検査する。既存の出力先を上書きするのは明示されている場合だけとする。
- canonicalizeは専用working fileをprepare、checkし、commentが失われることを既知の変換として原本を置き換える。active storageは変更しない。
- 「storageへ反映する」「applyする」「Entityの計画をこの内容に更新する」という依頼はactive storageへの反映まで含む。
- 単に「declarationを更新する」だけでfileとactive storageのどちらが終端か決められない場合は確認する。

requested artifactまたはstorage状態の検証までで終了する。同じ依頼に外部作業workflowが明示されていない限り、Entityのstartや実装へ進まない。

## 内容判断と反映経路を分ける

宣言内容、新規Entityの採否、重複、分解は`axon:register`、既存Entityの判断と固定declarationの変更は`axon:triage`の方針に従う。独立した追加情報は宣言へ混ぜず、依頼範囲に応じて`axon-kit:add-note`を使う。

目的、scope、完了条件が合意済みなら、新規ID、親、dependency、包含、分解を自律して構成する。目的、scope、完了条件、採否、時期、計画の意味を新たに決める場合だけ確認する。

既存の固定declarationは、必要な理由付きUndecided化、fresh exportへの変更、apply、全文と関係の確認、供給済みDispositionへの再判断を一続きの機械的な操作として完了する。明示されたResurface conditionも宣言とは別のControl state操作として反映する。

新規AcceptedかつNotStarted、AlwaysのEntityだけを宣言から直接作成する。単一の新規Entityでparent、dependency、Resurface conditionが供給済みなら、宣言importへ寄せず`axon-kit:plan`または`axon-kit:capture`で原子的に作成する。複数Entityの一括計画でAlways以外の条件またはUndecidedが必要なら、宣言importだけでは表現できないControl stateを明示し、作成、condition、Dispositionを検証可能な別phaseとして構成する。

## 編集対象を固定する

既存Entityは`axon show --skip-command-evaluation`、関係するDeclaration Revision、判断または変更に関係するNoteを読む。導出影響を検査するphaseだけCommand条件を評価する。明示ID、`--group`、`--recursive`で選ばれた和集合だけを編集対象とし、relation endpointを自動的な編集対象とみなさない。

prepareで新規IDが割り当てられた場合はkeyとの対応を保持する。Control state変更後に最終artifactを作るときは、保持したIDを明示してfresh exportする。動的selectorを再利用して対象を意図せず増減させない。

## ユーザー所有fileを保護する

prepare、canonicalize、applyの書き換えは、出力先と同じfilesystemに作るエージェント専用working fileで行う。原本の内容とmetadataを控え、検証が終わるまで変更しない。

通常fileは内容、mode、owner、groupを保ち、ACL、拡張属性、file flagsが存在する場合だけ、それらもworking fileへ再適用して検証する。原本にdriftがないことを確認してからatomic replaceする。symlinkまたはhardlinkは自動置換せず、扱いをユーザーへ返す。

必要なmetadataを保存または検証できない場合は原本を維持し、検証済みworking fileと保存できない内容を報告する。mtimeとctimeは保持対象にしない。

## 準備、検査、反映する

`axon-kit:declaration`の手順に従い、各mutationは単独のcommandとして実行する。

1. `axon import prepare <working-file>`でcanonical YAMLを作り、編集対象、新規ID、parent、outgoing dependency、readonly境界、外部snapshotを確認する。
2. `axon import check <working-file>`で構造変更、導出状態への影響、warning、file digestを確認する。
3. artifactだけの依頼なら、原本のdriftとmetadataを再確認して必要なfileだけを置き換える。
4. active storageへapplyする場合は、直前にfile digestと原本のdriftを確認し、`axon import apply <working-file>`を単独で実行する。
5. apply結果をcheck結果と比較し、再度checkして差分がないことを確認する。
6. 変更Entity、新しいRevision、関係、Control state、関連frontierを検証する。全履歴と全Noteは、その変更や判断に関係する場合だけ再読する。

各phaseは一つのtransactionではない。途中結果を観測して次へ進み、自動rollbackで履歴を隠さない。

## 競合と部分完了を解決する

stale fileをprepareで上書きしない。stale file、現行storageのfresh export、意図した最終値を比較し、競合しない変更はfresh exportへ自律して載せ直す。同じfieldまたはrelationが双方で変わっていても、合意済みの最終値が明確ならその値へ収束させる。

目的、scope、完了条件、Disposition、時期、計画の意味を変える必要がある競合はユーザーへ返す。導出状態への影響が変わっても、合意済み計画内の変化なら影響を再検証して続行できる。

storage適用後のfile rewrite失敗では同じapply fileを保持し、現在storageと照合して公式kitが許す同一fileの再実行を行う。command出力を失った場合もworking fileを変更せず、process終了とstorageを照合する。結果不明、重複適用の可能性、rollback、補償操作、別の最終状態が必要な場合は停止する。

途中失敗、競合調査、結果不明、復旧に必要なfileは削除しない。path、staleか再実行可能か、storageへ反映済みのphase、次に有効な操作を報告する。成功後は、永続成果物として残す合意がないエージェント所有一時fileだけを削除する。
