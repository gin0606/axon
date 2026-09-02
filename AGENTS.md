# axon

軸を分けたローカル issue tracker。このリポジトリは axon 自体の開発で、**タスク管理も axon 自身で行っている**。

以下は axon の一般仕様ではなく、このリポジトリでの運用規約である。
axon 自体は、誰が着手対象や採否を決め、いつレビューするかを規定しない。

**タスクの正は axon の DB (`.axon/`) にある。** git 管理外なので、着手候補は `axon ready`、採否または前提喪失について判断が必要なものは `axon triage` で確認する。`ready` にも `triage` にも出ない状態を含め、全件を確認する必要があるときは `axon list` を使う。変動する情報はドキュメントに書かない (二重管理しないため)。

## Entity の操作

未判断の新しい Issue または Group の登録を依頼されたら、`skills/axon-capture-issue/SKILL.md` の手順に従う。
採用済みの新しい Issue または Group の登録を依頼されたら、`skills/axon-plan-issue/SKILL.md` の手順に従う。
既存 Entity の採否相談または判断見直しを依頼されたら、未判断、前提喪失、Accepted からの不採用化、Rejected / Ended の再検討を含めて `skills/axon-triage-issue/SKILL.md` の手順に従う。
採用済み Entity の実装・計画進行、または Disposition を問わず InProgress Entity の進行同期・引き渡し・打ち切り・完了を依頼されたら、`skills/axon-implement-issue/SKILL.md` の手順に従う。

`axon ready` または `axon triage` の候補整理を依頼された場合は、優先候補、重複候補、判断材料を提案してよい。ただし、ユーザーの確認前に着手や採否の変更を行わない。

## ドッグフーディング

axon を使う中で、CLI の不足、不自然な往復、分かりにくい出力、手作業による補完、ドキュメントとの不一致を見つけたら、一般化できる問題を `skills/axon-capture-issue/SKILL.md` の手順で記録する。

一時的な不慣れや単純な入力ミスは `axon capture` しない。元の作業を不必要に中断せず、記録した改善へ勝手に着手しない。

## 判断と申し送り

判断を変えるときは必ず理由を残す (`axon decide` / `axon when` の `-r`)。
「なぜやるのか」「なぜやらないのか」「なぜ今やらないのか」は、類似の問題を考えるときや決定を再考するときに効く情報で、`axon log <id>` で辿れる。

## ビルド

rust は mise で入れている。

```sh
mise exec -- cargo build
./target/debug/axon --help
```

## 設計を変えるとき

`docs/axes.md` が「なぜこの設計なのか」の記録で、決着した論点に番号が振ってある。
実装で迷ったらここを見る。

基礎状態モデルは `spec/axon.qnt`、Entity と Group の拡張モデルは `spec/group_plan.qnt` に Quint で書いてあり、性質を検査できる。

```sh
quint typecheck spec/axon.qnt
quint typecheck spec/group_plan.qnt
quint run spec/axon.qnt --invariant=<名前>
quint run spec/group_plan.qnt --invariant=<名前>
```

モデルに関わる変更をするときは、**先に docs と該当する spec を更新して検査を通してから実装する**。Progress、Disposition、Resurface condition、dependency、terminal など共有 A / B / C / D の意味や満足条件を変える場合は両 spec を更新・検査する。cross-kind 展開、Group 起点の作用範囲、activation / completion wait graph、包含との相互作用、active scope、Group の完了・release を変える場合は `spec/group_plan.qnt` を更新・検査する。
この順序で進めたことで、議論だけでは見落としていた考慮漏れが実際に見つかっている。

## 実装の規範

- **状態更新は `Store::apply` を通す。** 判断履歴の記録判定もそこにあるため、別経路で書き換えると記録が漏れる
- **導出値をテーブルに持たない。** ready / blocked / orphaned / 進捗は計算する
- **DB から読んだ値は型に変換する境界を 1 箇所に保つ** (`RawEntity::into_entity`)。そこを通れば内部では型を信頼できる
- **軸を混ぜない。** 1 つのコマンドが進行と採否の両方を変えない。セットで打つべき場面は、警告を出して利用者に促す
