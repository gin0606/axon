---
name: storage
description: Axon management root を初期化する、または完全な file snapshot を検証する。backend 選択、init の復旧、Git ignore への影響、axon storage check に使い、migration、merge conflict 解消、通常の Entity 操作には使わない。
---

# Axon storage を初期化または検証する

`axon-kit:conventions` を使う。`init` の前に mutation contract を読む。`storage check` は read-only である。

## backend 選択を明示する

新しい root を作成するとき、呼び出し側が `sqlite` または `file` を与える。共有 SQLite と worktree-local な tracked state の選択を実装詳細として決めない。Git repository ごとに 1 backend だけが support される。worktree 間で backend が混在する状態は未 support の運用状態であり、selector を作る理由にはならない。

backend 設定 file はない。Git 内では、file storage は現在の worktree root の `.axon/state.jsonl`、SQLite は Git common directory の parent 配下の `.axon/axon.db` にあり、worktree 登録なしで共有される。Git 外の通常の discovery は、canonical state または `init.pending` がある最も近い ancestor で止まる。両方の canonical path がある場合は mixed-backend error、どちらもなければ uninitialized である。破損、読み取り不能な状態、pending marker がある場合、fallback せず discovery を停止する。規定された path 外の file が authoritative だと推測しない。

## 状態を置き換えず初期化する

`axon init --backend <sqlite|file> [prefix]` を単独の mutation として実行する。storage-root directory 名を意図する ID prefix とする場合だけ prefix を省略する。Git 外での init は current directory を使い、既存 management root 配下への入れ子を拒否する。

Init は新規作成専用であり、valid なものを含め、既存の state があれば拒否する。reset、upgrade、backend 切り替え、repair は行わない。SQLite は ignore file を変えず database を作成する。File init は Git の内外を問わず、`.axon/.gitignore`（自身と `state.jsonl` 以外を ignore）と root の `.gitattributes`（`/.axon/state.jsonl merge=axon`）を作成または補完する。

作成または更新した state と integration artifact を報告する。Init は driver の登録、stage、commit を行わない。無関係な rule を保持する。上位階層または global の ignore 設定が `.axon/` を隠していることは user policy であり、初期化 error ではなく、その policy を変更する許可にもならない。

部分的な初期化の後は、state、pending marker、temporary file、integration file を保持する。writer を停止し、報告された path を調査する。適切な権限と valid な state がある場合だけ auxiliary file を手動で完成させ、検証後にだけ marker を削除する。それ以外では、新しく init する前に不完全な management root を別の場所へ保存する。init の再実行は復旧ではなく、既存 state を上書きしてはならない。

## 完全な snapshot を検証する

canonical file storage を read-only で検証するには `axon storage check <snapshot>` を実行する。backend discovery、Git index、Command evaluation は使わない。check の成功はその byte 列の構造的 validity を立証するが、snapshot を authoritative にしたり、別 file に drift がないことを証明したりはしない。

## 結果を返す

選択した mode、management root または snapshot、該当する場合は backend と authoritative path、store identity、検証結果、Git ignore への影響、変更した artifact、storage 結果の分類を返す。別の許可済み workflow がその作用を所有しない限り、data migration、Git driver 登録、stage、commit、live root 切り替えを行わない。
