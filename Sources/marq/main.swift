import AppKit

// Set process name to "Marq" for the menu bar
let newName = strdup("Marq")!
CommandLine.unsafeArgv[0] = newName
ProcessInfo.processInfo.setValue("Marq", forKey: "processName")

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let delegate = AppDelegate()
app.delegate = delegate

app.run()
