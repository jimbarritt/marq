// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "marq",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "marq",
            exclude: ["Info.plist"],
            resources: [.copy("Resources")]
        ),
        // Measurement for exported PDFs. Part of the package so `swift build`
        // builds it with the app — the harness recipes that use it therefore
        // cannot be run against a stale binary.
        .executableTarget(name: "pdftool")
    ]
)
