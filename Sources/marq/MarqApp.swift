import AppKit
import WebKit

class SilentWebView: WKWebView {
    override func keyDown(with event: NSEvent) {
        // Try performKeyEquivalent first (dispatches to JS), fall back silently
        if !performKeyEquivalent(with: event) {
            // Don't call super.keyDown — that triggers NSBeep
        }
    }
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var webView: WKWebView!
    var fileWatcher: FileWatcher?
    var filePath: String = ""
    var rawMarkdown: String = ""
    var history: [String] = []
    var historyIndex: Int = -1

    func log(_ msg: String) {
        let ts = ISO8601DateFormatter().string(from: Date())
        let line = "[marq \(ts)] \(msg)\n"
        FileHandle.standardError.write(Data(line.utf8))
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        log("Starting up")
        // Resolve file path from CLI args
        let args = CommandLine.arguments
        if args.count > 1 {
            let path = args[1]
            if path.hasPrefix("/") {
                filePath = path
            } else {
                let cwd = FileManager.default.currentDirectoryPath
                filePath = URL(fileURLWithPath: cwd).appendingPathComponent(path).path
            }
        }

        log("File path resolved: \(filePath.isEmpty ? "(none)" : filePath)")

        // Configure WKWebView with message handler for link navigation
        let config = WKWebViewConfiguration()
        let contentController = WKUserContentController()
        contentController.add(self, name: "navigate")
        contentController.add(self, name: "openFile")
        config.userContentController = contentController

        log("Creating WKWebView")
        webView = SilentWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self

        log("Creating window")
        // Create window
        let screenRect = NSScreen.main?.frame ?? NSRect(x: 0, y: 0, width: 960, height: 700)
        let windowWidth: CGFloat = 960
        let windowHeight: CGFloat = 700
        let windowRect = NSRect(
            x: (screenRect.width - windowWidth) / 2,
            y: (screenRect.height - windowHeight) / 2,
            width: windowWidth,
            height: windowHeight
        )

        window = NSWindow(
            contentRect: windowRect,
            styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )

        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.toolbar = nil
        window.isMovableByWindowBackground = true
        window.standardWindowButton(.closeButton)?.isHidden = true
        window.standardWindowButton(.miniaturizeButton)?.isHidden = true
        window.standardWindowButton(.zoomButton)?.isHidden = true

        let fileName = filePath.isEmpty ? "marq" : URL(fileURLWithPath: filePath).lastPathComponent
        window.title = "\(fileName) — marq"
        window.contentView = webView
        window.makeKeyAndOrderFront(nil)

        // Build menu bar
        let mainMenu = NSMenu()

        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About marq", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit marq", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appMenuItem = NSMenuItem()
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        // File menu
        let fileMenu = NSMenu(title: "File")
        let openItem = NSMenuItem(title: "Open…", action: #selector(openFileDialog), keyEquivalent: "o")
        openItem.keyEquivalentModifierMask = .command
        fileMenu.addItem(openItem)
        let fileMenuItem = NSMenuItem()
        fileMenuItem.submenu = fileMenu
        mainMenu.addItem(fileMenuItem)

        // Navigate menu with back/forward (Cmd+Left / Cmd+Right)
        let navMenu = NSMenu(title: "Navigate")
        let backItem = NSMenuItem(title: "Back", action: #selector(navigateBack), keyEquivalent: String(Character(UnicodeScalar(NSLeftArrowFunctionKey)!)))
        backItem.keyEquivalentModifierMask = .command
        navMenu.addItem(backItem)
        let forwardItem = NSMenuItem(title: "Forward", action: #selector(navigateForward), keyEquivalent: String(Character(UnicodeScalar(NSRightArrowFunctionKey)!)))
        forwardItem.keyEquivalentModifierMask = .command
        navMenu.addItem(forwardItem)
        navMenu.addItem(.separator())
        let reloadItem = NSMenuItem(title: "Reload", action: #selector(reloadFile), keyEquivalent: "r")
        reloadItem.keyEquivalentModifierMask = .command
        navMenu.addItem(reloadItem)
        let navMenuItem = NSMenuItem()
        navMenuItem.submenu = navMenu
        mainMenu.addItem(navMenuItem)

        NSApp.mainMenu = mainMenu
        log("Menu bar configured, loading template...")

        // Load template via file URL with read access to entire filesystem.
        // loadFileURL does allow remote (https) resources — the earlier image issue
        // was unrelated to this choice.
        if let templateURL = Bundle.module.url(forResource: "template", withExtension: "html", subdirectory: "Resources") {
            log("Loading template from: \(templateURL.path)")
            webView.loadFileURL(templateURL, allowingReadAccessTo: URL(fileURLWithPath: "/"))
        } else {
            log("ERROR: Could not find template.html in bundle")
        }

        // Push initial file to history
        if !filePath.isEmpty {
            history = [filePath]
            historyIndex = 0
        }

        // Start file watcher
        startWatching()
    }

    func navigateTo(_ path: String, addToHistory: Bool = true) {
        fileWatcher?.stop()
        filePath = path

        if addToHistory {
            // Trim forward history
            if historyIndex < history.count - 1 {
                history = Array(history[0...historyIndex])
            }
            history.append(path)
            historyIndex = history.count - 1
        }

        let fileName = URL(fileURLWithPath: path).lastPathComponent
        window.title = "\(fileName) — marq"

        loadAndInject()
        startWatching()
    }

    @objc func navigateBack() {
        guard historyIndex > 0 else { return }
        historyIndex -= 1
        let path = history[historyIndex]
        fileWatcher?.stop()
        filePath = path
        let fileName = URL(fileURLWithPath: path).lastPathComponent
        window.title = "\(fileName) — marq"
        loadAndInject()
        startWatching()
    }

    @objc func reloadFile() {
        loadAndInject()
    }

    @objc func openFileDialog() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.init(filenameExtension: "md")!, .init(filenameExtension: "markdown")!]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.navigateTo(url.path)
        }
    }

    @objc func navigateForward() {
        guard historyIndex < history.count - 1 else { return }
        historyIndex += 1
        let path = history[historyIndex]
        fileWatcher?.stop()
        filePath = path
        let fileName = URL(fileURLWithPath: path).lastPathComponent
        window.title = "\(fileName) — marq"
        loadAndInject()
        startWatching()
    }

    func startWatching() {
        fileWatcher?.stop()
        guard !filePath.isEmpty else { return }
        let watcher = FileWatcher(path: filePath) { [weak self] in
            self?.loadAndInject()
        }
        watcher.start()
        fileWatcher = watcher
    }

    func loadAndInject() {
        guard !filePath.isEmpty else { log("No file path set"); return }
        log("Loading file: \(filePath)")
        do {
            rawMarkdown = try String(contentsOfFile: filePath, encoding: .utf8)
            log("Read \(rawMarkdown.count) chars")
        } catch {
            log("ERROR reading file: \(error)")
            rawMarkdown = "**Error:** Could not read file `\(filePath)`\n\n\(error.localizedDescription)"
        }
        injectMarkdown()
    }

    func injectMarkdown() {
        log("Injecting markdown (\(rawMarkdown.count) chars)")
        // Resolve relative image paths to absolute file:// URLs
        var md = rawMarkdown
        if !filePath.isEmpty {
            let baseDir = URL(fileURLWithPath: filePath).deletingLastPathComponent().path
            // Match ![alt](path) where path is relative (not http/https/file/data)
            let pattern = try! NSRegularExpression(pattern: #"!\[([^\]]*)\]\((?!https?://|file://|data:)([^)]+)\)"#)
            md = pattern.stringByReplacingMatches(
                in: md,
                range: NSRange(md.startIndex..., in: md),
                withTemplate: "![$1](file://\(baseDir)/$2)"
            )
        }

        let escaped = md
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "`", with: "\\`")
            .replacingOccurrences(of: "$", with: "\\$")
        let js = "renderMarkdown(`\(escaped)`);"
        webView.evaluateJavaScript(js) { [weak self] _, error in
            if let error = error {
                self?.log("JS ERROR: \(error)")
            } else {
                self?.log("Markdown injected successfully")
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }
}

extension AppDelegate: WKNavigationDelegate {
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        log("WebView navigation finished")
        loadAndInject()
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        log("WebView navigation FAILED: \(error)")
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        log("WebView provisional navigation FAILED: \(error)")
    }
}

extension AppDelegate: WKScriptMessageHandler {
    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        if message.name == "openFile" {
            openFileDialog()
            return
        }
        guard message.name == "navigate", let href = message.body as? String else { return }
        log("Navigate request: \(href)")

        // Resolve relative path against current file's directory
        let baseDir = URL(fileURLWithPath: filePath).deletingLastPathComponent()
        let targetURL = baseDir.appendingPathComponent(href)
        let resolved = targetURL.standardized.path

        guard FileManager.default.fileExists(atPath: resolved) else {
            let js = "alert('File not found: \(href)');"
            webView.evaluateJavaScript(js, completionHandler: nil)
            return
        }

        navigateTo(resolved)
    }
}
