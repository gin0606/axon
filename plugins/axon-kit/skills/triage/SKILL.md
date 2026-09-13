---
name: triage
description: 既存Entityの判断・本文・包含・依存・再浮上条件を、供給された意図に沿って更新する。
---

# 既存Entityを更新する

`axon-kit:conventions` を使い、共通契約で選択したbinary・対象rootを維持する。 show --detailsと関連するlog・Note、関係先を読む。与えられた判断に対応する `accept / withdraw / cancel / reconsider`、本文の `write`、包含の `group set A --parent G / group unset A`、依存の `dep add|rm A --needs B`、条件の `when set A --command ... / when clear A` を使う。

本文変更のためだけにlifecycleを往復させない。whenはシェル文字列を保存し、実行環境・副作用は呼び出し側の意図と照合する。保存操作では条件を実行しない。条件の調査でtriage/tasksを使う場合だけ実行契約をhelpで確認する。

作用を一つずつ検証し、予定にない子の終了や別対象の採用へ進まない。cancelなど終了状態に影響する変更は関係先を調査する。部分適用や結果不明は明示して返す。
