---
name: storage
description: 新lifecycle SQLite保存先を新規初期化する。既存データの修復や切替には使わない。
---

# SQLiteの初期化

`axon-kit:conventions` を使い、指定された新binaryと保存先を維持する。 `init [prefix]` は新規作成専用。選んだ場所がGit内ならworktree共通の保存先を使うため、独立試用はGit外かつ既存保存先の配下ではないdirectoryで行う。

`init --help` と保存先を照合し、単独commandで初期化して結果をlistで確認する。既存・混在・破損・初期化途中ではartifactを保全し、writerを止めて調査する。init再実行を修復として使わない。このSQLite入口ではfile backendとstorage checkを実装済みと扱わない。
