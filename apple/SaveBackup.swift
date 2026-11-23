import Foundation
import SwiftUI
import UniformTypeIdentifiers

/// `.koikari-save` is a single portable file holding every present save slot
/// and every progress record. It does not include resource references, so the
/// same file can be re-imported on another device or after a reinstall.
enum SaveBackup {
  static let format = 1
  static let appIdentifier = "dev.greenhats.koikari-reborn"
  static let fileExtension = "koikari-save"

  /// Public UTType for the document picker/exporter. Identical on both platforms
  /// because the identifier never leaves the app bundle declaration.
  static let utType = UTType(exportedAs: "\(appIdentifier).save")

  enum Failure: LocalizedError {
    case formatVersion(Int)
    case missingFiles
    case entryName(String)
    case entrySize(String)
    case empty

    var errorDescription: String? {
      switch self {
      case .formatVersion(let version):
        return "存档备份格式版本 \(version) 不受支持；请先更新应用再导入。"
      case .missingFiles: return "存档备份内容为空或未包含任何可导入的文件。"
      case .entryName(let name): return "存档备份包含非法文件名 \(name)。"
      case .entrySize(let name): return "存档备份中的 \(name) 超过允许大小。"
      case .empty: return "当前没有可导出的存档或进度。"
      }
    }
  }

  /// Name of a file allowed inside the backup. Must match `save.rs`'s naming.
  private static func validName(_ name: String) -> Bool {
    if name.hasPrefix("slot-") {
      guard let number = Int(name.dropFirst(5).dropLast(5)) else { return false }
      return (1...24).contains(number)
    }
    if name.hasPrefix("progress-") && name.hasSuffix(".json") {
      let hex = name.dropFirst(9).dropLast(5)
      return hex.count == 64 && hex.allSatisfy(\.isHexDigit)
    }
    return false
  }

  /// One entry in the manifest's `files` array.
  private struct Entry: Codable {
    let name: String
    let data: Data
  }

  private struct Manifest: Codable {
    let format: Int
    let app: String
    let createdAt: Date
    let files: [Entry]

    private enum CodingKeys: String, CodingKey {
      case format, app, createdAt, files
      // Backwards-compatibility: the first exported files used snake_case here.
      case createdAtLegacy = "created_at"
    }

    init(format: Int, app: String, createdAt: Date, files: [Entry]) {
      self.format = format
      self.app = app
      self.createdAt = createdAt
      self.files = files
    }

    init(from decoder: Decoder) throws {
      let container = try decoder.container(keyedBy: CodingKeys.self)
      format = try container.decode(Int.self, forKey: .format)
      app = try container.decode(String.self, forKey: .app)
      if let modern = try? container.decode(Date.self, forKey: .createdAt) {
        createdAt = modern
      } else {
        createdAt = try container.decode(Date.self, forKey: .createdAtLegacy)
      }
      files = try container.decode([Entry].self, forKey: .files)
    }

    func encode(to encoder: Encoder) throws {
      var container = encoder.container(keyedBy: CodingKeys.self)
      try container.encode(format, forKey: .format)
      try container.encode(app, forKey: .app)
      try container.encode(createdAt, forKey: .createdAt)
      try container.encode(files, forKey: .files)
    }
  }

  /// Collect every present save/progress file under `saves` into a single
  /// backup document. Slot files that exist but are corrupt are still exported;
  /// the restore side re-validates them on read.
  static func export(from saves: URL, to destination: URL) throws {
    let manager = FileManager.default
    let progressFiles = ((try? manager.contentsOfDirectory(atPath: saves.path)) ?? [])
      .filter { $0.hasPrefix("progress-") && validName($0) }
    let names = (1...24).map { "slot-\($0).json" } + progressFiles
    var files: [Entry] = []
    var total = 0
    for name in names {
      let url = saves.appendingPathComponent(name)
      guard let data = try? Data(contentsOf: url), !data.isEmpty else { continue }
      total += data.count
      guard total <= 256 * 1024 * 1024 else { continue }
      files.append(Entry(name: name, data: data))
    }
    guard !files.isEmpty else { throw Failure.empty }
    let manifest = Manifest(
      format: format, app: appIdentifier, createdAt: Date(), files: files)
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    try encoder.encode(manifest).write(to: destination, options: .atomic)
  }

  /// Parse `source` and write each valid file back into `saves`, creating the
  /// directory when needed. Returns the names actually written. Entries with a
  /// malformed name or oversized payload are skipped, not fatal.
  static func `import`(from source: URL, to saves: URL) throws -> [String] {
    let data = try Data(contentsOf: source)
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    let manifest = try decoder.decode(Manifest.self, from: data)
    guard manifest.format == format else { throw Failure.formatVersion(manifest.format) }
    guard manifest.app == appIdentifier else { throw Failure.missingFiles }
    guard !manifest.files.isEmpty else { throw Failure.empty }

    let manager = FileManager.default
    try manager.createDirectory(at: saves, withIntermediateDirectories: true)
    var written: [String] = []
    for entry in manifest.files {
      guard validName(entry.name), !entry.data.isEmpty,
        entry.data.count <= 4 * 1024 * 1024
      else { continue }
      let path = saves.appendingPathComponent(entry.name)
      try entry.data.write(to: path, options: .atomic)
      written.append(entry.name)
    }
    guard !written.isEmpty else { throw Failure.missingFiles }
    return written
  }
}

/// FileDocument wrapper so `fileExporter` can emit a `.koikari-save` without
/// writing a temporary copy first: the manifest is built lazily on export.
struct SaveBackupDocument: FileDocument {
  static var readableContentTypes: [UTType] { [SaveBackup.utType] }
  var source: URL?

  init(source: URL? = nil) { self.source = source }

  init(configuration: ReadConfiguration) throws {
    throw CocoaError(.featureUnsupported)
  }

  func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
    guard let source else { throw CocoaError(.coderInvalidValue) }
    let staging = FileManager.default.temporaryDirectory
      .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: staging) }
    let package = staging.appendingPathComponent("save", isDirectory: true)
    try SaveBackup.export(from: source, to: package)
    return try FileWrapper(url: package, options: .immediate)
  }
}
