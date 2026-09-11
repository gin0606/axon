---
name: register
description: 重複と計画の完成度を調べ、新しい懸念または採用済み計画を登録する。
---

# 登録

`axon:conventions` と `axon-kit:capture / plan` を使う。listと関連show・Noteで重複を確認し、未判断の懸念はcapture、採用済みで目的・完了条件が確定した計画はplanへ渡す。kind、親、依存は依頼から確定できるものだけにする。新しい採用判断は創作しない。既存Entityの変更はtriageへ渡す。保存IDと本文・状態・関係を照合する。
