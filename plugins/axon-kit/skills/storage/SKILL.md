---
name: storage
description: AxonのSQLite/file保存先を新規初期化し、file snapshotを検査する。既存データの修復やbackend切替には使わない。
---

# 新規初期化とsnapshot検査

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。`init [prefix]` は既定SQLite、`--backend file` は現在worktreeのfileを新規作成する。SQLiteはGit worktree間で共有する。独立試用はGit外かつ既存保存先配下ではないdirectoryで行う。

`init --help` と保存先・backendを照合し、単独commandで初期化してlistで確認する。file initのignore/attributes補完と失敗境界は [保存先と復旧](../conventions/references/storage.md) を読む。既存・混在・破損・初期化途中・未解決indexではartifactを保全し、writerを止めて調査する。init再実行を修復として使わない。

`storage check SNAPSHOT` はfile形式と完全な保存snapshotを、条件実行や正本変更なしに検査する。Git indexの解決やbackend切替は行わない。検査結果、初期化の保存結果、残ったartifactを返す。Git driverの登録・stage・commitはこのskillから許可されない。
