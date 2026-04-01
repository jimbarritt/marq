# marq - macOS markdown viewer

version := "1.1.1"
app_name := "Marq"
bundle_id := "com.jimbarritt.marq"

# Show the current version
version:
    @echo "{{version}}"

# Bump the version in justfile and Info.plist
bump VERSION:
    sed -i '' 's/^version := ".*"/version := "{{VERSION}}"/' justfile
    sed -i '' 's/<string>{{version}}<\/string>/<string>{{VERSION}}<\/string>/g' Sources/marq/Info.plist
    @echo "Version bumped to {{VERSION}}"

# Build and run marq with test doc. Pass --debug to run in foreground with logs.
run-local *FLAGS:
    #!/usr/bin/env bash
    swift build
    if echo "{{FLAGS}}" | grep -q -- "--debug"; then
        .build/debug/marq examples/test.md
    else
        nohup .build/debug/marq examples/test.md &>/dev/null &
    fi

# Build the .app bundle
bundle: _build-release _build-icon
    #!/usr/bin/env bash
    set -euo pipefail
    APP="build/{{app_name}}.app"

    # Clean and create .app structure
    mkdir -p "$APP/Contents/MacOS"
    mkdir -p "$APP/Contents/Resources"

    # Copy binary
    cp .build/release/marq "$APP/Contents/MacOS/marq"

    # Copy Info.plist
    cp Sources/marq/Info.plist "$APP/Contents/Info.plist"

    # Copy icon
    cp build/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

    # Copy SPM resource bundle (contains template.html and vendor assets)
    cp -r .build/release/marq_marq.bundle "$APP/Contents/Resources/"

    echo "Built $APP"

# Build release binary
_build-release:
    swift build -c release

# Generate .icns from SVG logo
_build-icon:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p build
    ICONSET="build/AppIcon.iconset"
    mkdir -p "$ICONSET"

    # Generate all required sizes using qlmanage (native macOS SVG renderer)
    for size in 16 32 64 128 256 512 1024; do
        qlmanage -t -s "$size" -o "$ICONSET" assets/icon.svg 2>/dev/null
        mv "$ICONSET/icon.svg.png" "$ICONSET/tmp_${size}.png"
    done
    for size in 16 32 128 256 512; do
        cp "$ICONSET/tmp_${size}.png" "$ICONSET/icon_${size}x${size}.png"
        double=$((size * 2))
        cp "$ICONSET/tmp_${double}.png" "$ICONSET/icon_${size}x${size}@2x.png"
    done

    iconutil -c icns "$ICONSET" -o build/AppIcon.icns
    echo "Built build/AppIcon.icns"

# Build and zip for distribution (unsigned)
package: bundle _zip
    @echo "Ready: build/{{app_name}}.zip"
    @shasum -a 256 "build/{{app_name}}.zip"

# Build, release to GitHub, and update homebrew cask
publish: package
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION="{{version}}"
    ZIP="build/{{app_name}}.zip"
    TAP="/tmp/homebrew-tap"
    CASK="$TAP/Casks/marq.rb"

    # Create GitHub release and upload zip
    gh release create "v$VERSION" "$ZIP" \
        --title "Marq v$VERSION" \
        --notes "See README for install instructions." \
        --repo jimbarritt/marq

    # Compute SHA256
    SHA=$(shasum -a 256 "$ZIP" | cut -d' ' -f1)
    echo "SHA256: $SHA"

    # Update homebrew tap
    if [ ! -d "$TAP" ]; then
        git clone git@github.com:jimbarritt/homebrew-tap.git "$TAP"
    else
        cd "$TAP" && git pull && cd -
    fi

    # Update version and sha256 in cask
    sed -i '' "s/version \".*\"/version \"$VERSION\"/" "$CASK"
    sed -i '' "s/sha256 \".*\"/sha256 \"$SHA\"/" "$CASK"

    # Commit cask update
    cd "$TAP"
    git add Casks/marq.rb
    git commit -m "marq v$VERSION"

    echo ""
    echo "Done. Now push the tap:"
    echo "  cd $TAP && git push"

# Sign the .app with Developer ID
sign IDENTITY: bundle
    codesign --force --deep --sign "{{IDENTITY}}" --options runtime "build/{{app_name}}.app"
    echo "Signed build/{{app_name}}.app"

# Notarize the .app with Apple
notarize APPLE_ID TEAM_ID: _zip
    xcrun notarytool submit "build/{{app_name}}.zip" \
        --apple-id "{{APPLE_ID}}" \
        --team-id "{{TEAM_ID}}" \
        --keychain-profile "notarytool" \
        --wait
    xcrun stapler staple "build/{{app_name}}.app"
    echo "Notarized and stapled"

# Create zip for distribution
_zip:
    cd build && zip -r "{{app_name}}.zip" "{{app_name}}.app"

# Build, sign, notarize, and zip for release
release IDENTITY APPLE_ID TEAM_ID: (sign IDENTITY) (notarize APPLE_ID TEAM_ID)
    @echo "Release ready: build/{{app_name}}.zip"

# Run the bundled .app (rebuilds automatically)
run-app: bundle
    open "build/{{app_name}}.app" --args "$(pwd)/examples/test.md"

# Clean build artifacts
clean:
    #!/usr/bin/env bash
    swift package clean
    if [ -d build ]; then mv build /tmp/marq-build-$$; fi
