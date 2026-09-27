import AppKit
import PDFKit

// Measurement for exported PDFs.
//
// Judging an export by eye does not work — a table that looked full width turned
// out to be at 80% of the measure, twice, in separate sessions. Every number
// here was previously obtained by writing a one-off `swiftc` script into a
// scratchpad that is deleted at the end of the session; three of them were
// written twice, in different sessions, from scratch. This is the same code,
// checked in, built by `swift build` alongside the app.

func fail(_ msg: String) -> Never {
    FileHandle.standardError.write(Data("pdftool: \(msg)\n".utf8))
    exit(1)
}

func emit(_ value: Any) {
    guard let data = try? JSONSerialization.data(
        withJSONObject: value, options: [.prettyPrinted, .sortedKeys]) else {
        fail("could not serialise result")
    }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

func round2(_ v: CGFloat) -> Double { (Double(v) * 100).rounded() / 100 }

func open(_ path: String) -> PDFDocument {
    guard let doc = PDFDocument(url: URL(fileURLWithPath: path)) else {
        fail("could not open \(path)")
    }
    return doc
}

/// Every character on a page, with its bounds in points.
///
/// `characterBounds(at:)` is not indexed by the same thing `page.string` is: the
/// line breaks in the string have no glyph, so the bounds index runs one behind
/// for every newline already passed. Feeding it the string index therefore
/// returns the wrong glyph's rectangle, drifting further down the page as you
/// go — the text comes back looking like a shuffled crossword, and any position
/// read off it is a confident lie.
func characters(_ page: PDFPage) -> [(String, CGRect)] {
    guard let text = page.string else { return [] }
    var out: [(String, CGRect)] = []
    var boundsIndex = 0
    for char in text {
        if char.isNewline {
            out.append((String(char), .zero))
            continue
        }
        out.append((String(char), page.characterBounds(at: boundsIndex)))
        boundsIndex += 1
    }
    return out
}

/// Group characters into lines by their vertical position.
///
/// Not by rounding the midpoint to a fixed grid: a descender sits 2pt below its
/// neighbours and an em-dash 3pt above, so a fixed grid shears one line of prose
/// into three, and the reassembled "line" is a scramble of every third letter.
/// Cluster on the glyph bottom with a tolerance scaled to the type size instead,
/// which handles the 7pt text a shrunken table prints at as well as body prose.
func lines(_ page: PDFPage) -> [[(String, CGRect)]] {
    let chars = characters(page)
        .filter { !$0.0.isEmpty && $0.0 != "\n" && !$0.1.isEmpty }
        .sorted { $0.1.minY > $1.1.minY }
    // Nearest open cluster rather than a single running one: across a table row
    // the cells' descenders trail several points behind their line-mates, and a
    // walk that closes a cluster as soon as it sees a lower glyph leaves them
    // stranded in a line of their own — "g g , , g p" under the row they belong
    // to.
    // Chain on the lowest glyph in the cluster so far, not on its first. One
    // line spans about 5pt of glyph bottoms — an apostrophe rides 3pt above
    // x-height, a descender drops 1.5pt below it — which is wider than any
    // tolerance that still separates two lines of 7pt table text. Anchoring on
    // whichever glyph happened to come first therefore measures every other
    // glyph from a mark that may be at either extreme of the line.
    var clusters: [(low: CGFloat, chars: [(String, CGRect)])] = []
    for c in chars {
        let tolerance = max(4, c.1.height * 0.6)
        if let i = clusters.indices.last(where: { clusters[$0].low - c.1.minY <= tolerance }) {
            clusters[i].chars.append(c)
            clusters[i].low = min(clusters[i].low, c.1.minY)
        } else {
            clusters.append((low: c.1.minY, chars: [c]))
        }
    }
    return clusters.map { $0.chars.sorted { $0.1.minX < $1.1.minX } }
}

/// Reassemble a line's text. Spaces have zero-width bounds and are dropped by
/// `characters`, so put them back wherever the gap is wide enough to be one.
func lineText(_ line: [(String, CGRect)]) -> String {
    var text = ""
    var previous: CGRect?
    for (char, rect) in line {
        if let p = previous, rect.minX - p.maxX > max(1, rect.height * 0.18) { text += " " }
        text += char
        previous = rect
    }
    return text
}

func render(_ page: PDFPage, scale: CGFloat) -> NSBitmapImageRep? {
    let box = page.bounds(for: .mediaBox)
    let w = Int(box.width * scale), h = Int(box.height * scale)
    guard w > 0, h > 0,
          let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
          let ctx = NSGraphicsContext(bitmapImageRep: rep) else { return nil }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = ctx
    NSColor.white.setFill()
    NSRect(x: 0, y: 0, width: CGFloat(w), height: CGFloat(h)).fill()
    ctx.cgContext.scaleBy(x: scale, y: scale)
    ctx.cgContext.translateBy(x: -box.minX, y: -box.minY)
    page.draw(with: .mediaBox, to: ctx.cgContext)
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write(Data("""
    usage: pdftool <command> FILE.pdf [args]

      info FILE.pdf                  page count, page sizes, per-page text extent
      pages FILE.pdf OUTDIR [N [M]]  render pages N..M to PNGs (1-based, default all)
      text FILE.pdf [N]              lines of text with x positions
      chars FILE.pdf N [MATCH]       per-character bounds, optionally only near MATCH
      vlines FILE.pdf N              vertical rules — i.e. table column borders

    """.utf8))
    exit(64)
}

let command = args[1]
let doc = open(args[2])
let pageArg = args.count > 3 ? Int(args[3]) : nil

switch command {

// Is the table filling the measure? A page's text extent against its printable
// width answers that in one number, without opening anything.
case "info":
    var pages: [[String: Any]] = []
    for i in 0..<doc.pageCount {
        guard let page = doc.page(at: i) else { continue }
        let box = page.bounds(for: .mediaBox)
        let boxes = characters(page).map { $0.1 }.filter { !$0.isEmpty }
        var entry: [String: Any] = [
            "page": i + 1,
            "widthPt": round2(box.width),
            "heightPt": round2(box.height),
            "characters": boxes.count
        ]
        if let minX = boxes.map({ $0.minX }).min(),
           let maxX = boxes.map({ $0.maxX }).max(),
           let minY = boxes.map({ $0.minY }).min(),
           let maxY = boxes.map({ $0.maxY }).max() {
            entry["textLeftPt"] = round2(minX)
            entry["textRightPt"] = round2(maxX)
            entry["textWidthPt"] = round2(maxX - minX)
            entry["textTopPt"] = round2(box.height - maxY)
            entry["textBottomPt"] = round2(box.height - minY)
        }
        pages.append(entry)
    }
    emit(["path": args[2], "pageCount": doc.pageCount, "pages": pages])

case "pages":
    guard args.count > 3 else { fail("pages needs an output directory") }
    let dir = args[3]
    let from = (args.count > 4 ? Int(args[4]) : 1) ?? 1
    let to = (args.count > 5 ? Int(args[5]) : doc.pageCount) ?? doc.pageCount
    try? FileManager.default.createDirectory(
        atPath: dir, withIntermediateDirectories: true)
    var written: [String] = []
    for n in from...max(from, to) where n >= 1 && n <= doc.pageCount {
        guard let page = doc.page(at: n - 1),
              let rep = render(page, scale: 2),
              let png = rep.representation(using: .png, properties: [:]) else { continue }
        let out = "\(dir)/page-\(String(format: "%02d", n)).png"
        try? png.write(to: URL(fileURLWithPath: out))
        written.append(out)
    }
    emit(["written": written])

case "text":
    var out: [[String: Any]] = []
    let range = pageArg.map { ($0 - 1)...($0 - 1) } ?? 0...(max(0, doc.pageCount - 1))
    for i in range where i >= 0 && i < doc.pageCount {
        guard let page = doc.page(at: i) else { continue }
        for line in lines(page) {
            guard let first = line.first, let last = line.last else { continue }
            out.append([
                "page": i + 1,
                "y": round2(first.1.midY),
                "x": round2(first.1.minX),
                "right": round2(last.1.maxX),
                "text": lineText(line)
            ])
        }
    }
    emit(["lines": out])

// Per-glyph bounds. This is how a broken word — "Manifes/t" — is found without
// squinting at a render: the fragment is on the page, at a known x.
case "chars":
    guard let n = pageArg, let page = doc.page(at: n - 1) else { fail("chars needs a valid page number") }
    let match = args.count > 4 ? args[4] : nil
    var out: [[String: Any]] = []
    let all = characters(page)
    let text = page.string ?? ""
    var wanted: Set<Int>? = nil
    if let match = match {
        var indices = Set<Int>()
        var search = text.startIndex
        while let r = text.range(of: match, range: search..<text.endIndex) {
            let start = text.distance(from: text.startIndex, to: r.lowerBound)
            let len = text.distance(from: r.lowerBound, to: r.upperBound)
            for k in start..<(start + len) { indices.insert(k) }
            search = r.upperBound
        }
        wanted = indices
    }
    for (i, c) in all.enumerated() where wanted?.contains(i) ?? true {
        if c.1.isEmpty { continue }
        out.append([
            "index": i, "char": c.0,
            "x": round2(c.1.minX), "right": round2(c.1.maxX),
            "y": round2(c.1.minY), "width": round2(c.1.width),
            "height": round2(c.1.height)
        ])
    }
    emit(["page": n, "match": match ?? "", "characters": out])

// Column borders, found by rasterising and looking for tall runs of dark pixels.
// Column widths as printed, rather than as the layout intended them — the two
// disagreeing is what the print work kept being about.
case "vlines":
    guard let n = pageArg, let page = doc.page(at: n - 1),
          let rep = render(page, scale: 2) else { fail("vlines needs a valid page number") }
    let w = rep.pixelsWide, h = rep.pixelsHigh
    let minRun = h / 20   // a rule shorter than a twentieth of the page is not a table border
    var runs: [[String: Any]] = []
    var x = 0
    while x < w {
        var best = 0, current = 0, topOfBest = 0
        for y in 0..<h {
            // Table borders are a light grey (#d0d7de), not black — a threshold
            // set for ink finds no rules at all on a real page.
            let dark = (rep.colorAt(x: x, y: y)?.brightnessComponent ?? 1) < 0.93
            if dark {
                current += 1
                if current > best { best = current; topOfBest = y - current + 1 }
            } else {
                current = 0
            }
        }
        if best >= minRun {
            runs.append([
                "xPt": round2(CGFloat(x) / 2),
                "lengthPt": round2(CGFloat(best) / 2),
                "topPt": round2(CGFloat(topOfBest) / 2)
            ])
        }
        x += 1
    }
    // Adjacent pixel columns belong to one rule; keep the leftmost of each group.
    var merged: [[String: Any]] = []
    for run in runs {
        let xPt = run["xPt"] as! Double
        if let last = merged.last, xPt - (last["xPt"] as! Double) < 1.5 { continue }
        merged.append(run)
    }
    emit([
        "page": n,
        "rules": merged,
        "gapsPt": zip(merged.dropFirst(), merged).map {
            round2(CGFloat(($0["xPt"] as! Double) - ($1["xPt"] as! Double)))
        }
    ])

default:
    fail("unknown command \(command)")
}
