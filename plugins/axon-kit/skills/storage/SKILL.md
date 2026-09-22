---
name: storage
description: Axonの保存先を`axon init`で新規初期化し、求められれば追跡する運用の手順を案内し、snapshot fileを`axon storage check`で検査する。既存データの修復には使わない。
---

# 新規初期化とsnapshot検査

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。`axon init [prefix]` は管理rootに正本 `.axon/state.jsonl` だけを新規作成する。保存形式は一つで、配置や形式を選ぶoptionはない。Git内では現在のworktree rootが対象で、Git外では現在directoryが対象になる。独立試用はGit外かつ既存保存先配下ではないdirectoryで行う。

`axon init --help` と対象rootを照合し、単独commandで初期化して`axon list`で確認する。Git内で初期化した場合は、`.axon` がuntrackedであること、運用を選ぶまでは `git add -A` でcommitされること、無視する運用と追跡する運用のどちらを使うかは利用者が決め一つのrepositoryでは混ぜないことを呼出し側へ返す。`axon init`が表示する手順の案内は呼出し側が明示的に求めた場合だけ行い、ignore fileの編集、`.gitattributes`への宣言、merge driverの登録、stage・commitはこのskillから実行しない。探索順、混在時の上書き、失敗境界は [保存先と復旧](../conventions/references/storage.md) を読む。既存・破損・初期化途中・未解決indexではartifactを保全し、writerを止めて調査する。

`axon storage check SNAPSHOT` は保存形式と完全な保存snapshotを、条件実行や正本変更なしに検査する。Git indexの解決は行わず、indexがunmergedなら検証済み正本のstageが必要なことを報告する。検査結果、初期化の保存結果、残ったartifactを返す。Git driverの登録・stage・commitはこのskillから許可されない。
