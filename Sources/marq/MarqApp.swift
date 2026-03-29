import AppKit
import WebKit

class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var webView: WKWebView!
    var fileWatcher: FileWatcher?
    var filePath: String = ""
    var rawMarkdown: String = ""

    func applicationDidFinishLaunching(_ notification: Notification) {
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

        // Configure WKWebView
        let config = WKWebViewConfiguration()
        config.preferences.setValue(true, forKey: "allowFileAccessFromFileURLs")
        webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self

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

        let fileName = filePath.isEmpty ? "marq" : URL(fileURLWithPath: filePath).lastPathComponent
        window.title = "\(fileName) — marq"
        window.contentView = webView
        window.makeKeyAndOrderFront(nil)

        // Build menu bar with Quit item
        let mainMenu = NSMenu()

        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About marq", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit marq", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

        let appMenuItem = NSMenuItem()
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        NSApp.mainMenu = mainMenu

        // Load template HTML
        if let templateURL = Bundle.module.url(forResource: "template", withExtension: "html", subdirectory: "Resources") {
            let baseURL = filePath.isEmpty ? templateURL.deletingLastPathComponent() : URL(fileURLWithPath: filePath).deletingLastPathComponent()
            let templateHTML = (try? String(contentsOf: templateURL, encoding: .utf8)) ?? ""
            webView.loadHTMLString(templateHTML, baseURL: baseURL)
        }

        // Start file watcher
        if !filePath.isEmpty {
            let watcher = FileWatcher(path: filePath) { [weak self] in
                self?.loadAndInject()
            }
            watcher.start()
            fileWatcher = watcher
        }
    }

    func loadAndInject() {
        guard !filePath.isEmpty else { return }
        do {
            rawMarkdown = try String(contentsOfFile: filePath, encoding: .utf8)
        } catch {
            rawMarkdown = "**Error:** Could not read file `\(filePath)`\n\n\(error.localizedDescription)"
        }
        injectMarkdown()
    }

    func injectMarkdown() {
        let escaped = rawMarkdown
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "`", with: "\\`")
            .replacingOccurrences(of: "$", with: "\\$")
        let js = "renderMarkdown(`\(escaped)`);"
        webView.evaluateJavaScript(js, completionHandler: nil)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return true
    }
}

extension AppDelegate: WKNavigationDelegate {
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // Template loaded, now inject markdown
        loadAndInject()
    }
}

