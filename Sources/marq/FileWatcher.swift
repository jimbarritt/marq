import Foundation

final class FileWatcher {
    private var source: DispatchSourceFileSystemObject?
    private let path: String
    private let onChange: () -> Void
    private var lastFired: Date = .distantPast
    private let debounceInterval: TimeInterval = 0.3

    init(path: String, onChange: @escaping () -> Void) {
        self.path = path
        self.onChange = onChange
    }

    func start() {
        stop()
        let fd = open(path, O_EVTONLY)
        guard fd >= 0 else {
            print("marq: unable to watch file: \(path)")
            return
        }

        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd,
            eventMask: [.write, .rename, .delete],
            queue: .main
        )

        source.setEventHandler { [weak self] in
            guard let self = self else { return }
            let now = Date()
            guard now.timeIntervalSince(self.lastFired) > self.debounceInterval else { return }
            self.lastFired = now

            let flags = source.data
            if flags.contains(.rename) || flags.contains(.delete) {
                // File was replaced (atomic save) — re-establish watch
                self.source?.cancel()
                self.source = nil
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in
                    self?.start()
                    self?.onChange()
                }
            } else {
                self.onChange()
            }
        }

        source.setCancelHandler {
            close(fd)
        }

        source.resume()
        self.source = source
    }

    func stop() {
        source?.cancel()
        source = nil
    }

    deinit {
        stop()
    }
}
