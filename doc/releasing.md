# Releasing Marq

## How distribution works

Marq is distributed as a pre-built `.app` bundle via Homebrew Cask. Homebrew
downloads the zip from the GitHub release, verifies the SHA256, and extracts
`Marq.app` to `/Applications/`. No compilation happens on the user's machine.

The binary in the zip is built for the architecture of the build machine
(currently arm64/Apple Silicon). For universal binaries, see the "Universal
builds" section below.

## Release process

### 1. Build the app bundle

```bash
just bundle
```

This compiles a release binary, generates the .icns icon, and assembles
`build/Marq.app`.

### 2. Sign and notarize (optional — see doc/signing.md)

```bash
just release "Developer ID Application: ..." "email" "TEAMID"
```

Or skip this step for unsigned distribution.

### 3. Create the zip

```bash
cd build && zip -r Marq.zip Marq.app
```

### 4. Create a GitHub release

```bash
gh release create v1.x.x build/Marq.zip \
  --title "Marq v1.x.x" \
  --notes "Release notes here"
```

### 5. Update the Homebrew cask

Get the SHA of the new zip:

```bash
shasum -a 256 build/Marq.zip
```

Then update `Casks/marq.rb` in the `jimbarritt/homebrew-tap` repo:

```ruby
cask "marq" do
  version "1.x.x"
  sha256 "NEW_SHA_HERE"
  # ...
end
```

Push the change. Users can then `brew upgrade marq`.

## Universal builds

To support both Intel and Apple Silicon in a single binary:

```bash
swift build -c release --arch arm64 --arch x86_64
```

Then run `just bundle` as normal (it copies from `.build/release/marq`).

## What users get

```
brew tap jimbarritt/tap
brew install --cask marq
```

Homebrew downloads `Marq.zip` from the GitHub release, checks the SHA256,
extracts `Marq.app` to `/Applications/`, and registers it with LaunchServices.
Users can then run `open -a Marq file.md`.
