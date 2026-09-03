# axon

軸を分けたローカル issue tracker。このリポジトリは axon 自体の開発で、**タスク管理も axon 自身で行っている**。

以下は axon の一般仕様ではなく、このリポジトリでの運用規約である。
axon 自体は、誰が着手対象や採否を決め、いつレビューするかを規定しない。

**タスクの正は axon の DB (`.axon/`) にある。** git 管理外なので、着手候補は `axon ready`、採否または前提喪失について判断が必要なものは `axon triage` で確認する。`ready` にも `triage` にも出ない状態を含め、全件を確認する必要があるときは `axon list` を使う。変動する情報はドキュメントに書かない (二重管理しないため)。

## Entity の操作

未判断の新しい Issue または Group の登録を依頼されたら、`skills/axon-capture-issue/SKILL.md` の手順に従う。
採用済みの新しい Issue または Group の登録を依頼されたら、`skills/axon-plan-issue/SKILL.md` の手順に従う。
既存 Entity の採否相談、判断見直し、title、description、parent、outgoing dependency の変更を依頼されたら、Disposition や Progress を問わず `skills/axon-triage-issue/SKILL.md` の手順に従う。Undecided の declaration 編集は採否を変えずに反映できるが、Accepted / Rejected の実変更は同 skill の再判断手順を使う。
既存 Entity への独立した追加情報の Note 追記を依頼されたら、または他の workflow で作業結果や申し送りを残す必要が生じたら、`skills/axon-add-note/SKILL.md` の手順に従う。
採用済み Entity の実装・計画進行、または Disposition を問わず InProgress Entity の進行同期・引き渡し・打ち切り・完了を依頼されたら、`skills/axon-implement-issue/SKILL.md` の手順に従う。
Entity 数を問わず、tracker data に対する宣言 file の review / canonicalize、または `axon export` / `axon import prepare|check|apply` の実行を扱うときは `skills/axon-declaration-plan/SKILL.md` に従う。内容の採否や判断も伴う場合は、同 skill が定める順序で plan / capture / triage skill を併用する。これらのコマンド自体の実装、文書、テストを変更・レビューする作業には declaration skill を使わない。宣言 file を使わない既存 Entity の declaration 編集は triage skill、それ以外の単一 Entity 操作は該当する既存 skill を使う。

`axon ready` または `axon triage` の候補整理を依頼された場合は、優先候補、重複候補、判断材料を提案してよい。ただし、ユーザーの確認前に着手や採否の変更を行わない。

## ドッグフーディング

axon を使う中で、CLI の不足、不自然な往復、分かりにくい出力、手作業による補完、ドキュメントとの不一致を見つけたら、一般化できる問題を `skills/axon-capture-issue/SKILL.md` の手順で記録する。

一時的な不慣れや単純な入力ミスは `axon capture` しない。元の作業を不必要に中断せず、記録した改善へ勝手に着手しない。

## 判断と申し送り

Entity を操作する前は `axon show <id>` の出力を省略せず、plan declaration、Control state、記録件数、表示された全 Note を読む。Declaration Revision があれば `axon revision list <id>` と各 `axon revision show <id> <number>` も読み、古い Revision や Note を重要でないものとして読み飛ばさない。

判断や作業の保留理由を状態変更へ結び付けるときは、必ず reason を残す (`axon decide` / `axon when` / `axon release` の `-r`)。
「なぜやるのか」「なぜやらないのか」「なぜ今やらないのか」は、類似の問題を考えるときや決定を再考するときに効く情報で、`axon log <id>` で辿れる。

Entity が何であるかを変える title、description、parent、outgoing dependency は plan declaration である。Accepted / Rejected の declaration を変える場合は、理由付きで Undecided に戻し、編集後の全文を確認してから改めて採否を判断する。

調査結果、作業結果、申し送りなど、Entity の定義や状態を変えない追加情報は Note へ追記する。既存 description へ継ぎ足さない。明示された Note 追加と、着手中 Entity で確定した重要情報は、エージェントが本文を構成して追加してよい。単なる進捗実況、定型的な開始・完了報告、reason と同じ内容の重複 Note は作らない。

Note は追記専用である。誤りや前提変化は既存 Note を編集せず、元の Note 番号を示す新しい Note として残す。plan declaration や Control state を変える内容は Note として反映せず、該当する Entity workflow で扱う。情報分類または対象 Entity が不明な場合と、Note 本文がユーザーの判断を代弁する場合だけ、追加前にユーザーへ確認する。

## ビルド

rust は mise で入れている。

```sh
mise exec -- cargo build
./target/debug/axon --help
```

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
