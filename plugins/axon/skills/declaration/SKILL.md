---
name: declaration
description: Axonの計画のdeclaration作成・レビュー・一括反映について、依頼の範囲と判断権限を確認し、`axon export`・`axon import prepare`・`axon import check`・`axon import apply`を進める。
---

# 計画の一括登録・編集を進める

`axon:conventions` と `axon-kit:declaration` を使う。形式・保存・再試行はkitと選択したCLIの `axon docs declaration` に従い、ここでは何を変えるかと適用の判断を扱う。

## 依頼の成果と境界を固定する

計画の対象、declaration fileへの反映、active storageへの反映を区別する。file作成・レビューだけの依頼を保存済みEntityの変更へ広げない。`axon import prepare`もfileを置き換えるため、レビューだけの依頼では元fileを保持した作業copyで検査する。保存先への適用が未承認なら、fileと検査結果を具体的に揃えて変更案を提示し、その判断が必要な理由を説明する。明示された適用の直前で同じ権限を再確認しない。

ユーザー所有の既存fileは内容と出典を調べてから扱い、都合のよい`axon export`で上書きしない。固定したsnapshot・候補・digestと取得後の編集を保持する。エージェント所有の作業fileも結果不明の照合が終わるまで保持する。

新規登録では未終了Entityの重複を `axon:conventions` に従って確認する。目的・scope・完了条件から一意に必要な構成整理は自律して行う。新しい目的、採否、独立した完了単位、代替前提の選択はユーザーへ返す。雛形の `not-started` や一括編集という形式を採用権限とみなさない。

## 編集と検査を読む

保存済みの対象・関係・必要なlogとNoteを確認し、kitの手順で取得・編集・`axon import prepare`・`axon import check`する。対象を広げる必要があれば、それが依頼のscope内か判断する。既存Entityは別fileへの`axon export`の再実行で追加し、元の編集意図を保持する。

Groupから外すには対象recordの `parent: null`、依存を外すにはそのdependentの `needs` を編集する。編集集合の宣言は文面・親・outgoing dependencyの完全な最終値としてレビューする。

`axon import prepare`と`axon import apply`の出力で新規recordの `key -> 完全ID` を確認する。`axon import check`はtitleの前後を`axon list`と同じ可視化で一行表示し、descriptionは変更有無だけを示す。`axon import check`の構造差分だけで本文レビューを済ませず、fileの全文差分と目的・完了条件を照合する。外部参照のtitle・lifecycleは最新値と一致する保証がなく、incoming edgeも含まないため、判断に必要な関係先は `axon show ID --details --skip-conditions` で読む。

## 反映と競合を扱う

差分が合意済みの対象・内容に収まり、`axon import check`が成功し、保存先への適用権限があればkitの`axon import apply`と保存後の照合まで進める。fileだけの成果を求められた場合はfileと検査結果を返す。

競合では元artifactと最新の保存情報を比較する。同じ効果へ解決できる訂正は自律できるが、意味上の選択が変わる場合は具体的な差と案を返す。`axon export`の再実行は別fileへ行い、ID・base・digestの再生成で競合を隠さない。結果不明はkitの固定入力による照合・再試行に従う。

保存先への適用とfile書戻しの結果を分けて報告する。複数段階の前段が適用済みならその成果を保持し、後段失敗後の状態を説明する。自動rollbackや補償判断を発明せず、未実行や結果不明を完了扱いにしない。実装・commit・`axon start`・`axon complete`・保存先の切替へ権限を広げない。
