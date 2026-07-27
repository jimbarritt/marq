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
    var pendingOpenFile: String?
    var statusLabel: NSTextField!
    var statusBar: NSView!
    var resetScrollOnNextInject: Bool = false

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
        if #available(macOS 13.3, *) {
            webView.isInspectable = true
        }

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

        // Build content view: webView stacked above a status bar
        let containerView = NSView(frame: NSRect(x: 0, y: 0, width: windowWidth, height: windowHeight))
        containerView.autoresizingMask = [.width, .height]

        let statusBarHeight: CGFloat = 22

        // Status bar
        statusBar = NSView(frame: NSRect(x: 0, y: 0, width: windowWidth, height: statusBarHeight))
        statusBar.autoresizingMask = [.width]
        statusBar.wantsLayer = true
        let statusLayer = CALayer()
        statusLayer.backgroundColor = NSColor(red: 0.98, green: 0.98, blue: 0.98, alpha: 1.0).cgColor
        statusBar.layer = statusLayer

        // Top separator line
        let separator = NSBox(frame: NSRect(x: 0, y: statusBarHeight - 1, width: windowWidth, height: 1))
        separator.autoresizingMask = [.width]
        separator.boxType = .separator
        statusBar.addSubview(separator)

        // Filename label
        statusLabel = NSTextField(frame: NSRect(x: 8, y: 3, width: windowWidth - 16, height: 16))
        statusLabel.autoresizingMask = [.width]
        statusLabel.isEditable = false
        statusLabel.isSelectable = false
        statusLabel.isBordered = false
        statusLabel.drawsBackground = false
        statusLabel.font = NSFont.systemFont(ofSize: 11)
        statusLabel.textColor = NSColor.darkGray
        statusLabel.stringValue = filePath.isEmpty ? "" : filePath
        statusBar.addSubview(statusLabel)

        statusBar.frame = NSRect(x: 0, y: 0, width: windowWidth, height: statusBarHeight)
        containerView.addSubview(statusBar)

        // WebView sits above the status bar
        webView.frame = NSRect(x: 0, y: statusBarHeight, width: windowWidth, height: windowHeight - statusBarHeight)
        webView.autoresizingMask = [.width, .height]
        containerView.addSubview(webView)

        window.contentView = containerView
        // Set layer background after view is in hierarchy so layer is guaranteed non-nil
        statusBar.layer?.backgroundColor = NSColor(red: 0.98, green: 0.98, blue: 0.98, alpha: 1.0).cgColor
        window.makeKeyAndOrderFront(nil)
        // Focus not working reliably without ignoringOtherApps — removed

        // Build menu bar
        let mainMenu = NSMenu()

        let appMenu = NSMenu(title: "Marq")
        appMenu.addItem(withTitle: "About Marq", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit Marq", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appMenuItem = NSMenuItem()
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        // File menu
        let fileMenu = NSMenu(title: "File")
        let openItem = NSMenuItem(title: "Open…", action: #selector(openFileDialog), keyEquivalent: "o")
        openItem.keyEquivalentModifierMask = .command
        fileMenu.addItem(openItem)
        let exportItem = NSMenuItem(title: "Export as PDF…", action: #selector(exportPDF), keyEquivalent: "e")
        exportItem.keyEquivalentModifierMask = .command
        fileMenu.addItem(exportItem)
        let closeItem = NSMenuItem(title: "Close", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "w")
        closeItem.keyEquivalentModifierMask = .command
        fileMenu.addItem(closeItem)
        let fileMenuItem = NSMenuItem()
        fileMenuItem.submenu = fileMenu
        mainMenu.addItem(fileMenuItem)

        // Edit menu — read-only viewer, stripped to copy/select only.
        // NSMenuDelegate removes any items macOS injects (e.g. Writing Tools on Sequoia).
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editMenu.delegate = self
        let editMenuItem = NSMenuItem()
        editMenuItem.submenu = editMenu
        mainMenu.addItem(editMenuItem)

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

        // Window menu
        let windowMenu = NSMenu(title: "Window")
        let minimizeItem = NSMenuItem(title: "Minimize", action: #selector(NSWindow.miniaturize(_:)), keyEquivalent: "m")
        minimizeItem.keyEquivalentModifierMask = .command
        windowMenu.addItem(minimizeItem)
        let zoomItem = NSMenuItem(title: "Zoom", action: #selector(NSWindow.zoom(_:)), keyEquivalent: "")
        windowMenu.addItem(zoomItem)
        windowMenu.addItem(.separator())
        let fullScreenItem = NSMenuItem(title: "Enter Full Screen", action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
        fullScreenItem.keyEquivalentModifierMask = [.control, .command]
        windowMenu.addItem(fullScreenItem)
        windowMenu.addItem(.separator())
        let bringAllItem = NSMenuItem(title: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        windowMenu.addItem(bringAllItem)
        let windowMenuItem = NSMenuItem()
        windowMenuItem.submenu = windowMenu
        mainMenu.addItem(windowMenuItem)
        NSApp.windowsMenu = windowMenu

        NSApp.mainMenu = mainMenu
        log("Menu bar configured, loading template...")

        // Load template via file URL with read access to entire filesystem.
        // loadFileURL does allow remote (https) resources — the earlier image issue
        // was unrelated to this choice.
        //
        // Bundle.module (SPM-generated) looks at Bundle.main.bundleURL/marq_marq.bundle
        // which resolves to Marq.app/marq_marq.bundle — wrong. The bundle lives in
        // Marq.app/Contents/Resources/ so we find it via resourceURL instead.
        if let templateURL = findTemplateURL() {
            log("Loading template from: \(templateURL.path)")
            webView.loadFileURL(templateURL, allowingReadAccessTo: URL(fileURLWithPath: "/"))
        } else {
            log("ERROR: Could not find template.html in bundle")
        }

        // If a file was passed via open -a (Apple Event), use it
        if filePath.isEmpty, let pending = pendingOpenFile {
            filePath = pending
            pendingOpenFile = nil
            log("Using file from open -a: \(filePath)")
        }

        // Push initial file to history
        if !filePath.isEmpty {
            history = [filePath]
            historyIndex = 0
            let fileName = URL(fileURLWithPath: filePath).lastPathComponent
            window.title = "\(fileName) — Marq"
            updateStatusBar(path: filePath)
            resetScrollOnNextInject = true
        }

        // Start file watcher
        startWatching()
    }

    func updateStatusBar(path: String) {
        statusLabel?.stringValue = path
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
        updateStatusBar(path: path)

        resetScrollOnNextInject = true
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
        updateStatusBar(path: path)
        resetScrollOnNextInject = true
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
        updateStatusBar(path: path)
        resetScrollOnNextInject = true
        loadAndInject()
        startWatching()
    }

    @objc func exportPDF() {
        guard !filePath.isEmpty else {
            let alert = NSAlert()
            alert.messageText = "No file open"
            alert.informativeText = "Open a markdown file before exporting to PDF."
            alert.addButton(withTitle: "OK")
            alert.runModal()
            return
        }

        let panel = NSSavePanel()
        let defaultName = URL(fileURLWithPath: filePath).deletingPathExtension().lastPathComponent
        panel.nameFieldStringValue = defaultName + ".pdf"
        panel.allowedContentTypes = [.pdf]

        panel.begin { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.generatePDF(to: url)
        }
    }

    // Export via the print system rather than WKWebView.createPDF().
    //
    // createPDF renders the entire scroll height onto a single page — a 20-page
    // document comes out as one enormous sheet scaled down to fit, which is not
    // a usable PDF. printOperation(with:) runs the same pagination the Print
    // dialog uses, so the document breaks across pages and honours the @page and
    // break-inside rules in template.html.
    func generatePDF(to url: URL) {
        let info = NSPrintInfo()
        info.jobDisposition = .save
        info.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = url as NSURL
        info.horizontalPagination = .fit
        info.verticalPagination = .automatic
        info.isHorizontallyCentered = false
        info.isVerticallyCentered = false
        info.topMargin = 36
        info.bottomMargin = 36
        info.leftMargin = 36
        info.rightMargin = 36

        let printableWidth = info.paperSize.width - info.leftMargin - info.rightMargin
        let printableHeight = info.paperSize.height - info.topMargin - info.bottomMargin

        // Tables are sized in pixels against the window, which is meaningless on
        // paper. Re-run the layout against the printable width before handing
        // the view to the print system, then restore it afterwards.
        webView.evaluateJavaScript("layoutTablesForPrint(\(printableWidth), \(printableHeight));") { [weak self] _, error in
            guard let self = self else { return }
            if let error = error {
                self.log("Error preparing tables for print: \(error)")
            }

            let operation = self.webView.printOperation(with: info)
            operation.showsPrintPanel = false
            operation.showsProgressPanel = false
            // The print operation lays the web view out at this size, so it must
            // be the printable area rather than the window size.
            operation.view?.frame = NSRect(x: 0, y: 0, width: printableWidth, height: printableHeight)

            if let window = self.webView.window {
                operation.runModal(
                    for: window,
                    delegate: self,
                    didRun: #selector(self.pdfExportDidRun(_:success:contextInfo:)),
                    contextInfo: nil
                )
            } else {
                operation.run()
                self.log("PDF exported to: \(url.path)")
                self.restoreAfterPrint()
            }
        }
    }

    func restoreAfterPrint() {
        webView.evaluateJavaScript("restoreTableLayoutAfterPrint();", completionHandler: nil)
    }

    @objc func pdfExportDidRun(_ operation: NSPrintOperation, success: Bool, contextInfo: UnsafeMutableRawPointer?) {
        restoreAfterPrint()
        if success {
            log("PDF exported successfully")
        } else {
            log("ERROR generating PDF")
            let alert = NSAlert()
            alert.messageText = "Export failed"
            alert.informativeText = "Could not generate the PDF."
            alert.addButton(withTitle: "OK")
            alert.runModal()
        }
    }

    func findTemplateURL() -> URL? {
        let candidates: [URL?] = [
            // Installed .app: Marq.app/Contents/Resources/marq_marq.bundle
            Bundle.main.resourceURL?.appendingPathComponent("marq_marq.bundle"),
            // Development: binary sits next to the bundle in .build/
            URL(fileURLWithPath: CommandLine.arguments[0])
                .deletingLastPathComponent()
                .appendingPathComponent("marq_marq.bundle"),
        ]
        for case let bundleURL? in candidates {
            if let bundle = Bundle(url: bundleURL),
               let url = bundle.url(forResource: "template", withExtension: "html", subdirectory: "Resources") {
                return url
            }
        }
        return nil
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
        // Resolve relative image paths to absolute file:// URLs with a cache-busting
        // timestamp so WKWebView always re-reads from disk on each inject.
        var md = rawMarkdown
        if !filePath.isEmpty {
            let baseDir = URL(fileURLWithPath: filePath).deletingLastPathComponent().path
            let cacheBuster = Int(Date().timeIntervalSince1970 * 1000)
            // Match ![alt](path) where path is relative (not http/https/file/data)
            let pattern = try! NSRegularExpression(pattern: #"!\[([^\]]*)\]\((?!https?://|file://|data:)([^)]+)\)"#)
            md = pattern.stringByReplacingMatches(
                in: md,
                range: NSRange(md.startIndex..., in: md),
                withTemplate: "![$1](file://\(baseDir)/$2?t=\(cacheBuster))"
            )
        }

        let escaped = md
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "`", with: "\\`")
            .replacingOccurrences(of: "$", with: "\\$")
        let shouldReset = resetScrollOnNextInject
        resetScrollOnNextInject = false
        let js = "renderMarkdown(`\(escaped)`, \(shouldReset ? "true" : "false"));"
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

    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        if let path = filenames.first, path.hasSuffix(".md") || path.hasSuffix(".markdown") || path.hasSuffix(".mdown") || path.hasSuffix(".mkd") {
            log("Opened via file association: \(path)")
            if webView != nil {
                navigateTo(path)
            } else {
                // WebView not ready yet — defer until applicationDidFinishLaunching completes
                pendingOpenFile = path
            }
        }
        sender.reply(toOpenOrPrint: .success)
    }
}

extension AppDelegate: WKNavigationDelegate {
    // Without this, clicking an external link navigates the web view itself to
    // the site, replacing the rendered document — and didFinish then tries to
    // inject markdown into a page that has no renderMarkdown. External links
    // belong in the user's browser, so cancel the navigation and hand them over.
    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
        guard navigationAction.navigationType == .linkActivated,
              let url = navigationAction.request.url else {
            decisionHandler(.allow)
            return
        }

        if let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" {
            log("Opening external link: \(url.absoluteString)")
            NSWorkspace.shared.open(url)
            decisionHandler(.cancel)
            return
        }

        decisionHandler(.allow)
    }

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

        let trimmed = href.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed == "back" { navigateBack(); return }
        if trimmed == "forward" { navigateForward(); return }

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

extension AppDelegate: NSMenuDelegate {
    func menuWillOpen(_ menu: NSMenu) {
        let allowed = ["Copy", "Select All"]
        for item in menu.items.reversed() {
            if !allowed.contains(item.title) {
                menu.removeItem(item)
            }
        }
    }
}
