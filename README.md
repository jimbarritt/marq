# marq

A macOS desktop app that renders markdown files in a native window with live reload.

## Features

- GitHub-style markdown rendering (light theme)
- Auto-refresh when the file changes on disk
- Syntax-highlighted code blocks with copy button
- Mermaid.js diagrams
- MathJax / LaTeX math
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

## Requirements

- macOS 13+
- Swift 5.9+
- Internet connection (JS libraries loaded from CDN)
