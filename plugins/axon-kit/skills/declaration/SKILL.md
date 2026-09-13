---
name: declaration
description: Axonの専用export/importを依頼されたとき、非対応の境界と依頼された対象範囲を整理する。実際の一括適用には使わない。
---

# declarationの境界

`axon-kit:conventions` に従って対象環境のbinaryとrootを固定する。専用export/importは未提供。選択したCLIのhelpで対応状況を確認し、非対応なら観測したversionと利用できない操作を返す。非対応操作を別のbinaryや保存ファイルの直接編集で代替しない。

既存データとdeclaration artifactを保全し、確認できたschema・backend・対象範囲、要求された読出しや変更を呼び出し側へ返す。artifactへの書戻しと保存済みEntityへの適用を区別し、一括操作を無断の逐次CLI操作で代用しない。新規登録だけで履歴・構造を含む移行完了とはしない。通常操作の説明には選択したCLIの `docs` を使う。変換器の実装、backend切替、既存データの削除はこのskillの範囲外。
