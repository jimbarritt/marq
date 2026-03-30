# Moving Marq to Ubiqtek

Marq will move to the Ubiqtek organisation on GitHub.

## Steps

1. **Transfer repo**: GitHub > Settings > Transfer ownership to `ubiqtek`
2. **Transfer or recreate tap**: create `ubiqtek/homebrew-tap` repo
3. **Update cask formula**: change URL from `jimbarritt/marq` to `ubiqtek/marq`
4. **Update bundle ID**: change `com.jimbarritt.marq` to `com.ubiqtek.marq` in `Sources/marq/Info.plist`
5. **Update README**: brew install command becomes `brew install --cask ubiqtek/tap/marq`

## Things to update

- `Sources/marq/Info.plist` — `CFBundleIdentifier`
- `justfile` — `bundle_id` variable
- `Casks/marq.rb` in homebrew-tap — download URL
- README.md — install instructions
