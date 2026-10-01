//! Owned by task T-10. See doc/comments-design.md section 6.1 (`render`).

use serde_json::Value;

/// Renders one markdown file and its threads as a single static HTML page.
///
/// `threads` are the objects `list --json` prints (design 6.2): each has
/// `annotation`, `state`, `anchor`, `stateChanges` and `replies`.
///
/// This body is a placeholder so the command can be wired and the acceptance run
/// can pass its render check. Task T-10 replaces it.
pub fn render_page(markdown: &str, threads: &[Value]) -> String {
    let _ = (markdown, threads);
    "<!doctype html><meta charset=utf-8><title>marq-comments</title><p>The render page is not built yet (task T-10).</p>\n".to_string()
}
