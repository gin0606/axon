# リリース

公開は `vX.Y.Z` タグのpushを起点に [Release workflow](../../.github/workflows/release.yml) が行います。処理の内容はworkflowと `scripts/` を見てください。この文書には、リポジトリの外にある設定と運用の決めごとだけを書きます。

## 決めごと

- rootと `crates/axon-gui` の `Cargo.toml` の `version` を同じ値に手で上げ、mainへ統合してから、mainのcommitにタグを付けます。自動採番やversion更新commitは使いません。
- 公開済みのものを直すときは、タグを付け直さず、新しいversionで公開します。

## リポジトリの外にある設定

- secretsとvariablesは、リポジトリではなく `release` environmentに登録します。このenvironmentは `v*` タグからの実行だけが使えるよう制限しています。
- tapの更新に使うGitHub Appには、`gin0606/homebrew-tap` のContents書込権限が必要です。AppのClient IDは秘密ではないので、variable `APP_CLIENT_ID` に置きます。
- secretsの中身は次のとおりです。
  - `APP_PRIVATE_KEY`: そのAppの秘密鍵
  - `MACOS_CERT_P12_BASE64`・`MACOS_CERT_PASSWORD`: Developer ID Applicationの証明書と秘密鍵を書き出した `.p12` をbase64にしたものと、そのパスワード
  - `NOTARY_KEY_P8_BASE64`・`NOTARY_KEY_ID`・`NOTARY_ISSUER_ID`: 公証に使うApp Store Connect API keyの `.p8` をbase64にしたものと、そのKey ID・Issuer ID

証明書の期限が切れると署名が失敗します。証明書を作り直したら、`MACOS_CERT_P12_BASE64` と `MACOS_CERT_PASSWORD` を更新してください。

## 失敗したとき

原因を解消してから、失敗したjobだけを再実行します（`gh run rerun RUN_ID --failed --repo gin0606/axon`）。workflowは再実行で復旧できるように作ってあります。tapへのpushが拒否されても、強制pushはしません。
