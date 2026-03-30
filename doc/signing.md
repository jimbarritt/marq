# Code Signing & Notarization

Marq is currently distributed unsigned. To properly sign and notarize for
Gatekeeper-friendly distribution, follow these steps.

## Prerequisites

You need a **Developer ID Application** certificate (not "Apple Development"
or "Apple Distribution" — those are for dev builds and App Store respectively).

1. Go to https://developer.apple.com/account/resources/certificates/list
2. Click **+** to create a new certificate
3. Select **Developer ID Application**
4. Follow the prompts to upload a Certificate Signing Request (CSR):
   - Open **Keychain Access** > Certificate Assistant > Request a Certificate from a CA
   - Enter your email, select "Saved to disk", generate
   - Upload the `.certSigningRequest` file
5. Download and double-click the `.cer` to install it in your keychain

## Signing

Find your identity:

```bash
security find-identity -v -p codesigning
```

Look for one that says `Developer ID Application: Your Name (TEAMID)`.

Sign the app:

```bash
just sign "Developer ID Application: Your Name (TEAMID)"
```

This runs `codesign --force --deep --sign IDENTITY --options runtime build/Marq.app`.

## Notarization

### One-time setup

Store your credentials in the keychain so `notarytool` can use them
non-interactively:

```bash
xcrun notarytool store-credentials "notarytool" \
  --apple-id YOUR_APPLE_ID_EMAIL \
  --team-id YOUR_TEAM_ID \
  --password APP_SPECIFIC_PASSWORD
```

Generate an app-specific password at https://appleid.apple.com/account/manage
(Security > App-Specific Passwords).

### Notarize

```bash
just notarize YOUR_APPLE_ID_EMAIL YOUR_TEAM_ID
```

This zips the app, submits to Apple's notary service, waits for approval,
and staples the ticket to the app.

## Full release pipeline

Once signing and notarization are set up:

```bash
just release "Developer ID Application: Your Name (TEAMID)" "your@email" "TEAMID"
```

This runs: build → bundle → sign → zip → notarize → staple.

## Updating the Homebrew cask

After creating a new signed release:

1. Upload the new `build/Marq.zip` to a GitHub release
2. Get the SHA: `shasum -a 256 build/Marq.zip`
3. Update `version` and `sha256` in `jimbarritt/homebrew-tap` cask formula
4. Remove `--no-quarantine` from the cask (only needed for unsigned builds)
