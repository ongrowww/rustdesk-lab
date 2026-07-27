import AppKit
import Foundation

enum IconError: Error, CustomStringConvertible {
    case usage
    case sourceImage
    case bitmap(Int)
    case png(Int)
    case iconutil(Int32)

    var description: String {
        switch self {
        case .usage:
            return "usage: generate_macos_icon.swift <mark.png> <output.icns> <preview.png>"
        case .sourceImage:
            return "failed to load source mark"
        case .bitmap(let size):
            return "failed to create \(size)x\(size) bitmap"
        case .png(let size):
            return "failed to encode \(size)x\(size) PNG"
        case .iconutil(let status):
            return "iconutil failed with exit status \(status)"
        }
    }
}

let violet = NSColor(
    srgbRed: 0x75 / 255.0,
    green: 0x16 / 255.0,
    blue: 0xF8 / 255.0,
    alpha: 1.0
)
let lime = NSColor(
    srgbRed: 0xDB / 255.0,
    green: 0xF8 / 255.0,
    blue: 0x7C / 255.0,
    alpha: 1.0
)

func renderIcon(mark: NSImage, size: Int) throws -> Data {
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: size,
        pixelsHigh: size,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    ) else {
        throw IconError.bitmap(size)
    }

    bitmap.size = NSSize(width: size, height: size)
    guard let graphicsContext = NSGraphicsContext(bitmapImageRep: bitmap) else {
        throw IconError.bitmap(size)
    }

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = graphicsContext
    graphicsContext.imageInterpolation = .high

    let canvas = NSRect(x: 0, y: 0, width: size, height: size)
    let inset = CGFloat(size) * 0.035
    let tile = canvas.insetBy(dx: inset, dy: inset)
    let cornerRadius = CGFloat(size) * 0.205
    let tilePath = NSBezierPath(roundedRect: tile, xRadius: cornerRadius, yRadius: cornerRadius)

    violet.setFill()
    tilePath.fill()

    NSGraphicsContext.saveGraphicsState()
    tilePath.addClip()
    lime.setFill()
    NSBezierPath(
        rect: NSRect(
            x: tile.minX,
            y: tile.minY,
            width: tile.width,
            height: CGFloat(size) * 0.075
        )
    ).fill()
    NSGraphicsContext.restoreGraphicsState()

    let markWidth = CGFloat(size) * 0.72
    let markHeight = markWidth
    let markRect = NSRect(
        x: (CGFloat(size) - markWidth) / 2,
        y: (CGFloat(size) - markHeight) / 2 + CGFloat(size) * 0.025,
        width: markWidth,
        height: markHeight
    )
    mark.draw(
        in: markRect,
        from: .zero,
        operation: .sourceOver,
        fraction: 1.0,
        respectFlipped: true,
        hints: [.interpolation: NSImageInterpolation.high]
    )

    graphicsContext.flushGraphics()
    NSGraphicsContext.restoreGraphicsState()

    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        throw IconError.png(size)
    }
    return png
}

do {
    guard CommandLine.arguments.count == 4 else {
        throw IconError.usage
    }

    let markURL = URL(fileURLWithPath: CommandLine.arguments[1])
    let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])
    let previewURL = URL(fileURLWithPath: CommandLine.arguments[3])
    guard let mark = NSImage(contentsOf: markURL) else {
        throw IconError.sourceImage
    }

    let fileManager = FileManager.default
    let temporaryRoot = fileManager.temporaryDirectory
        .appendingPathComponent("ongrow-support-desk-\(UUID().uuidString)", isDirectory: true)
    let iconsetURL = temporaryRoot.appendingPathComponent("AppIcon.iconset", isDirectory: true)
    try fileManager.createDirectory(at: iconsetURL, withIntermediateDirectories: true)
    defer {
        try? fileManager.removeItem(at: temporaryRoot)
    }

    let iconFiles: [(String, Int)] = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ]

    for (filename, size) in iconFiles {
        try renderIcon(mark: mark, size: size)
            .write(to: iconsetURL.appendingPathComponent(filename), options: .atomic)
    }

    try renderIcon(mark: mark, size: 1024).write(to: previewURL, options: .atomic)
    try fileManager.createDirectory(
        at: outputURL.deletingLastPathComponent(),
        withIntermediateDirectories: true
    )

    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
    process.arguments = [
        "--convert", "icns",
        "--output", outputURL.path,
        iconsetURL.path,
    ]
    try process.run()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else {
        throw IconError.iconutil(process.terminationStatus)
    }
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
