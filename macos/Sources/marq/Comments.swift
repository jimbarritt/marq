import AppKit
import WebKit

enum CommentsBinary {
    static let environmentKey = "MARQ_COMMENTS_BIN"
    static let name = "marq-comments"

    enum Lookup {
        case disabled
        case missing
        case found(URL)
    }

    static func locate() -> Lookup {
        if let value = ProcessInfo.processInfo.environment[environmentKey] {
            if value.isEmpty { return .disabled }
            return FileManager.default.isExecutableFile(atPath: value) ? .found(URL(fileURLWithPath: value)) : .missing
        }
        var candidates: [URL] = []
        if let executable = Bundle.main.executableURL {
            candidates.append(executable.deletingLastPathComponent().appendingPathComponent(name))
            let resolved = executable.resolvingSymlinksInPath().path
            if let range = resolved.range(of: "/macos/.build/") {
                let root = URL(fileURLWithPath: String(resolved[..<range.lowerBound]))
                for profile in ["debug", "release"] {
                    candidates.append(root.appendingPathComponent("cli/target/\(profile)/\(name)"))
                }
            }
        }
        for candidate in candidates where FileManager.default.isExecutableFile(atPath: candidate.path) {
            return .found(candidate)
        }
        return .missing
    }
}

private final class Once {
    private let lock = NSLock()
    private var done = false

    func claim() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if done { return false }
        done = true
        return true
    }

    var isDone: Bool {
        lock.lock()
        defer { lock.unlock() }
        return done
    }
}

enum SpawnedOutput {
    static func run(_ executable: URL, _ arguments: [String], timeout: TimeInterval = 5, completion: @escaping (Data?) -> Void) {
        DispatchQueue.global(qos: .utility).async {
            let process = Process()
            process.executableURL = executable
            process.arguments = arguments
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            process.standardInput = FileHandle.nullDevice
            let once = Once()
            do {
                try process.run()
            } catch {
                if once.claim() { completion(nil) }
                return
            }
            let pid = process.processIdentifier
            DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + timeout) {
                guard !once.isDone else { return }
                process.terminate()
                if once.claim() { completion(nil) }
                DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 1) {
                    if process.isRunning { kill(pid, SIGKILL) }
                }
            }
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            if once.claim() { completion(process.terminationStatus == 0 ? data : nil) }
        }
    }

    static func gitCommonDirectory(of directory: String, completion: @escaping (String?) -> Void) {
        run(URL(fileURLWithPath: "/usr/bin/git"), ["-C", directory, "rev-parse", "--git-common-dir"]) { data in
            guard let data = data,
                  let text = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !text.isEmpty else {
                completion(nil)
                return
            }
            if text.hasPrefix("/") {
                completion(URL(fileURLWithPath: text).standardized.path)
            } else {
                completion(URL(fileURLWithPath: directory, isDirectory: true).appendingPathComponent(text).standardized.path)
            }
        }
    }
}

final class DirectoryWatcher {
    private var source: DispatchSourceFileSystemObject?
    private let path: String
    private let onChange: () -> Void

    init(path: String, onChange: @escaping () -> Void) {
        self.path = path
        self.onChange = onChange
    }

    @discardableResult
    func start() -> Bool {
        stop()
        let fd = open(path, O_EVTONLY)
        guard fd >= 0 else { return false }
        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd,
            eventMask: [.write, .rename, .delete],
            queue: .main
        )
        source.setEventHandler { [weak self] in self?.onChange() }
        source.setCancelHandler { close(fd) }
        source.resume()
        self.source = source
        return true
    }

    func stop() {
        source?.cancel()
        source = nil
    }

    deinit {
        stop()
    }
}

struct CommentsInput {
    var source: String
    var bom: Bool
    var edits: [[Any]]
    var readable: Bool
}

extension AppDelegate {
    static let commentsShownKey = "commentsShown"
    static let commentNumbersKey = "commentNumbers"
    static let commentsDebounce: TimeInterval = 0.3

