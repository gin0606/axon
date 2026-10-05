# デスクトップアプリ（GPUI）

`crates/axon-gui` は GPUI で作るデスクトップアプリの crate で、binary `axon-gui` と、UI テストが使う library を持つ。CLI とは別 crate とし、CLI 単体のビルドに GPUI を要求しない。層の位置づけは [層構造の地図](architecture.md) を参照する。

## 起動と検証

workspace の `default-members` に GUI を含めないため、root の `cargo build`・`cargo run` は CLI だけを対象にする。GUI は package を指定して起動・検証する。

```sh
cargo run -p axon-gui
cargo test --locked -p axon-gui
```

UI テストは GPUI の test platform 上の headless window で動き、ディスプレイや GPU を使わない。`--workspace` を付けた full verification は GUI も対象に含める。手順は [検証方針](verification.md) にある。

## 採用した依存

| crate | 指定 | 役割 |
| --- | --- | --- |
| `gpui-kit` | `=0.7.1` | GPUI、GPUI Base、GPUI Component と既定 asset の facade。アプリは GPUI を `gpui_kit` 経由で使う |
| `gpui-pre` | `gpui-kit` が `=0.3.8` に固定 | Zed の GPUI の snapshot。直接は依存しない |

版は exact 指定と `Cargo.lock` で固定し、開発ブランチや Git 依存には追従しない。更新するときは版を上げる変更として、この文書の要件と確認結果も見直す。一次資料は [GPUI Kit の導入手順](https://gpui-kit.com/docs/installation/) と [v0.7.1 の Cargo.toml](https://github.com/longbridge/gpui-kit/blob/v0.7.1/Cargo.toml)。

GPUI は serde_json の `preserve_order` を有効にするため、GUI と同じ build では CLI・コアの serde_json も挿入順の map になる。記録の canonical bytes がこれに依存しないよう、コアは記録者 metadata のキーを整列して書く（[記録 file と codec](lifecycle-file.md#記録-file-と-codec)）。

## 要件

- Rust: GUI の `rust-version` は 1.95。固定した依存の一部が 1.95 で安定化した標準ライブラリ API を使う。pull request の CI とリリース前の検証で `cargo +1.95.0 check --locked -p axon-gui --all-targets` を実行する。CLI の MSRV（`Cargo.toml` の 1.89）は GUI に合わせて上げない。
- macOS: 15 以上と Xcode Command Line Tools（GPUI Kit の要件）。
- Linux: ビルドに `gcc g++ clang libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev` を使う。CI は Ubuntu 24.04 でこれを導入し、ビルド・lint と headless の UI テストだけを行う。リンクは x86_64 Linux の Rust 既定である LLD を前提にする。window の表示には Wayland または X11 の session と Vulkan driver が要る。
- Windows: GPUI Kit は Windows 10 以上、MSVC toolchain、Visual Studio 2022 Build Tools、CMake を要件とする。

## 操作の仕様

ウィンドウはタイトル入力、種類（label）を選ぶメニュー、本文の複数行入力を持つ。種類の選択肢はコアの `Label::ALL` から作り、項目名には記録と CLI と同じ綴り（`Label::name()`）をそのまま使う。メニューは項目を選ぶと閉じる。

Tab と Shift-Tab は各欄の間でフォーカスを移す。本文でも Tab は字下げではなくフォーカス移動に使い、キーボードだけで本文から抜けられるようにする。本文の字下げは `cmd-]`・`cmd-[`（macOS）で行う。ウィンドウを閉じるとアプリを終了する。macOS のメニューバーには終了と、編集（取り消す・やり直す・カット・コピー・ペースト・すべてを選択）を置く。

## 確認結果

2026-10-05、macOS 27.0.1（Apple Silicon）、Rust 1.98.0、上記の依存で次を確認した。

- `cargo run -p axon-gui` でウィンドウが開き、日本語のプレースホルダーと説明文が表示される。システムの外観（ライト・ダーク）に追従する。
- headless の UI テスト（`crates/axon-gui/tests/workbench.rs`）で、タイトルと本文への日本語入力と本文の改行、Tab・Shift-Tab によるフォーカスの一巡、メニューの選択と閉鎖、最小ウィンドウ（420×320）で全操作要素がウィンドウ内に収まることを確認した。テストの文字入力は OS の入力メソッドを経由せず、文字幅は test platform の固定幅で測るため、実際のフォントでの収まりは確かめていない。
- GUI の Rust 1.95.0 でのビルドと、CLI の Rust 1.89.0 での `cargo +1.89.0 check --locked --all-targets --all-features` が通る。
- Ubuntu 24.04（aarch64 の container、4 CPU、メモリ 7.7 GB、LLD でリンク）で上記の package を導入し、full verification の各 command が headless で通る。同じ環境で GNU ld を使うと、GUI の test binary のリンクがメモリ不足で失敗した。GitHub Actions の x86_64 runner では未実行。

未確認の範囲は次のとおり。

- 実機での日本語 IME の変換操作（変換候補の表示位置、確定前文字列の表示、確定）と、実機のキーボード・マウスによるフォーカス移動とメニュー操作。
- Linux と Windows でのウィンドウ表示と入力。Linux は CI 相当のビルドと headless テストだけを対象にする。
- macOS 15・16 と Intel Mac での起動。
