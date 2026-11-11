import Foundation

@main struct AssetImportTests {
  static func main() throws {
    let manager = FileManager.default
    let root = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try manager.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? manager.removeItem(at: root) }
    let source = root.appendingPathComponent("Source")
    try manager.createDirectory(at: source, withIntermediateDirectories: false)
    var data = Data([0x58, 0x50, 0x33, 0x0d, 0x0a, 0x20, 0x0a, 0x1a, 0x8b, 0x67, 0x01])
    data.append(Data(repeating: 42, count: 3 * 1024 * 1024))
    try data.write(to: source.appendingPathComponent("data.xp3"))
    let output = root.appendingPathComponent("Success")
    var progress: [(Int64, Int64)] = []
    try AssetImport().copy(from: source, to: output) { progress.append(($0, $1)) }
    let copied = try Data(contentsOf: output.appendingPathComponent("data.xp3"))
    precondition(copied == data)
    precondition(progress.first?.0 == 0 && progress.last?.0 == Int64(data.count))
    precondition(!manager.fileExists(atPath: output.appendingPathComponent("patch.xp3").path))
    do {
      try AssetImport().copy(from: source, to: output) { _, _ in }
      preconditionFailure("Existing destination must not be replaced")
    } catch AssetImport.Failure.destinationExists {}
    precondition(manager.fileExists(atPath: output.appendingPathComponent("data.xp3").path))
    let cancelled = root.appendingPathComponent("Cancelled")
    let job = AssetImport()
    do {
      try job.copy(from: source, to: cancelled) { _, _ in job.cancel() }
      preconditionFailure("Cancellation must stop copying")
    } catch AssetImport.Failure.cancelled {}
    precondition(!manager.fileExists(atPath: cancelled.path))
    try Data("invalid patch".utf8).write(to: source.appendingPathComponent("patch.xp3"))
    do {
      try AssetImport().copy(from: source, to: cancelled) { _, _ in }
      preconditionFailure("Invalid optional patch must be rejected")
    } catch AssetImport.Failure.invalid(_) {}
    precondition(!manager.fileExists(atPath: cancelled.path))
    try data.write(to: source.appendingPathComponent("patch.xp3"))
    for name in ["adult.xp3", "patch2.xp3", "patch10.xp3", "patch50.xp3"] {
      try data.write(to: source.appendingPathComponent(name))
    }
    try Data("do not import executables".utf8).write(to: source.appendingPathComponent("Setup.exe"))
    let patched = root.appendingPathComponent("Patched")
    var patchProgress: [(Int64, Int64)] = []
    try AssetImport().copy(from: source, to: patched) { patchProgress.append(($0, $1)) }
    for name in ["data.xp3", "adult.xp3", "patch.xp3", "patch2.xp3", "patch10.xp3", "patch50.xp3"] {
      let imported = try Data(contentsOf: patched.appendingPathComponent(name))
      precondition(imported == data)
    }
    precondition(patchProgress.last?.0 == Int64(data.count) * 6)
    precondition(patchProgress.last?.1 == Int64(data.count) * 6)
    precondition(!manager.fileExists(atPath: patched.appendingPathComponent("Setup.exe").path))
    let midCopy = AssetImport()
    do {
      try midCopy.copy(from: source, to: cancelled) { copied, _ in
        if copied > 0 { midCopy.cancel() }
      }
      preconditionFailure("Supplement copy cancellation must remove its partial destination")
    } catch AssetImport.Failure.cancelled {}
    precondition(!manager.fileExists(atPath: cancelled.path))
    for name in ["adult.xp3", "patch50.xp3"] {
      try Data("invalid archive".utf8).write(to: source.appendingPathComponent(name))
      do {
        try AssetImport().copy(from: source, to: cancelled) { _, _ in }
        preconditionFailure("Invalid supplements must not be silently skipped")
      } catch AssetImport.Failure.invalid(let failed) { precondition(failed == name) }
      precondition(!manager.fileExists(atPath: cancelled.path))
      try data.write(to: source.appendingPathComponent(name))
    }
    do {
      try AssetImport.checkSpace(required: 100, available: 100)
      preconditionFailure("Space reserve must be enforced")
    } catch AssetImport.Failure.space {}
    try AssetImport.checkSpace(required: 100, available: 64 * 1024 * 1024 + 100)
    try AssetImport.checkSpace(required: 100, available: nil)
    let abandoned = root.appendingPathComponent("Import-\(UUID().uuidString)")
    try manager.createDirectory(at: abandoned, withIntermediateDirectories: false)
    AssetImport.cleanAbandoned(in: root)
    precondition(!manager.fileExists(atPath: abandoned.path))
    precondition(manager.fileExists(atPath: source.path) && manager.fileExists(atPath: output.path))
    print(
      "Asset import: base/supplement/numbered patches, total progress, mid-copy cancellation, invalid archives, executable exclusion, destination protection, cleanup and disk reserve passed"
    )
  }
}
