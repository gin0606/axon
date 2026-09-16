---
name: migrate
description: Axonのschema移行やbackend変換を依頼されたとき、非対応の境界と移行元・移行先を整理する。変換の実行には使わない。
---

# migrateの境界

`axon-kit:conventions` に従って対象環境のbinaryとrootを固定する。専用migrate操作は未提供。選択したCLIのhelpで対応状況を確認し、非対応なら観測したversionと利用できない操作を返す。非対応操作を別のbinaryや保存ファイルの直接編集で代替しない。

移行元のデータと既存artifactを保全し、確認できたschema・backend・対象範囲、要求された移行先・変換内容・保持すべき情報を呼び出し側へ返す。新規登録だけで履歴・構造を含む移行完了とはしない。通常操作の説明には選択したCLIの `axon docs` を使い、空の保存先の初期化が依頼されていれば `axon-kit:storage` へ渡す。変換器の実装、backend切替、移行元の削除はこのskillの範囲外。
