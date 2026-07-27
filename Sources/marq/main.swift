import AppKit

// Answered before AppKit starts up: --help should not open a window.
if CommandLine.arguments.contains("--help") || CommandLine.arguments.contains("-h") {
    print("""
    marq — markdown viewer

    usage: marq FILE.md [options]

      --export-pdf OUT.pdf    render, export and quit
      --export-png OUT.png    render the whole document to a PNG and quit
      --dump-metrics OUT      report the layout as JSON and quit ("-" for stdout)
      --print                 with --dump-metrics, measure the A4 page rather
                              than the window
      --width N, --height N   window size, so a measurement is reproducible
      --settle SECONDS        how long to let the document settle (default 1.5)
      --timeout SECONDS       watchdog for headless runs (default 60, 0 disables)

    See `just --list` for the recipes built on these.
    """)
    exit(0)
}

// Set process name to "Marq" for the menu bar
let newName = strdup("Marq")!
CommandLine.unsafeArgv[0] = newName
ProcessInfo.processInfo.setValue("Marq", forKey: "processName")

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let delegate = AppDelegate()
app.delegate = delegate

app.run()
