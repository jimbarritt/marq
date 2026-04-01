# Marq

A macOS desktop app that renders markdown files in a native window with live reload.

See [examples/test.md](examples/test.md) for a demo of all supported features.

## Install

```bash
brew tap jimbarritt/tap
brew install --cask marq
```

Since Marq is currently unsigned, you'll need to allow it past Gatekeeper on first run:

```bash
xattr -cr /Applications/Marq.app
```

Then open any markdown file with:

```bash
open -a Marq path/to/file.md
```

### Set as default markdown handler

To open `.md` files with Marq by default (e.g. double-clicking in Finder):

```bash
brew install duti
duti -s com.jimbarritt.marq .md all
duti -s com.jimbarritt.marq .markdown all
```

If Marq doesn't register immediately, force Launch Services to re-index it first:

```bash
/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister -f /Applications/Marq.app
```

## Features

- GitHub-style markdown rendering (light theme)
- All rendering libraries bundled offline (no internet required)
- Auto-refresh when the file changes on disk
- Follow relative markdown links between files
- Syntax-highlighted code blocks with copy button
- Mermaid.js diagrams
- KaTeX math / LaTeX
- Local and remote images
- "Copy Markdown" button (top right)
- Vim-style navigation keys

## Usage

```bash
open -a Marq path/to/file.md
```

### Building from source

```bash
just bundle        # builds build/Marq.app
just run-app       # builds and opens with test doc
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll down / up |
| `Ctrl-D` / `Ctrl-U` | Half page down / up |
| `gg` | Go to top |
| `G` | Go to bottom |
| `/` | Search |
| `n` / `N` | Next / previous match |
| `Esc` | Close search |
| `Cmd-Left` / `Cmd-Right` | Navigate back / forward |
| `Cmd-Q` | Quit |

## Requirements

- macOS 14+
- Swift 5.9+
