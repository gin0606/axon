# axon

軸を分けたローカル issue tracker。このリポジトリは axon 自体の開発で、**タスク管理も axon 自身で行っている**。

以下は axon の一般仕様ではなく、このリポジトリでの運用規約である。
axon 自体は、誰が着手対象や採否を決め、いつレビューするかを規定しない。

**タスクの正は axon の DB (`.axon/`) にある。** git 管理外なので、着手候補は `axon ready`、採否または前提喪失について判断が必要なものは `axon triage` で確認する。`ready` にも `triage` にも出ない状態を含め、全件を確認する必要があるときは `axon list` を使う。変動する情報はドキュメントに書かない (二重管理しないため)。

このリポジトリでの Axon 操作には、個人用協業方針として [`axon:conventions`](plugins/axon/skills/conventions/SKILL.md) を適用する。

## ドッグフーディング

axon を使う中で、CLI の不足、不自然な往復、分かりにくい出力、手作業による補完、ドキュメントとの不一致を見つけたら、一般化できる問題を `axon:register` から `axon-kit:capture` を使う手順で記録する。

一時的な不慣れや単純な入力ミスは `axon capture` しない。元の作業を不必要に中断せず、記録した改善へ勝手に着手しない。

## 設計を変えるとき

`docs/axes.md` が「なぜこの設計なのか」の記録で、決着した論点に番号が振ってある。
実装で迷ったらここを見る。

基礎状態モデルは `spec/axon.qnt`、Entity と Group の拡張モデルは `spec/group_plan.qnt`、情報分類と declaration 固定は `spec/information_model.qnt` に Quint で書いてあり、性質を検査できる。

```sh
quint typecheck spec/axon.qnt
quint typecheck spec/group_plan.qnt
quint typecheck spec/information_model.qnt
quint run spec/axon.qnt --invariant=<名前>
quint run spec/group_plan.qnt --invariant=<名前>
```

`quint run` を引数なしで成功させても列挙した性質の検査にはならない。3 model の invariant / witness をまとめて検査するときは、`docs/axes.md` の「5. Quint models と検査」にある Quint version、backend、thread 数、sample 数、step 数、seed を含む再現条件をそのまま使う。exit status 0 に加えて列挙した全 witness が 1 trace 以上で観測されたことを確認する。

モデルに関わる変更をするときは、**先に docs と該当する spec を更新して検査を通してから実装する**。Progress、Disposition、Resurface condition、dependency、terminal など共有 A / B / C / D の意味や満足条件を変える場合は `spec/axon.qnt` と `spec/group_plan.qnt` を更新・検査する。plan declaration、Revision、Note、Control state と typed history の境界や操作範囲を変える場合は `docs/information-model.md` と `spec/information_model.qnt` も更新し、`docs/axes.md` に列挙した invariant / witness を検査する。cross-kind 展開、Group 起点の作用範囲、activation / completion wait graph、包含との相互作用、active scope、Group の完了・release を変える場合は `spec/group_plan.qnt` を更新・検査する。
この順序で進めたことで、議論だけでは見落としていた考慮漏れが実際に見つかっている。

## 実装の規範

- **状態更新は `Store::apply` を通す。** 判断履歴の記録判定もそこにあるため、別経路で書き換えると記録が漏れる
- **導出値をテーブルに持たない。** ready / blocked / orphaned / 進捗は計算する
- **DB から読んだ値は型に変換する境界を 1 箇所に保つ** (`RawEntity::into_entity`)。そこを通れば内部では型を信頼できる
- **軸を混ぜない。** 1 つのコマンドが進行と採否の両方を変えない。セットで打つべき場面は、警告を出して利用者に促す
