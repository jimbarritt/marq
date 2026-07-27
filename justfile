# marq - macOS markdown viewer

version := "1.2.10"
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

### Harness ###################################################################
#
# Marq is a window: nothing about it is observable from a terminal unless the
# app is asked to say what it did. These recipes are that asking. Every one of
# them depends on `swift build`, which is deliberate — testing against a stale
# binary, or against an app instance launched before the change, wasted three
# separate debugging sessions. The template is read once at launch, so a running
# Marq keeps rendering with the template it started with.
#
# Reach for the cheapest instrument that answers the question. `grep` beats
# `just probe` for "why is this grey"; `just probe` beats exporting a PDF and
# looking at it.

harness_dir := ".harness"

# The default width is stated because allocation depends on the available measure.
#
# Metrics for the screen layout, as JSON.
probe FILE="examples/test.md" WIDTH="960": _build
    @.build/debug/marq {{FILE}} --width {{WIDTH}} --dump-metrics - --timeout 60 2>/dev/null

# Column widths, font scale and broken words as they will print.
#
# Measures the A4 page without exporting anything.
probe-print FILE="examples/test.md": _build
    @.build/debug/marq {{FILE}} --dump-metrics - --print --timeout 60 2>/dev/null

# Just the headline: anything listed here is a bug.
problems FILE="examples/test.md": _build
    #!/usr/bin/env bash
    set -euo pipefail
    for mode in screen print; do
        flag=""; [ "$mode" = print ] && flag="--print"
        echo "== $mode"
        .build/debug/marq {{FILE}} --width 960 --dump-metrics - $flag --timeout 60 2>/dev/null \
            | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps(d["problems"], indent=2)); print("tables:", [(t["index"], t["fillPct"], t["fontScale"]) for t in d["tables"]])'
    done

# Screen render as a PNG, whole document. Prints the path.
shot FILE="examples/test.md" WIDTH="960": _build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p {{harness_dir}}
    OUT="{{harness_dir}}/$(basename {{FILE}} .md).png"
    .build/debug/marq {{FILE}} --width {{WIDTH}} --export-png "$OUT" --timeout 60 2>/dev/null
    echo "$OUT"

# Export a PDF and report its shape. Prints the path.
pdf FILE="examples/test.md": _build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p {{harness_dir}}
    OUT="{{harness_dir}}/$(basename {{FILE}} .md).pdf"
    .build/debug/marq {{FILE}} --export-pdf "$OUT" --timeout 90 2>/dev/null
    .build/debug/pdftool info "$OUT" | python3 -c '
    import json, sys
    d = json.load(sys.stdin)
    print("pages:", d["pageCount"])
    for p in d["pages"]:
        print("  p%s: text %s-%spt of %spt wide" % (p["page"], p.get("textLeftPt", 0), p.get("textRightPt", 0), p["widthPt"]))
    '
    echo "$OUT"

# Render PDF pages to PNGs so they can be looked at.
pages PDF FROM="1" TO="": _build
    @.build/debug/pdftool pages {{PDF}} {{harness_dir}}/pages {{FROM}} {{TO}}

# Text with x-positions — where a line starts and ends on the page.
pdf-text PDF PAGE="": _build
    @.build/debug/pdftool text {{PDF}} {{PAGE}}

# A broken word is found here, not by squinting: the fragment is at a known x.
#
# Per-glyph bounds, optionally only for a matching string.
pdf-chars PDF PAGE MATCH="": _build
    @.build/debug/pdftool chars {{PDF}} {{PAGE}} {{MATCH}}

# Column borders as printed, with the gaps between them — the real column widths.
pdf-columns PDF PAGE: _build
    @.build/debug/pdftool vlines {{PDF}} {{PAGE}}

# Compare every fixture's metrics to its committed baseline.
check *FIXTURES: _build
    @python3 tools/check-metrics.py {{FIXTURES}}

# Record the current metrics as the baseline. Read the diff before committing.
bless *FIXTURES: _build
    @python3 tools/check-metrics.py --bless {{FIXTURES}}

# Headless runs carry a watchdog; a binary started by hand does not.
#
# Kill anything left running.
kill-probes:
    #!/usr/bin/env bash
    pkill -f '.build/debug/marq' 2>/dev/null && echo "killed debug marq" || echo "no debug marq running"
    pgrep -fl 'marq|pdftool' || true

_build:
    @swift build >&2

### Packaging #################################################################

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
    echo "=================================================================="
    echo "  GitHub release v$VERSION is live, tap committed locally."
    echo "  The cask is NOT published until the tap is pushed."
    echo "=================================================================="
    read -n1 -s -r -p "  Press SPACE to push the tap now (Ctrl-C to skip)... "
    echo ""
    git push
    echo "Tap pushed — cask v$VERSION is live."

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

# Build and install to /Applications
install-local: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    pkill -x "{{app_name}}" 2>/dev/null || true
    rm -rf "/Applications/{{app_name}}.app"
    cp -r "build/{{app_name}}.app" "/Applications/{{app_name}}.app"
    echo "Installed to /Applications/{{app_name}}.app"

# Clean build artifacts
clean:
    #!/usr/bin/env bash
    swift package clean
    if [ -d build ]; then mv build /tmp/marq-build-$$; fi
