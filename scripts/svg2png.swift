// Draw an SVG into a PNG of a given size, keeping its transparency.
//   swift svg2png.swift in.svg out.png 1024 [height]
import AppKit

let args = CommandLine.arguments
guard args.count >= 4, let width = Int(args[3]), let image = NSImage(contentsOfFile: args[1]) else {
    FileHandle.standardError.write("usage: svg2png.swift in.svg out.png width [height]\n".data(using: .utf8)!)
    exit(1)
}
let height = args.count > 4 ? Int(args[4])! : Int((Double(width) * Double(image.size.height / image.size.width)).rounded())
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
                              samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                              bytesPerRow: 0, bitsPerPixel: 0)!
bitmap.size = NSSize(width: width, height: height)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
NSGraphicsContext.current?.imageInterpolation = .high
image.draw(in: NSRect(x: 0, y: 0, width: width, height: height), from: .zero, operation: .copy, fraction: 1)
NSGraphicsContext.restoreGraphicsState()
try! bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))
