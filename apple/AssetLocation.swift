import Foundation

enum AssetLocation {
  static func identifier(for directory: URL, relativeTo root: URL) -> String? {
    guard directory.isFileURL,
      directory.standardizedFileURL.deletingLastPathComponent().path
        == root.standardizedFileURL.path,
      validIdentifier(directory.lastPathComponent)
    else { return nil }
    return directory.lastPathComponent
  }

  static func resolve(_ stored: String, relativeTo root: URL) -> URL? {
    if validIdentifier(stored) {
      return root.appendingPathComponent(stored, isDirectory: true)
    }
    guard stored.hasPrefix("/") else { return nil }
    let legacy = URL(fileURLWithPath: stored)
    guard !legacy.pathComponents.contains(".."),
      Array(legacy.deletingLastPathComponent().pathComponents.suffix(3))
        == ["Library", "Application Support", root.lastPathComponent],
      validIdentifier(legacy.lastPathComponent)
    else { return nil }
    return root.appendingPathComponent(legacy.lastPathComponent, isDirectory: true)
  }

  private static func validIdentifier(_ value: String) -> Bool {
    value.hasPrefix("Assets-") && UUID(uuidString: String(value.dropFirst(7))) != nil
  }
}
