---
name: declaration
description: Axonの専用export/importを依頼されたとき、非対応の境界とユーザーが求める成果物・変更範囲を整理する。
---

# declarationの境界

`axon:conventions` と `axon-kit:declaration` を使い、選択したCLIで観測した専用export/importの対応状況・version・対象を返す。

依頼された計画範囲、artifactの作成・レビュー・書戻し、保存済みEntityへの適用を区別して整理する。ユーザー所有のartifactと既存データを保持し、実行できていない変更や移行を完了と報告しない。保存・代替操作の境界はkitの契約に従う。
