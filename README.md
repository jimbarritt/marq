# marq

A macOS desktop app that renders markdown files in a native window with live reload.

See [examples/test.md](examples/test.md) for a demo of all supported features.

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
swift run marq path/to/file.md
```

Or build a release binary:

```bash
swift build -c release
.build/release/marq file.md
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

- macOS 13+
- Swift 5.9+
