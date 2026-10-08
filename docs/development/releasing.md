# リリース

正式版の `vX.Y.Z` タグをpushすると、[Release workflow](../../.github/workflows/release.yml) が検証、macOS向けバイナリの公開、[Homebrew tap](https://github.com/gin0606/homebrew-tap) の `axon.rb` 更新、両CPUでのHomebrew導入検証を行います。配布はApple SiliconとIntelのmacOS 15（Sequoia）以上が対象で、それより古いOSでの動作は保証しません。

## 公開する

1. rootの `Cargo.toml` の `version` を更新し、必要な `Cargo.lock` の変更と一緒にmainへ統合します。自動採番やversion更新commitは行いません。
2. [検証方針](verification.md) のfull verificationとMSRV検査が通ることを確認します。リリース用スクリプトは `python3 scripts/test-release.py` で公開を伴わずに検証できます。
3. mainの対象commitでversionと未使用タグを確認してからpushします。以下はversionが `0.1.0` の場合です。

   ```sh
   git switch main
   git pull --ff-only
   python3 scripts/release.py validate-tag v0.1.0
   git ls-remote --tags origin refs/tags/v0.1.0
   # 上の出力が空であることを確認する
   git tag v0.1.0
   git push origin v0.1.0
   ```

4. GitHub ActionsのRelease実行で、`validate`、両CPUの `build`、`publish`、両CPUの `verify-homebrew` が成功したことを確認します。

tapの認証にはActions secretsの `APP_ID` と `APP_PRIVATE_KEY` を使います。Appに `gin0606/homebrew-tap` のContents書込権限を与えてください。発行するtokenの対象はこのtapだけです。

## tap更新が失敗したとき

Releaseが公開済みでも、tapへのpushが失敗すればworkflowは失敗します。認証や競合の原因を解消してから、該当実行の失敗jobだけを再実行します。

```sh
gh run rerun RUN_ID --failed --repo gin0606/axon
```

`publish` は公開済みarchiveからFormulaを生成し直すため、ビルドartifactが期限切れでも再実行で復旧できます。tapに新しいversionがあれば古い実行は失敗し、巻き戻しません。同時更新でpushが拒否された場合も、強制pushせず失敗として扱います。

Homebrew検証だけが失敗した場合も、原因を確認したうえで同じコマンドで失敗jobを再実行します。公開済みバイナリの修正が必要なら、新しいversionをmainへ統合して新しいタグで公開します。
