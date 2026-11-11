import Foundation

final class AssetImport: @unchecked Sendable {
  static let archiveNames =
    ["data.xp3", "adult.xp3", "patch.xp3"]
    + (1...50).map { "patch\($0).xp3" }
  private let lock = NSLock()
  private var cancelled = false
  var isCancelled: Bool { lock.withLock { cancelled } }
  func cancel() { lock.withLock { cancelled = true } }

  enum Failure: LocalizedError {
    case invalid(String)
    case space, cancelled, destinationExists
    var errorDescription: String? {
      switch self {
      case .invalid(let name): return "\(name) 不是可读取的 XP3 文件。请选择包含 data.xp3 的文件夹。"
      case .space: return "存储空间不足。请预留资源大小及至少 64 MB 额外空间后重试。"
      case .cancelled: return "已取消导入，原资源未更改。"
      case .destinationExists: return "导入目标已存在，未覆盖任何已有文件。"
      }
    }
  }

  static func checkSpace(required: Int64, available: Int64?) throws {
    if let available, available < 64 * 1024 * 1024 || required > available - 64 * 1024 * 1024 {
      throw Failure.space
    }
  }

  static func cleanAbandoned(in directory: URL) {
    let entries =
      (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil))
      ?? []
    for entry in entries
    where entry.lastPathComponent.hasPrefix("Import-")
      && UUID(uuidString: String(entry.lastPathComponent.dropFirst(7))) != nil
    {
      try? FileManager.default.removeItem(at: entry)
    }
  }

  func copy(from source: URL, to destination: URL, progress: (Int64, Int64) -> Void) throws {
    let manager = FileManager.default
    var files: [(URL, Int64)] = []
    var total: Int64 = 0
    for (index, name) in Self.archiveNames.enumerated() {
      let url = source.appendingPathComponent(name)
      if index > 0 && !manager.fileExists(atPath: url.path) { continue }
      let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
      guard values.isRegularFile == true, let size = values.fileSize, size >= 11 else {
        throw Failure.invalid(name)
      }
      let input = try FileHandle(forReadingFrom: url)
      let magic = try input.read(upToCount: 11)
      try input.close()
      guard magic == Data([0x58, 0x50, 0x33, 0x0d, 0x0a, 0x20, 0x0a, 0x1a, 0x8b, 0x67, 0x01]),
        Int64(size) <= Int64.max - total
      else { throw Failure.invalid(name) }
      files.append((url, Int64(size)))
      total += Int64(size)
    }
    let parent = destination.deletingLastPathComponent()
    try manager.createDirectory(at: parent, withIntermediateDirectories: true)
    let capacity = try parent.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey]
    )
    try Self.checkSpace(
      required: total, available: capacity.volumeAvailableCapacityForImportantUsage)
    if isCancelled { throw Failure.cancelled }
    guard !manager.fileExists(atPath: destination.path) else { throw Failure.destinationExists }
    try manager.createDirectory(at: destination, withIntermediateDirectories: false)
    var completed = false
    defer { if !completed { try? manager.removeItem(at: destination) } }
    var copied: Int64 = 0
    var reported: Int64 = 0
    progress(0, total)
    for (url, expected) in files {
      let input = try FileHandle(forReadingFrom: url)
      defer { try? input.close() }
      let target = destination.appendingPathComponent(url.lastPathComponent)
      try Data().write(to: target, options: .withoutOverwriting)
      let output = try FileHandle(forWritingTo: target)
      defer { try? output.close() }
      var count: Int64 = 0
      while let bytes = try input.read(upToCount: 2 * 1024 * 1024), !bytes.isEmpty {
        if isCancelled { throw Failure.cancelled }
        count += Int64(bytes.count)
        guard count <= expected else { throw Failure.invalid(url.lastPathComponent) }
        try output.write(contentsOf: bytes)
        copied += Int64(bytes.count)
        if copied - reported >= 16 * 1024 * 1024 || copied == total {
          progress(copied, total)
          reported = copied
        }
      }
      guard count == expected else { throw Failure.invalid(url.lastPathComponent) }
      try output.synchronize()
    }
    if isCancelled { throw Failure.cancelled }
    completed = true
  }
}
