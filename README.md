# axon

[日本語](README.ja.md)

A local CLI for managing personal tasks and plans as Issues and Groups. Capture ideas before deciding what to do, organize work with nested Groups and dependencies, and keep notes and a history of changes.

Data lives in a local `.axon/` directory. You can keep it private or track it in Git alongside your project. Shell conditions let you bring work back into view when it becomes relevant.

## Install

Requires Rust 1.89 or later. macOS is supported; Linux and WSL2 are unverified, best-effort environments. Native Windows is not supported.

```sh
git clone https://github.com/gin0606/axon.git
cd axon
cargo install --locked --path .
```

Ensure Cargo's binary directory (usually `~/.cargo/bin`) is on your `PATH`.

## Quick start

In the directory where you want to manage work:

```sh
axon init work
axon capture --label docs --title 'Write a user guide' -m 'Explain installation and basic usage.'
axon proposals
```

See [Getting started](docs/guide/getting-started.md) for the full workflow and Git setup.

## Agent skills

Plugins for Claude Code and Codex are included:

- [axon](plugins/axon/skills): ready-to-use workflows for collaboration between you and an agent.
- [axon-kit](plugins/axon-kit/skills): core operations and contracts for building your own workflows.

The [example workflow](examples/agent-workflow/README.md) includes plugin installation instructions and skills for planning, implementation, verification, and commits. You can use it as-is or adapt it to your workflow.

## Documentation

Run `axon --help` for commands and `axon docs` for the lifecycle and daily workflow. Detailed guides are currently in Japanese:

- [Getting started](docs/guide/getting-started.md)
- [Daily usage](docs/guide/usage.md)
- [All documentation, including development](docs/README.md)

## License

[MIT](LICENSE)