    func resolveCommentOptions() {
        let defaults = UserDefaults.standard
        commentsShown = true
        commentNumbers = false
        if !options.isHeadless {
            if defaults.object(forKey: AppDelegate.commentsShownKey) != nil {
                commentsShown = defaults.bool(forKey: AppDelegate.commentsShownKey)
            }
            if defaults.object(forKey: AppDelegate.commentNumbersKey) != nil {
                commentNumbers = defaults.bool(forKey: AppDelegate.commentNumbersKey)
            }
        }
        if let flag = options.commentsShown { commentsShown = flag }
        if let flag = options.commentNumbers { commentNumbers = flag }
    }

    func addCommentMenuItems(to menu: NSMenu) {
        menu.addItem(.separator())
        let shown = NSMenuItem(title: "Show Comments", action: #selector(toggleComments), keyEquivalent: "c")
        shown.keyEquivalentModifierMask = [.command, .shift]
        shown.target = self
        shown.state = commentsShown ? .on : .off
        menu.addItem(shown)
        let numbers = NSMenuItem(title: "Comment Numbers", action: #selector(toggleCommentNumbers), keyEquivalent: "c")
        numbers.keyEquivalentModifierMask = [.command, .option]
        numbers.target = self
        numbers.state = commentNumbers ? .on : .off
        menu.addItem(numbers)
    }

    @objc func toggleComments() {
        commentsShown.toggle()
        UserDefaults.standard.set(commentsShown, forKey: AppDelegate.commentsShownKey)
        applyCommentOptions()
    }

    @objc func toggleCommentNumbers() {
        commentNumbers.toggle()
        UserDefaults.standard.set(commentNumbers, forKey: AppDelegate.commentNumbersKey)
        applyCommentOptions()
    }

    var commentOptionArguments: [String: Any] {
        ["options": ["shown": commentsShown, "numbers": commentNumbers]]
    }

    func applyCommentOptions() {
        log("Comment options: shown \(commentsShown), numbers \(commentNumbers)")
        webView.callAsyncJavaScript(
            "if (typeof setCommentOptions === 'function') { setCommentOptions(options); } return true;",
            arguments: commentOptionArguments, in: nil, in: .page
        ) { [weak self] outcome in
            if case .failure(let error) = outcome { self?.log("JS ERROR setting comment options: \(error)") }
        }
    }

    func recordCommentsInput(source: String, edits: [[Any]]) {
        commentsGeneration += 1
        commentsInput = CommentsInput(source: source, bom: fileHasBOM, edits: edits, readable: fileReadable)
        lastDeliveredPayload = nil
        commentsRefreshWork?.cancel()
        commentsRefreshWork = nil
    }

    func spawnComments() {
        guard let input = commentsInput else { return }
        let generation = commentsGeneration
        guard input.readable, !filePath.isEmpty else {
            noteCommentsOutcome(generation, "file not readable")
            return
        }
        switch CommentsBinary.locate() {
        case .disabled:
            noteCommentsOutcome(generation, "disabled by \(CommentsBinary.environmentKey)")
        case .missing:
            noteCommentsOutcome(generation, "no \(CommentsBinary.name) binary found")
        case .found(let binary):
            let file = filePath
            let directory = URL(fileURLWithPath: file).deletingLastPathComponent().path
            log("Spawning \(binary.path) -C \(directory) list \(file) --json")
            commentsInFlight += 1
            SpawnedOutput.run(binary, ["-C", directory, "list", file, "--json"]) { [weak self] data in
                DispatchQueue.main.async {
                    self?.commentsInFlight -= 1
                    self?.receiveComments(data, generation: generation)
                }
            }
        }
    }

    func receiveComments(_ data: Data?, generation: Int) {
        guard generation == commentsGeneration else {
            log("Comments result discarded: a newer inject ran")
            return
        }
        guard let data = data else {
            noteCommentsOutcome(generation, "the CLI failed, timed out or exited non-zero")
            return
        }
        guard let input = commentsInput,
              let threads = (try? JSONSerialization.jsonObject(with: data)) as? [Any] else {
            noteCommentsOutcome(generation, "the CLI output is not a JSON array")
            return
        }
        let payload: [String: Any] = [
            "threads": threads,
            "source": input.source,
            "bom": input.bom,
            "edits": input.edits,
        ]
        guard let encoded = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .withoutEscapingSlashes]),
              let text = String(data: encoded, encoding: .utf8) else {
            noteCommentsOutcome(generation, "the payload cannot be encoded")
            return
        }
        guard text != lastDeliveredPayload else {
            noteCommentsOutcome(generation, "unchanged since the last delivery")
            return
        }
        lastDeliveredPayload = text
        log("Comments payload: \(threads.count) threads, \(input.edits.count) edits, bom \(input.bom), \(text.utf8.count) bytes")
        var arguments = commentOptionArguments
        arguments["payload"] = text
        let script = """
        if (typeof applyComments !== 'function') { return false; }
        await applyComments(payload);
        if (typeof setCommentOptions === 'function') { setCommentOptions(options); }
        return true;
        """
        commentsDelivering += 1
        webView.callAsyncJavaScript(script, arguments: arguments, in: nil, in: .page) { [weak self] outcome in
            guard let self = self else { return }
            self.commentsDelivering -= 1
            switch outcome {
            case .success(let value): self.log("Comments delivered: \(value)")
            case .failure(let error): self.log("JS ERROR applying comments: \(error)")
            }
            self.noteCommentsOutcome(generation, nil)
        }
    }

