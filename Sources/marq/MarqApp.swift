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
    var pendingExportPath: String?   // --export-pdf <path>, exported then quit
    var isHeadlessExport = false
    var rawMarkdown: String = ""
    // History is a list of *positions*, not files. An anchor jump does not change
    // the file, so a list of paths has nothing to push and Back either does
    // nothing or leaves the document entirely — which is worse than nothing.
    struct HistoryEntry {
        var path: String
        var scrollY: Double
    }
    var history: [HistoryEntry] = []
    var historyIndex: Int = -1
    var pendingScrollRestore: Double?
    var pendingOpenFile: String?
    var statusLabel: NSTextField!
    var statusBar: NSView!
    var resetScrollOnNextInject: Bool = false

    // Text zoom. `pageZoom` scales the whole document — prose, gutter, search
    // box — which is what a reader means by zoom, and it leaves the native
    // status bar alone because that is an AppKit view outside the web view.
    // Fixed steps rather than a multiplier so Cmd-- after Cmd-+ lands back on
    // 100% exactly, and so the sequence matches Safari's.
    static let zoomSteps: [CGFloat] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]
    static let defaultZoomIndex = 4   // 1.0
    static let zoomDefaultsKey = "textZoom"
    var zoomIndex: Int = AppDelegate.defaultZoomIndex

    func log(_ msg: String) {
        let ts = ISO8601DateFormatter().string(from: Date())
        let line = "[marq \(ts)] \(msg)\n"
        FileHandle.standardError.write(Data(line.utf8))
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        log("Starting up")

        // Resolve file path from CLI args
        let args = CommandLine.arguments
        if let i = args.firstIndex(of: "--export-pdf"), i + 1 < args.count {
            pendingExportPath = args[i + 1]
        }
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
        contentController.add(self, name: "anchor")
        config.userContentController = contentController

        log("Creating WKWebView")
        webView = SilentWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self
        if #available(macOS 13.3, *) {
            webView.isInspectable = true
        }

        // Restore the saved zoom level before the first load, so the document
        // is laid out at the right size rather than reflowing after it appears.
        // Set directly rather than via applyZoom() — the template's JS does not
        // exist yet, and it runs layoutTables() itself once it does.
        // `integer(forKey:)` rather than `object(forKey:) as? Int`: a value passed
        // on the command line as `-textZoom 8` lands in the argument domain as a
        // string, and the cast would silently drop it. The nil check keeps an
        // absent key from reading as index 0.
        if UserDefaults.standard.object(forKey: AppDelegate.zoomDefaultsKey) != nil {
            let saved = UserDefaults.standard.integer(forKey: AppDelegate.zoomDefaultsKey)
            zoomIndex = min(max(saved, 0), AppDelegate.zoomSteps.count - 1)
        }
        webView.pageZoom = AppDelegate.zoomSteps[zoomIndex]

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

        // View menu — text zoom
        let viewMenu = NSMenu(title: "View")
        let zoomInItem = NSMenuItem(title: "Zoom In", action: #selector(zoomIn), keyEquivalent: "+")
        zoomInItem.keyEquivalentModifierMask = .command
        viewMenu.addItem(zoomInItem)
        let zoomOutItem = NSMenuItem(title: "Zoom Out", action: #selector(zoomOut), keyEquivalent: "-")
        zoomOutItem.keyEquivalentModifierMask = .command
        viewMenu.addItem(zoomOutItem)
        let zoomResetItem = NSMenuItem(title: "Actual Size", action: #selector(zoomReset), keyEquivalent: "0")
        zoomResetItem.keyEquivalentModifierMask = .command
        viewMenu.addItem(zoomResetItem)
        let viewMenuItem = NSMenuItem()
        viewMenuItem.submenu = viewMenu
        mainMenu.addItem(viewMenuItem)

        // Cmd-+ is really Cmd-Shift-= on most layouts, which the menu item above
        // catches and displays correctly. A bare Cmd-= is what people actually
        // press, and it cannot be a second menu item: a visible duplicate is
        // clutter and a hidden one never fires, because key equivalent matching
        // skips hidden items.
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self = self,
                  event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
                  event.charactersIgnoringModifiers == "=" else { return event }
            self.zoomIn()
            return nil
        }

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
            history = [HistoryEntry(path: filePath, scrollY: 0)]
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

    // Every navigation records where the reader was before it moves them, so
    // Back returns to that spot rather than to the top of the file.
    func withCurrentScroll(_ body: @escaping () -> Void) {
        webView.evaluateJavaScript("window.scrollY") { [weak self] result, _ in
            guard let self = self else { return }
            if self.historyIndex >= 0, self.historyIndex < self.history.count,
               let y = result as? Double {
                self.history[self.historyIndex].scrollY = y
            }
            body()
        }
    }

    func navigateTo(_ path: String, addToHistory: Bool = true) {
        guard addToHistory else {
            openEntry(HistoryEntry(path: path, scrollY: 0))
            return
        }
        withCurrentScroll { [weak self] in
            guard let self = self else { return }
            // Trim forward history
            if self.historyIndex < self.history.count - 1 {
                self.history = Array(self.history[0...self.historyIndex])
            }
            self.history.append(HistoryEntry(path: path, scrollY: 0))
            self.historyIndex = self.history.count - 1
            self.openEntry(self.history[self.historyIndex])
        }
    }

    // Move to a history entry. Within the same document this is an anchor jump,
    // and restoring the offset *is* the whole navigation — reloading would throw
    // the position away and flash the document for no reason.
    func openEntry(_ entry: HistoryEntry) {
        if entry.path == filePath && !filePath.isEmpty {
            webView.evaluateJavaScript("restoreScroll(\(entry.scrollY));", completionHandler: nil)
            return
        }

        fileWatcher?.stop()
        filePath = entry.path

        let fileName = URL(fileURLWithPath: entry.path).lastPathComponent
        window.title = "\(fileName) — marq"
        updateStatusBar(path: entry.path)

        resetScrollOnNextInject = entry.scrollY == 0
        pendingScrollRestore = entry.scrollY == 0 ? nil : entry.scrollY
        loadAndInject()
        startWatching()
    }

    // An anchor jump is handled in the page — the file never changes — so the
    // template reports where it went and the entry is pushed here.
    func recordAnchorJump(from: Double, to: Double) {
        guard historyIndex >= 0, historyIndex < history.count else { return }
        history[historyIndex].scrollY = from
        if historyIndex < history.count - 1 {
            history = Array(history[0...historyIndex])
        }
        history.append(HistoryEntry(path: filePath, scrollY: to))
        historyIndex = history.count - 1
    }

    @objc func navigateBack() {
        guard historyIndex > 0 else { return }
        withCurrentScroll { [weak self] in
            guard let self = self else { return }
            self.historyIndex -= 1
            self.openEntry(self.history[self.historyIndex])
        }
    }

    @objc func reloadFile() {
        loadAndInject()
    }

    // MARK: - Zoom

    @objc func zoomIn() {
        setZoomIndex(zoomIndex + 1)
    }

    @objc func zoomOut() {
        setZoomIndex(zoomIndex - 1)
    }

    @objc func zoomReset() {
        setZoomIndex(AppDelegate.defaultZoomIndex)
    }

    func setZoomIndex(_ index: Int) {
        let clamped = min(max(index, 0), AppDelegate.zoomSteps.count - 1)
        guard clamped != zoomIndex else { return }
        zoomIndex = clamped
        UserDefaults.standard.set(zoomIndex, forKey: AppDelegate.zoomDefaultsKey)
        applyZoom()
    }

    func applyZoom() {
        let zoom = AppDelegate.zoomSteps[zoomIndex]
        log("Zoom set to \(Int(zoom * 100))%")
        webView.pageZoom = zoom
        // Zooming changes the CSS viewport, so the column widths layoutTables()
        // computed against the old one are stale. The template's resize listener
        // does fire, but it is debounced by 100ms and the tables visibly jump in
        // the meantime — re-running here settles them in the same frame.
        webView.evaluateJavaScript("layoutTables(); buildGutter();", completionHandler: nil)
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
        withCurrentScroll { [weak self] in
            guard let self = self else { return }
            self.historyIndex += 1
            self.openEntry(self.history[self.historyIndex])
        }
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
        // Defensive: measured, printOperation(with:) ignores pageZoom entirely —
        // exporting at 175% produces a PDF identical to one at 100%, down to the
        // glyph bounds. Pinned to 1.0 anyway so a WebKit that starts honouring it
        // cannot silently scale every export by whatever the reader last zoomed to.
        webView.pageZoom = 1.0

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
        webView.pageZoom = AppDelegate.zoomSteps[zoomIndex]
        webView.evaluateJavaScript("restoreTableLayoutAfterPrint();", completionHandler: nil)
    }

    @objc func pdfExportDidRun(_ operation: NSPrintOperation, success: Bool, contextInfo: UnsafeMutableRawPointer?) {
        restoreAfterPrint()
        if isHeadlessExport {
            log(success ? "PDF exported successfully" : "ERROR generating PDF")
            NSApp.terminate(nil)
            return
        }
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
                // Back into a *different* file lands at the offset the reader
                // left it at. restoreScroll re-applies after images load, which
                // is when the content stops moving under them.
                if let y = self?.pendingScrollRestore {
                    self?.pendingScrollRestore = nil
                    self?.webView.evaluateJavaScript("restoreScroll(\(y));", completionHandler: nil)
                }
                self?.runPendingExportIfAny()
            }
        }
    }

    // `marq file.md --export-pdf out.pdf` exports without touching the UI and
    // quits, so export can be scripted and checked.
    //
    // The delay is not politeness: the export measures the rendered document, and
    // mermaid, KaTeX and images all settle after renderMarkdown returns. Exporting
    // in the same turn measures a document that is still moving.
    func runPendingExportIfAny() {
        guard let out = pendingExportPath else { return }
        pendingExportPath = nil
        isHeadlessExport = true
        log("Exporting to \(out)")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            let path = out.hasPrefix("/")
                ? out
                : URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                    .appendingPathComponent(out).path
            self.generatePDF(to: URL(fileURLWithPath: path))
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
        if message.name == "anchor" {
            guard let body = message.body as? [String: Any],
                  let from = body["from"] as? Double,
                  let to = body["to"] as? Double else { return }
            log("Anchor jump: \(Int(from)) -> \(Int(to))")
            recordAnchorJump(from: from, to: to)
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
