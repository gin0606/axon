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
```

`plan` と `capture` を分けているのは、作成時に採否を必ず表明させるため。
デフォルト値を置くと、投げ込んだものが自動的に「やると決めた」ことになり、
B の軸を分けた意味が消える。

### 軸ごとの操作

```
axon decide accept <id>      # B=採用
axon decide reject <id>      # B=不採用
axon decide undecide <id>    # B=未判断

axon when at <id> <date>     # C=日付。その日まで浮上しない
axon when after <id> <ref>   # C=参照。その issue が終端に達するまで浮上しない
axon when clear <id>         # C=なし。常に浮上する

axon dep add <id> --needs <id>   # 依存を張る
axon dep rm  <id> --needs <id>   # 依存を外す
```

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

## 最小スコープから外すもの

- **グループ関連のコマンド** — 開発初期は規模が小さく、グループ無しで回る。
　ただしグループ間依存は本命の要求なので、次の段階で入れる
- **JSON 出力**
- **`blockedReason` の根本原因辿り** — `show` では直接の依存先だけ表示する