    func noteCommentsOutcome(_ generation: Int, _ reason: String?) {
        if let reason = reason { log("No comments: \(reason)") }
        guard generation == commentsGeneration else { return }
        commentsOutcomeSeen = true
        runHeadlessTaskIfAny()
    }

    var commentsBusy: Bool {
        commentsRefreshWork != nil || commentsInFlight > 0 || commentsDelivering > 0
    }

    func whenCommentsIdle(limit: TimeInterval = 8, _ body: @escaping () -> Void) {
        let deadline = Date().addingTimeInterval(limit)
        func poll() {
            if !commentsBusy || Date() >= deadline {
                body()
            } else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { poll() }
            }
        }
        poll()
    }

    func scheduleCommentsRefresh() {
        commentsRefreshWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.commentsRefreshWork = nil
            self?.log("Comments refresh after a ref event")
            self?.spawnComments()
        }
        commentsRefreshWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + AppDelegate.commentsDebounce, execute: work)
    }

    func stopCommentWatchers() {
        commentWatchToken += 1
        commentWatchers.forEach { $0.stop() }
        commentWatchers = []
    }

    func startCommentWatchers() {
        stopCommentWatchers()
        guard !filePath.isEmpty else { return }
        let token = commentWatchToken
        let directory = URL(fileURLWithPath: filePath).deletingLastPathComponent().path
        SpawnedOutput.gitCommonDirectory(of: directory) { [weak self] common in
            DispatchQueue.main.async {
                guard let self = self, token == self.commentWatchToken else { return }
                guard let common = common else {
                    self.log("Comment watch: not in a git repository")
                    return
                }
                var paths = [common + "/refs/heads"]
                let reftable = common + "/reftable"
                if FileManager.default.fileExists(atPath: reftable) { paths.append(reftable) }
                for path in paths {
                    let watcher = DirectoryWatcher(path: path) { [weak self] in self?.scheduleCommentsRefresh() }
                    if watcher.start() {
                        self.commentWatchers.append(watcher)
                        self.log("Watching refs: \(path)")
                    } else {
                        self.log("Comment watch: cannot open \(path)")
                    }
                }
            }
        }
    }
}

extension AppDelegate: NSMenuItemValidation {
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(toggleComments) { item.state = commentsShown ? .on : .off }
        if item.action == #selector(toggleCommentNumbers) { item.state = commentNumbers ? .on : .off }
        return true
    }
}
