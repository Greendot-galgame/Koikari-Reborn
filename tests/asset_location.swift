import Foundation

@main struct AssetLocationTests {
  static func main() {
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
    print("Asset location: 16 relative-path, migration and rejection assertions passed")
  }
}
