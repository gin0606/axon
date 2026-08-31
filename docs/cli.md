# CLI

## 方針

- **出力は簡潔なテキスト 1 本**。JSON は作らない。
　主要な利用者がコーディングエージェントであり、JSON はキー名が繰り返されるぶんトークンを食う。
　自然言語のほうが読みやすく短い。スクリプトから使う必要が出てから足す
- **コマンド名に意図を持たせる**。汎用的な動詞 (`add` 等) を避ける。
　迷ったエージェントが「デフォルトらしいもの」に吸い寄せられるのを防ぎ、意図の表明を強制する
- **`--help` がモデルの説明になる**ように、軸ごとの操作をサブコマンドにまとめる

## コマンド

### 日常の操作 (トップレベル)

```
axon init                  # prefix を決めて DB を作る
axon plan <title>          # B=採用 で作成。「やると決めたものを計画に入れる」
axon capture <title>       # B=未判断 で作成。「思いついたものを投げ込む」
axon ready                 # 着手可能なものを一覧 (取得しない)
axon next                  # ready から 1 件取って着手する (atomic)
axon start <id>            # 指定して着手する。既に claim されていたら失敗
axon done <id>             # A=終了 にする
axon list                  # 全 issue を状態つきで一覧
axon show <id>             # 詳細
axon log <id>              # 変更の履歴
axon stale [--hours N]     # 放置されたまま残っている着手を探す
axon release <id>          # 着手を取り消して未着手に戻す
```

`plan` と `capture` を分けているのは、作成時に採否を必ず表明させるため。
デフォルト値を置くと、投げ込んだものが自動的に「やると決めた」ことになり、
B の軸を分けた意味が消える。

### 軸ごとの操作

```
axon decide accept <id> [-r 理由]      # B=採用
axon decide reject <id> [-r 理由]      # B=不採用
axon decide undecide <id> [-r 理由]    # B=未判断

axon when at <id> <date>     # C=日付。その日まで浮上しない
axon when after <id> <ref>   # C=参照。その issue が終端に達するまで浮上しない
axon when clear <id>         # C=なし。常に浮上する

axon dep add <id> --needs <id>   # 依存を張る
axon dep rm  <id> --needs <id>   # 依存を外す

axon group new <slug> [表示名] [--parent <slug>]
axon group list
axon group show <slug>
axon group set <id> <slug>       # issue をグループに入れる
axon group unset <id>
axon group reject <slug>         # 機能群ごとやめる (子孫を一括で不採用)
axon group dep add <slug> --needs <slug>
axon group dep rm  <slug> --needs <slug>
```

グループは状態を持たないので、進捗も完了も子から導出する。
グループ間依存は**全子孫 issue が終端に達したら解除**され、不採用が混じっていても解除する。
祖先グループの依存は子グループにも効く。

`when after` と `dep add` は似て見えるが意味が違う (`axes.md` C-1 参照)。
参照先が不採用になったとき、`when after` は条件が満たされて浮上し、
`dep` は前提喪失 (orphaned) になる。

## ready / start / next の住み分け

想定する運用が 2 通りあるため、3 つとも必要になる。

| 運用 | 使うコマンド |
| --- | --- |
| 人間が `ready` を見て、複数のエージェントに割り振る | `ready` で閲覧 → 各エージェントが `start <id>` |
| エージェントが自律的に次を取る | `next` |

`next` は取得と着手が 1 トランザクションなので競合しない。
`start` は人間が割り振りを誤ったときのために、既に claim されていたら失敗する。

## 出力例

```
$ axon ready
axon-a3f9k2  スキーマを起こす
axon-k9d3p2  actor 検出を実装する

$ axon next
axon-a3f9k2 に着手しました (claude-code)
スキーマを起こす

$ axon done axon-a3f9k2
axon-a3f9k2 を終了しました
着手可能になりました: axon-b7c2m1 ドメイン型を定義する

$ axon list
axon-a3f9k2  [着手中/採用]        スキーマを起こす
axon-b7c2m1  [未着手/採用] 待ち    ドメイン型を定義する
axon-x1y2z3  [未着手/未判断]       C の外部コマンド条件
axon-m4n5o6  [未着手/採用] 2026-10-01  履歴を実装する

$ axon show axon-b7c2m1
axon-b7c2m1  ドメイン型を定義する
進行: 未着手  採否: 採用  時期: 常に浮上
待ち: axon-a3f9k2 (スキーマを起こす)
```

`done` が「着手可能になりました」を出すのは、エージェントが次の作業を知るため。

状態を変える操作は履歴に残る。`-r/--reason` を付けると理由も一緒に記録され、
`axon log` で後から辿れる。とくに不採用の理由は、後のセッションが判断を復元する手がかりになる。

`show` は止まっている理由を遡って表示する。直接の待ち相手が前提を失っている場合や、
その奥に原因がある場合、グループ依存で塞がれている場合をそれぞれ示す。

`stale` は「一定時間動きがない」かつ「着手したプロセスが終了している」ものだけを挙げる。
時間だけで判断すると長時間の作業を誤検出するため。検出しても自動では解放せず、`release` を案内する。

## まだ無いもの

- **JSON 出力**
