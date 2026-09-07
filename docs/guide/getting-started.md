# 使い始める

axon CLIと公式のAxon Skill Kitを導入したら、普段使っているcoding agentに相談しながら使い始められます。

## インストールと対応環境

インストール手順はWIPです。配布方法が決まったら、[README](../../README.md#インストール)に短い手順、ここに詳しい手順を載せます。

macOSをサポート対象とし、Apple Silicon macOSで開発・検証しています。LinuxとWSL2は未検証のbest effort、native Windowsは非対応です。ソースからのビルドに必要な最低Rustバージョンは1.89です。開発環境は[検証方針](../development/verification.md#rust-toolchain)を参照してください。

## Agent向けpluginを入れる

[`axon-kit@axon`](../../plugins/axon-kit)は、agentがaxonの状態や操作のルールを理解するための公式Skill Kitです。IssueやGroupの操作を、履歴や計画の意味を保って行うための共通の土台として使います。どの仕事を選ぶか、どこまでagentに任せるかは、利用者の指示や協業方針と組み合わせます。

[`axon@axon`](../../plugins/axon)は、公式kitに作者個人の協業方針を重ねる任意の追加pluginです。合意した範囲はagentに進めてもらい、重要な判断はユーザーへ返す、という方針で、重複確認や計画の整理、申し送りなどを扱います。好みと違うところがあれば、`plugins/axon`をコピーしてカスタマイズして使ってください。公式kitと併用するもので、置き換えるものではありません。

使っているagentに合わせて、marketplaceを登録してインストールします。axon CLIのインストールも別途必要です。

Codexの場合：

```sh
codex plugin marketplace add gin0606/axon
codex plugin add axon-kit@axon
codex plugin add axon@axon # Optional
```

Claude Codeの場合：

```sh
claude plugin marketplace add gin0606/axon
claude plugin install axon-kit@axon
claude plugin install axon@axon # Optional
```

SQLite backendでlinked worktreeから共有DBを更新する際、Codexのsandboxに書き込みを阻まれる場合は、[Codexでのアクセス設定に必要な情報](codex.md)を参照してください。

## データをGitに置くか決める

| backend（保存方式） | 意図 |
| --- | --- |
| SQLite（既定） | 手元でタスクと計画を管理する。管理データをGitに置く必要はない |
| file | その管理データもGitに置き、コードと一緒に変更を残す |

SQLite backendが、ローカルでタスク管理をするための基本形です。Issueや計画もGitで管理したい場合にfile backendを選びます。

この選択でworktree間の扱いも変わります。SQLiteはlinked worktree間で同じデータを共有し、fileはworktreeごとのデータをGitで取り込みます。具体的な保存先や初期化時の変更は[backendとworktree](storage.md)にまとめています。

## 使い方はAgentに聞いてみる

公式kitには、axonの状態や操作のルールをまとめています。使い方を知りたいときは、ドキュメントを一通り読むより、kitを入れたagentに聞くのが手っ取り早いと思います。

「axonで、まだやるか決めていないことを残したい」「axonで管理している計画を別のworktreeで進めたい」「axonでライブラリの更新待ちを設定したい」など、axonを使いたいことと、やりたいことを伝えてみてください。

自分で確認したい場合は、[日常の操作](usage.md)、[状態と用語](concepts.md)、[backendとworktree](storage.md)も参照できます。
