import Foundation

@main struct AssetLocationTests {
  static func main() throws {
    let root = URL(fileURLWithPath: "/new-container/Library/Application Support/KoikariReborn")
    let name = "Assets-12345678-1234-4321-ABCD-123456789ABC"
    let current = root.appendingPathComponent(name)
    let legacy = "/old-container/Library/Application Support/KoikariReborn/\(name)"

    precondition(AssetLocation.identifier(for: current, relativeTo: root) == name)
    for stored in [name, current.path, legacy] {
      precondition(AssetLocation.resolve(stored, relativeTo: root)?.path == current.path)
    }
    for invalid in [
      "", "Saves", "Assets-not-a-uuid", "../\(name)", "\(name)/data.xp3",
      "/tmp/\(name)", "/old/Library/Application Support/OtherApp/\(name)",
      "/old/Library/Application Support/KoikariReborn/../KoikariReborn/\(name)",
      "file://\(legacy)",
    ] {
      precondition(AssetLocation.resolve(invalid, relativeTo: root) == nil, invalid)
    }
    for outside in [root, root.appendingPathComponent("Saves"), URL(fileURLWithPath: legacy)] {
      precondition(AssetLocation.identifier(for: outside, relativeTo: root) == nil)
    }
    let manager = FileManager.default
    let temporary = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? manager.removeItem(at: temporary) }
    let documents = temporary.appendingPathComponent("Documents")
    let folder = try AssetLocation.prepareDocumentResources(in: documents)
    precondition(folder.lastPathComponent == "GameResources")
    precondition(manager.fileExists(atPath: folder.appendingPathComponent("资源放置说明.txt").path))
    precondition(!manager.fileExists(atPath: folder.appendingPathComponent("data.xp3").path))
    let original = Data("user-owned resource placeholder".utf8)
    let save = Data("existing save placeholder".utf8)
    try original.write(to: folder.appendingPathComponent("data.xp3"))
    try save.write(to: documents.appendingPathComponent("manual-save.json"))
    try Data("user instructions".utf8).write(to: folder.appendingPathComponent("资源放置说明.txt"))
    let prepared = try AssetLocation.prepareDocumentResources(in: documents)
    precondition(prepared == folder)
    let resourceAfter = try Data(contentsOf: folder.appendingPathComponent("data.xp3"))
    let saveAfter = try Data(contentsOf: documents.appendingPathComponent("manual-save.json"))
    let instructionsAfter = try String(
      contentsOf: folder.appendingPathComponent("资源放置说明.txt"), encoding: .utf8)
    precondition(resourceAfter == original && saveAfter == save)
    precondition(instructionsAfter == "user instructions")
    let identifier = AssetLocation.identifier(for: folder, relativeTo: root, documents: documents)
    precondition(identifier == AssetLocation.documentIdentifier)
    precondition(
      AssetLocation.resolve(identifier!, relativeTo: root, documents: documents) == folder)
    let newDocuments = URL(fileURLWithPath: "/new-container/Documents")
    precondition(
      AssetLocation.resolve(identifier!, relativeTo: root, documents: newDocuments)?.path
        == "/new-container/Documents/GameResources")
    precondition(AssetLocation.resolve(identifier!, relativeTo: root) == nil)
    for outside in [documents, folder.appendingPathComponent("nested"), temporary] {
      precondition(
        AssetLocation.identifier(for: outside, relativeTo: root, documents: documents) == nil)
    }
    for invalid in ["Documents/../GameResources", "Documents/GameResources/data.xp3", folder.path] {
      precondition(AssetLocation.resolve(invalid, relativeTo: root, documents: documents) == nil)
    }
    print(
      "Asset location: legacy migration, Files resource folder, container relocation, preservation and rejection assertions passed"
    )
  }
}
