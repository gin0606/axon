# CLIと保存の接続

[CLI](../../src/cli/mod.rs) は引数と入力を共通コア・保存 adapter へ渡し、その結果を表示する。module の配置と読み取りの流れは [層構造](architecture.md)、引数・表示・終了コードは [CLI 契約](../reference/cli.md)、候補の評価は [候補と外部条件](../reference/candidates.md) を参照する。

## 独立した保存先で使う

この checkout の binary を試すときは、既存の保存先の外に fixture を作る。

```sh
cargo build
AXON_BIN="$PWD/target/debug/axon"
FIXTURE_DIR="$(mktemp -d)"
cd "$FIXTURE_DIR"
"$AXON_BIN" init demo
```

以降は [日常の操作](../guide/usage.md) の `axon` を `"$AXON_BIN"` に置き換えて実行する。

## mutationと出力の境界

変更は保存 adapter の lock 内で共通コアに検査させ、保存結果を受け取ってから出力する。lock 前の読取から保存結果を推定しない。保存と出力は別の失敗境界であり、出力失敗で保存成功を未適用と報告しない。診断と BrokenPipe の扱いは [mutation の結果](../reference/cli.md#mutationの結果)、保存側の失敗境界は [書込の保証](lifecycle-file.md#書込の保証) に従う。

## 内部Git呼出し

内部 Git 呼出しは `GIT_*` の override を除外し、現在 directory を探索の起点にする。Git 内では最寄りの `.git` と Git が返す root を照合し、bare repository・壊れた marker から祖先へ fallback しない。repository が探索境界。探索の段と確定の規則、破損で fallback しないことは [保存と統合の契約](../reference/storage.md#探索) に定める。

検証の入口は [各層の検証入口](architecture.md#各層の検証入口)、独立 fixture の方針は [検証方針](verification.md#モデルと運用検証) を参照する。
