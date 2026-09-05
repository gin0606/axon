# Codex の linked worktree から使う

axon は Git の common directory の親にある `.axon/axon.db` を全 worktree で共有する。Codex の workspace-write sandbox では、この共有 DB が現在の linked worktree の外にあると書き込みを拒否されることがある。

axon を PATH 上の `axon` としてインストールした後、ユーザー共通の `~/.codex/rules/axon.rules` を一度だけ作成する。

```starlark
prefix_rule(
    pattern = [
        "axon",
        ["init", "plan", "capture", "start", "done", "release", "write"],
    ],
    decision = "allow",
    justification = "Allow axon state changes to its shared local database",
    match = [
        "axon init",
        "axon plan document sandbox setup",
        "axon capture investigate failure",
        "axon start axon-abc123",
        "axon done axon-abc123",
        "axon release axon-abc123 --reason handoff",
        "axon write axon-abc123 --message description",
    ],
    not_match = [
        "axon list",
        "axon show axon-abc123",
        "git status",
    ],
)

prefix_rule(
    pattern = ["axon", "decide", ["accept", "reject", "undecide"]],
    decision = "allow",
    justification = "Allow axon state changes to its shared local database",
    match = [
        "axon decide accept axon-abc123 --reason approved",
        "axon decide reject axon-abc123 --reason obsolete",
        "axon decide undecide axon-abc123 --reason reconsider",
    ],
    not_match = ["axon decide unknown axon-abc123"],
)

prefix_rule(
    pattern = ["axon", "when", ["at", "after", "clear"]],
    decision = "allow",
    justification = "Allow axon state changes to its shared local database",
    match = [
        "axon when at axon-abc123 2026-09-03",
        "axon when after axon-abc123 axon-def456",
        "axon when clear axon-abc123",
    ],
    not_match = ["axon when unknown axon-abc123"],
)

prefix_rule(
    pattern = ["axon", "dep", ["add", "rm"]],
    decision = "allow",
    justification = "Allow axon state changes to its shared local database",
    match = [
        "axon dep add axon-abc123 --needs axon-def456",
        "axon dep rm axon-abc123 --needs axon-def456",
    ],
    not_match = ["axon dep unknown axon-abc123"],
)

prefix_rule(
    pattern = ["axon", "group", ["plan", "capture", "set", "unset"]],
    decision = "allow",
    justification = "Allow axon state changes to its shared local database",
    match = [
        "axon group plan migration",
        "axon group capture possible migration",
        "axon group set axon-abc123 axon-def456",
        "axon group unset axon-abc123",
    ],
    not_match = ["axon group unknown axon-abc123"],
)

prefix_rule(
    pattern = ["axon", "note", "add"],
    decision = "allow",
    justification = "Allow axon to append Notes to its shared local database",
    match = [
        "axon note add axon-abc123 --message handoff",
        "axon note add axon-abc123 --file result.md",
    ],
    not_match = [
        "axon note list axon-abc123",
        "axon revision show axon-abc123 1",
    ],
)
```

Codex を再起動すると、列挙した axon の状態変更だけが確認なしで sandbox 外においてログインユーザーの権限で実行される。この Rule は `.axon` だけにファイル権限を与えるものではないため、PATH 上の信頼できる axon binary にだけ使う。`axon ready`、`axon show`、`axon list` などの読み取りコマンド、将来追加される未列挙の subcommand、axon 以外のコマンドには一致せず、通常の sandbox 制限が引き続き適用される。実行ファイルの絶対パスや wrapper 経由の呼び出しにも一致しないため、Codex からは `axon ...` の形で実行する。

Codex Rules は実験的機能であり、形式や挙動が変わる可能性がある。現在の仕様と `codex execpolicy check` による確認方法は [Codex Rules の公式ドキュメント](https://developers.openai.com/codex/exec-policy) を参照する。
