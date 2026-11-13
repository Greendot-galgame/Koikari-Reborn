import Foundation

enum AssetLocation {
  static let documentFolder = "GameResources"
  static let documentIdentifier = "Documents/GameResources"

  static func prepareDocumentResources(in documents: URL) throws -> URL {
    let folder = documents.appendingPathComponent(documentFolder, isDirectory: true)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let instructions = folder.appendingPathComponent("资源放置说明.txt")
    if !FileManager.default.fileExists(atPath: instructions.path) {
      try Data(
        """
        将自己合法持有的 data.xp3 放在本文件夹中，可同时放入 adult.xp3、patch.xp3 和 patch1.xp3 至 patch50.xp3。
        无需选择文件夹或复制到另一份应用缓存。移动或复制完成后，回到游戏点击“重新检查资源”，也可重新启动应用。
        不要把 XP3 放在更深的子文件夹中；无需 EXE、DLL 或原版 savedata。游戏运行时不要修改资源。
        本应用不附带原作资源。原有游戏存档由应用单独管理，不在此文件夹。
        """.utf8
      ).write(to: instructions, options: .withoutOverwriting)
    }
    return folder
  }

  static func identifier(for directory: URL, relativeTo root: URL, documents: URL? = nil) -> String?
  {
    if let documents,
      directory.standardizedFileURL.path
        == documents.appendingPathComponent(documentFolder).standardizedFileURL.path
    {
      return documentIdentifier
    }
    guard directory.isFileURL,
      directory.standardizedFileURL.deletingLastPathComponent().path
        == root.standardizedFileURL.path,
      validIdentifier(directory.lastPathComponent)
    else { return nil }
    return directory.lastPathComponent
  }

  static func resolve(_ stored: String, relativeTo root: URL, documents: URL? = nil) -> URL? {
    if stored == documentIdentifier, let documents {
      return documents.appendingPathComponent(documentFolder, isDirectory: true)
    }
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
