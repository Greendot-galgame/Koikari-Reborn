import ImageIO
import SwiftUI

/// Optional title artwork comes from the connected, verified XP3 store, never the app bundle.
/// Swift also bounds encoded bytes and decoded dimensions before asking ImageIO to decode.
@MainActor
struct OriginalUIArtwork {
  enum Asset: String, CaseIterable {
    case titleLogo = "ui/title/TI_タイトルロゴ.webp"
    case titleBackground = "ui/title/TI_base_0.webp"
  }

  static let byteLimit = 8 * 1024 * 1024
  static let dimensionLimit = 4096
  static let pixelLimit = 4 * 1024 * 1024
  var titleLogo: Image?
  var titleBackground: Image?
  var hasUnavailableArtwork = false

  static func load(read: (String) -> Data?) -> Self {
    var result = Self()
    for asset in Asset.allCases {
      guard let data = read(asset.rawValue), let decoded = decode(data) else {
        result.hasUnavailableArtwork = true
        continue
      }
      let image = Image(decorative: decoded, scale: 1, orientation: .up)
      switch asset {
      case .titleLogo: result.titleLogo = image
      case .titleBackground: result.titleBackground = image
      }
    }
    return result
  }

  static func decode(_ data: Data) -> CGImage? {
    guard !data.isEmpty, data.count <= byteLimit,
      let source = CGImageSourceCreateWithData(
        data as CFData,
        [kCGImageSourceShouldCache: false] as CFDictionary),
      CGImageSourceGetCount(source) == 1,
      let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
      let width = properties[kCGImagePropertyPixelWidth] as? NSNumber,
      let height = properties[kCGImagePropertyPixelHeight] as? NSNumber,
      permitsDimensions(width: width.intValue, height: height.intValue)
    else { return nil }
    guard
      let image = CGImageSourceCreateImageAtIndex(
        source, 0,
        [kCGImageSourceShouldCacheImmediately: true] as CFDictionary),
      image.width == width.intValue, image.height == height.intValue
    else { return nil }
    return image
  }

  static func permitsDimensions(width: Int, height: Int) -> Bool {
    width > 0 && height > 0 && width <= dimensionLimit && height <= dimensionLimit
      && width * height <= pixelLimit
  }
}
