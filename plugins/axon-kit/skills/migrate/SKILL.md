---
name: migrate
description: 新lifecycle版で未提供のmigrate操作を依頼されたとき、利用可能な境界を説明する。
---

# migrateの境界

単一lifecycle版ではこの操作は未提供。まず共通conventionsに従って対象環境のbinaryを選ぶ。選択したbinaryのhelpを確認し、未提供ならその事実を返す。旧CLIの手順を新保存先へ実行したり、保存ファイルを直接編集して補ったりしない。

既存データやdeclaration artifactは保全し、元schema・backend・対象範囲と要求した変換を呼び出し側へ返す。新規登録だけで履歴・構造を含む移行完了を代用しない。新規運用の説明は選択したCLIの `docs`、初期化は `axon-kit:storage` を使う。変換器の実装、backend切替、旧dataの削除はこのskillから許可されない。
