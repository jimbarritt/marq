import AppKit

ProcessInfo.processInfo.setValue("Marq", forKey: "processName")

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let delegate = AppDelegate()
app.delegate = delegate

app.run()
