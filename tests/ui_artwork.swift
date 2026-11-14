// Synthetic image fixtures only. No original game resources, preferences or saves.
import Foundation
import ImageIO

@main struct UIArtworkTests {
  @MainActor static func main() {
    var assertions = 0
    func check(_ condition: @autoclosure () -> Bool, _ message: String) {
      precondition(condition(), message)
      assertions += 1
    }
    for (w, h, expected) in [
      (900, 486, true), (2560, 1440, true), (2048, 2048, true), (4096, 1, true),
      (0, 1, false), (1, 0, false), (-1, 1, false), (1, -1, false),
      (4097, 1, false), (1, 4097, false), (4096, 4096, false),
      (Int.max, Int.max, false), (Int.min, 1, false),
    ] {
      check(
        OriginalUIArtwork.permitsDimensions(width: w, height: h) == expected, "dimension bounds")
    }
    let context = CGContext(
      data: nil, width: 16, height: 8, bitsPerComponent: 8,
      bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.setFillColor(CGColor(red: 0.3, green: 0.4, blue: 0.5, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: 16, height: 8))
    let pixels = context.makeImage()!
    func encoded(type: String, frames: Int = 1) -> Data {
      let data = NSMutableData()
      let destination = CGImageDestinationCreateWithData(data, type as CFString, frames, nil)!
      for _ in 0..<frames { CGImageDestinationAddImage(destination, pixels, nil) }
      precondition(CGImageDestinationFinalize(destination))
      return data as Data
    }
    let png = encoded(type: "public.png")
    let decoded = OriginalUIArtwork.decode(png)
    check(decoded?.width == 16 && decoded?.height == 8, "valid image decodes")
    check(OriginalUIArtwork.decode(Data()) == nil, "empty image rejected")
    check(OriginalUIArtwork.decode(Data([0, 1, 2, 3])) == nil, "invalid image rejected")
    check(
      OriginalUIArtwork.decode(Data(repeating: 0, count: OriginalUIArtwork.byteLimit + 1)) == nil,
      "encoded byte cap")
    check(
      OriginalUIArtwork.decode(encoded(type: "com.compuserve.gif", frames: 2)) == nil,
      "multi-frame artwork rejected")
    var requested: [String] = []
    let loaded = OriginalUIArtwork.load {
      requested.append($0)
      return png
    }
    check(
      requested == OriginalUIArtwork.Asset.allCases.map(\.rawValue),
      "only allowlisted roles requested")
    check(
      loaded.titleLogo != nil && loaded.titleBackground != nil && !loaded.hasUnavailableArtwork,
      "complete artwork set")
    let partial = OriginalUIArtwork.load {
      $0 == OriginalUIArtwork.Asset.titleLogo.rawValue ? png : nil
    }
    check(
      partial.titleLogo != nil && partial.titleBackground == nil && partial.hasUnavailableArtwork,
      "partial artwork keeps valid image and reports fallback")
    let missing = OriginalUIArtwork.load { _ in nil }
    check(
      missing.titleLogo == nil && missing.titleBackground == nil && missing.hasUnavailableArtwork,
      "missing artwork does not preserve stale image")
    let corrupted = OriginalUIArtwork.load { _ in Data([0]) }
    check(corrupted.titleLogo == nil && corrupted.hasUnavailableArtwork, "decode failure fallback")
    check(!OriginalUIArtwork().hasUnavailableArtwork, "unconnected default does not claim an error")
    print("\(assertions) original UI decode/fallback assertions passed (synthetic images only)")
  }
}
