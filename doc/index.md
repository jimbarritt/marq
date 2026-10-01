# Marq Documentation

A native macOS markdown viewer with vim keybindings, live reload, and offline rendering.

## Guides

- [The layout harness](agent-harness.md) — How to check a rendering change: the app's own metrics, `pdftool`, and the golden baselines
- [Releasing](releasing.md) — How to build, bundle, and publish a new version
- [Code Signing & Notarization](signing.md) — Setting up Developer ID signing and Apple notarization
- [Moving to Ubiqtek](move-to-ubiqtek.md) — Plan for transferring repo and tap to the Ubiqtek org
- [Comments design](comments-design.md) — Git-backed comments and suggestions: storage on the `md-comments` branch, anchoring, the `marq-comments` CLI
- [Comments agent guide](comments-agent-guide.md) — A block to paste into `CLAUDE.md`: how an agent uses `marq-comments`

## Architecture Decision Records

- [adr/](adr/) — decisions kept alongside their reasoning; see
  [0001](adr/0001-record-architecture-decisions.md) for the convention
