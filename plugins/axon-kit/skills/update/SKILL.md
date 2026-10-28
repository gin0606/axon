---
name: update
description: Axonの対象と変更内容が確定したEntityに、状態・種類・本文・label・包含・依存・再浮上条件の更新を適用・検証する操作契約。変更前後のいずれかで同じGroupに含まれる（Group自身と全子孫）、またはdependencyでつながる複数Entityのtitle・description・label・親・dependencyの一括変更は通常`axon-kit:declaration`の対象で、どちらを使うかは呼び出し側が決める。採用判断や変更内容の策定は行わない。
---

# 既存Entityを更新する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。渡されたreasonの付与と保存確認は [保存操作](../conventions/references/mutations.md) に従う。 `axon show ID --details --skip-conditions`と関連するlog・Note、関係先を読む。与えられた判断に対応する `axon accept|withdraw|cancel|reconsider|reopen`、本文の `axon write`、labelの `axon label set A VALUE`、包含の `axon parent set A --parent G / axon parent unset A`、依存の `axon dep add|rm A --needs B`、条件の `axon condition set A --command ... / axon condition unset A`、種類の `axon convert A --kind issue|group`（前提は [作成と照合](../conventions/references/creation.md)）を使う。

未終了の本文は`axon write`、labelは`axon label set`で直接編集し、terminal後の訂正はNoteで行うため、本文やlabelの変更のためだけにlifecycleを往復させない。`axon condition`はシェル文字列を保存し、実行環境・副作用は呼び出し側の意図と照合する。`axon condition set --command`は渡した文字列を許可し、`axon condition allow ID --digest DIGEST`は条件を許可する。どちらも呼び出し側が与えた文字列・digestだけを使い、意味・表示・確かめ方は[モデルと参照](../conventions/references/model.md#情報を混同しない)の条件の許可の段落に従う。条件を評価する`axon proposals`/`axon tasks`と既定の`axon show`を使う場合だけ実行契約をhelpで確認する。

`axon reopen`は`Completed`を`NotStarted`へ戻す判断が与えられた場合だけ使い、全祖先が採用済みで、`Completed`の依存元がないことを要する。拒否されたら、祖先の採用や終了した祖先の`Reopen`・`Reconsider`、`Completed`の依存元の`Reopen`を行うかを呼び出し側へ返し、自動で広げない。`Reopen`は子や依存元のlifecycleを変えない。`Reopen`を含むlifecycle操作は対象と祖先Groupの実効lifecycleと状況を変えうるため、祖先も波及として照合する。Groupの`axon withdraw`は実効値が`NotStarted`のときだけ、着手・完了した子孫を持つGroupの`axon accept`は全祖先が採用済みのときだけ受け付けられる。操作前のGroupの状態と、成功出力に出ない対象・祖先Groupの実効lifecycleは、`axon show ID --details --skip-conditions`の実効値と保存値の両方で照合する。

作用を一つずつ検証し、予定にない子の終了や別対象の採用へ進まない。`axon cancel`など終了状態に影響する変更は関係先を調査する。部分適用や結果不明は明示して返す。
