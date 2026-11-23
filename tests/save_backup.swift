import Foundation

@main struct SaveBackupChecks {
  static func main() throws {
    let manager = FileManager.default
    let root = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try manager.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? manager.removeItem(at: root) }
    let saves = root.appendingPathComponent("Saves")
    try manager.createDirectory(at: saves, withIntermediateDirectories: false)
    // A valid slot file and one global progress file.
    let slot9 = Data(#"{"version":2,"state":{"x":1},"assets":"deadbeef"}"#.utf8)
    let slot3 = Data(#"{"version":2,"state":{"x":2},"assets":"deadbeef"}"#.utf8)
    let progress = Data(#"{"version":1,"progress":{},"assets":"deadbeef"}"#.utf8)
    try slot9.write(to: saves.appendingPathComponent("slot-9.json"))
    try slot3.write(to: saves.appendingPathComponent("slot-3.json"))
    try progress.write(
      to: saves.appendingPathComponent(
        "progress-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.json"))
    // Files that must NOT be exported.
    try Data("foreign".utf8).write(to: saves.appendingPathComponent("slot-25.json"))
    try Data("foreign".utf8).write(to: saves.appendingPathComponent("slot-0.json"))
    try Data("foreign".utf8).write(to: saves.appendingPathComponent("slot-1.txt"))
    try Data("foreign".utf8).write(to: saves.appendingPathComponent("progress-short.json"))
    try Data("foreign".utf8).write(to: saves.appendingPathComponent("unrelated.json"))
    try Data().write(to: saves.appendingPathComponent("slot-5.json"))

    let backup = root.appendingPathComponent("backup.koikari-save")
    try SaveBackup.export(from: saves, to: backup)
    let manifestData = try Data(contentsOf: backup)
    let manifest = try JSONSerialization.jsonObject(with: manifestData) as? [String: Any]
    precondition(manifest?["format"] as? Int == 1)
    precondition(manifest?["app"] as? String == "dev.greenhats.koikari-reborn")
    let entries = manifest?["files"] as? [[String: Any]]
    precondition(entries?.count == 3)
    let names = Set(entries?.compactMap { $0["name"] as? String } ?? [])
    let progressName =
      "progress-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.json"
    precondition(names == ["slot-3.json", "slot-9.json", progressName])

    // Empty directory exports nothing.
    let emptySaves = root.appendingPathComponent("Empty")
    try manager.createDirectory(at: emptySaves, withIntermediateDirectories: false)
    do {
      try SaveBackup.export(from: emptySaves, to: backup)
      preconditionFailure("Export of an empty directory must fail")
    } catch SaveBackup.Failure.empty {}

    // Round-trip into a fresh directory.
    let restore = root.appendingPathComponent("Restore")
    let written = try SaveBackup.import(from: backup, to: restore)
    precondition(Set(written) == names)
    for name in names {
      let restored = try Data(contentsOf: restore.appendingPathComponent(name))
      let original = try Data(contentsOf: saves.appendingPathComponent(name))
      precondition(restored == original)
    }
    precondition(
      !manager.fileExists(
        atPath: restore.appendingPathComponent("slot-25.json").path))
    precondition(
      !manager.fileExists(
        atPath: restore.appendingPathComponent("unrelated.json").path))

    // Wrong format version is rejected.
    var bad = manifest!
    bad["format"] = 99
    let badBackup = root.appendingPathComponent("bad.koikari-save")
    try JSONSerialization.data(withJSONObject: bad).write(to: badBackup)
    do {
      _ = try SaveBackup.import(from: badBackup, to: restore)
      preconditionFailure("Wrong format must be rejected")
    } catch SaveBackup.Failure.formatVersion(let version) {
      precondition(version == 99)
    }

    // A backup whose entries are all invalid names still fails cleanly.
    let junkFiles: [[String: Any]] = [
      ["name": "../escape.json", "data": Data("x".utf8).base64EncodedString()],
      ["name": "slot-99.json", "data": Data("x".utf8).base64EncodedString()],
      ["name": "progress-short.json", "data": Data("x".utf8).base64EncodedString()],
    ]
    let junkManifest: [String: Any] = [
      "format": 1, "app": "dev.greenhats.koikari-reborn",
      "created_at": ISO8601DateFormatter().string(from: Date()), "files": junkFiles,
    ]
    let junk = root.appendingPathComponent("junk.koikari-save")
    try JSONSerialization.data(withJSONObject: junkManifest).write(to: junk)
    do {
      _ = try SaveBackup.import(from: junk, to: restore)
      preconditionFailure("All-invalid backup must report missing files")
    } catch SaveBackup.Failure.missingFiles {}
    precondition(
      !manager.fileExists(
        atPath: restore.appendingPathComponent("slot-99.json").path))

    print(
      "Save backup: manifest round-trip, slot/progress naming guard, empty and invalid entry rejection passed"
    )
  }
}
