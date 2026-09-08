---
name: merge
description: Axon file-backend snapshot の three-way merge を準備、解決、検証、公開する。axon merge workspace と file-backend conflict に使い、declaration YAML の merge や通常の Git merge/rebase 完了には使わない。
---

# Axon file snapshot を安全に merge する

`axon-kit:conventions` を使い、mutation contract を読む。この capability は Axon の merge workspace と candidate 公開を所有する。周辺の Git merge、staging、commit、rebase、conflict policy は所有しない。

prepare、resolution、check、apply、Git driver の復旧、不確かな結果を扱う場合は、[merge workflow](references/workflow.md)を最後まで読む。

## Git integration を分離する

Driver 登録には install 後の通常の Git config を使い、`axon merge setup` は提供されない。File init が attribute と ignore rule を用意する。merge の依頼だけでは Git 設定の変更を許可しない。別途許可された場合、repository に `merge.axon.driver` を `axon merge driver %O %A %B`、`merge.axon.recursive` を `binary` として登録する。Axon は PATH 上に必要で、clone ごとに登録が必要である。Global 設定は任意であり必須ではない。

`axon merge driver` を手動 merge command として呼び出さない。その `%O`、`%A`、`%B` argument と driver の temporary output path は Git が所有する。

## 結果を返す

operation mode、workspace と output path、input と candidate の digest、未解決 conflict または適用済み repair、検証と storage 結果の分類、変更した artifact、この capability 外に残る正確な Git 作業を返す。
