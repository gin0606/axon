---
name: add-note
description: 既存 Issue または Group の declaration や Control state を変えず、補足情報を immutable な Note として追記する。要求された Note、確定した結果、訂正、handoff に使い、declaration 編集、状態変更、新規 Entity には使わない。
---

# Axon Note を 1 件追記する

`axon-kit:conventions` を使う。Note を追加する前に、そのモデルと mutation contract を読む。

## 補足情報だけを受け入れる

情報を分類するために必要な対象 context を読む。Note には調査・実装結果、後から判明した制約、訂正、有用な handoff を記録できる。Note によって Entity の定義や現在の Control state を変更してはならない。

- title、description、parent、outgoing dependency の変更は `axon-kit:triage` に渡す。
- Progress、Disposition、Resurface condition、claim の変更は、その状態を所有する capability に渡す。
- 状態変更の reason は typed history に置き、Note として重複させない。
- 古い Note の訂正は、元の Note を stable ID で特定する新しい Note の追記で行う。元の Note を編集・削除しない。

呼び出し側の依頼または workflow が対象と Note 内容を与える。この capability に確定した事実の表現を整えるよう求めてもよいが、capability は判断を創作せず、対象も選ばない。対象、情報 class、表現する判断が未解決の場合は `input required` を返す。

## 追記入力を固定する

正確な Note 本文を固定する。最初の追記試行前に、追記と同じ environment と working directory で `axon actor` を実行し、現在の actor label を観測する。これは最初の Note より前でも機能し、Axon storage を開かず record も作成しない。観測後に environment または directory が変わると次の操作の actor も変わりうる。過去の Note や claim は現在の label を保証しない。`-F` または stdin を使う場合は、mutation contract に従って byte 列を保存し digest を記録する。

actor は表示・調査用の label であり、一意な session ID、認証、lock ではない。複数の Codex または他の agent session が共有しうる。actor を知るためだけに Note を作成したり作業を開始したりしない。

追記の直前に `axon note list <id>` を読み、件数と stable ID の完全な集合を記録する。Note がなければ空集合を使う。

## 1 回だけ追記して検証する

`axon note add <id> -m <body>` または同等の `-F <snapshot>` を、単独の mutation として実行する。

成功を観測したら、返された Note ID を記録し、追記を繰り返さない。`axon note show <id> <note-id>` を読み、本文、actor、timestamp を検証する。

追記が明確に失敗した場合、原因を解消せず再試行しない。結果が不明な場合は次を行う。

1. 元の process が終了したことを確認する。
2. 追記前に記録した集合になかった Note ID をすべて列挙する。ID 順は作成順ではない。
3. 各候補を読み、固定した本文と actor の両方を比較する。
4. actor の一致だけでは所有を立証できない。固定した本文と actor の両方に一致する新しい Note が 1 件だけ見つかった場合でも、証拠が追記結果を立証するときだけ成功とみなす。複数件が一致する場合や、証拠だけでは結論できない場合は `storage result: unknown` とし、無関係な concurrent Note を保存する。
5. 後続の Note をすべて観測し、一致が 1 件もない場合だけ、同じ固定済み追記を 1 回再試行できる。その試行も同じ規則で照合し、それ以上再試行しない。

Note ID と要約を返し、concurrent Note は別途報告する。storage 結果が不明のままなら入力 snapshot を保持する。
