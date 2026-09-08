---
name: conventions
description: Axon の共通の状態・情報モデルと CLI 操作の安全性 contract を提供する。他の axon-kit skill と利用側 workflow の基盤として使い、協業方針、作業選択、実装、commit は規定しない。
---

# Axon 操作 contract

この skill をすべての Axon capability の共通基盤として使う。Axon データの意味と、履歴を失わず、非 idempotent な作用を重複させず、不確かな storage 結果を成功と誤認せずに、許可された操作を実行する方法を定める。

## 層を分離する

- Axon は Entity の状態、関係、導出事実、情報の所有範囲を定める。人と agent のどちらが作業を選ぶか、Disposition を判断するか、結果を review するかは決めない。
- 呼び出し側の依頼または workflow が、対象、意図する作用、その作用への権限を与える。`axon-kit` capability はその作用だけを実行して制御を返し、後続 phase への許可を推測しない。
- repository の規則と host の権限は引き続き有効である。workflow はこの kit を使って外部への権限を拡張できない。
- Axon の状態同期は、実装、test、review、commit のふるまいを規定しない。

## 該当する contract を読む

Entity を解釈する前、declaration または Control state を変更する前、関係や導出事実について判断する前に、[モデル contract](references/model.md)を読む。

Axon storage または storage 関連 artifact を変更する command の前に、[mutation contract](references/mutations.md)を読む。過去の mutation の結果が不確かでない限り、read-only な調査ではこの参照は不要である。

新しい Issue または Group を作成するときは、[作成 contract](references/creation.md)も読む。

## インストール済み CLI の contract を優先する

command 構文または入力 contract が不確かなときは、`axon help` または該当する `axon <command> --help` を使う。インストール済み CLI、その documentation、この指示の間に不一致がある場合、別の書き込み経路を考案して埋め合わせない。呼び出し側がどの version または artifact を正とするか決められるよう、不一致を報告する。

## capability の結果を返す

対象 ID または storage artifact、要求された作用、観測した最終状態、関係する frontier または関係への影響を報告する。mutation では、storage 結果を適用済み、未適用、部分適用、不明のいずれかに分類する。未完了の phase を一般的な成功報告で隠さない。
