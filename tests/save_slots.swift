import Foundation

@main struct SaveSlotChecks {
  static func main() throws {
    let decoder = JSONDecoder()
    var checks = 0
    func expect(_ condition: Bool) {
      precondition(condition)
      checks += 1
    }
    let base: [String: Any] = [
      "slot": 1, "exists": true, "compatible": true, "saved_at": 1_790_000_000_000,
      "pages": 128, "speaker": "合成角色", "text": "同一页也可能有不同的保存画面。",
    ]
    func decode(_ values: [String: Any]) throws -> SaveSlot {
      try decoder.decode(SaveSlot.self, from: JSONSerialization.data(withJSONObject: values))
    }
    let olderCatalog = try decode(base)
    expect(olderCatalog.previewRevision == nil)
    expect(olderCatalog.compatible && olderCatalog.exists)
    expect(olderCatalog.slot == 1 && olderCatalog.pages == 128)
    expect(olderCatalog.savedAt == 1_790_000_000_000)
    expect(olderCatalog.name == "手动槽位 1")
    var record = base
    record["preview_revision"] = NSNull()
    expect(try decode(record).previewRevision == nil)
    let firstRevision = String(repeating: "a", count: 64)
    record["preview_revision"] = firstRevision
    let first = try decode(record)
    expect(first.previewRevision == firstRevision)
    let secondRevision = String(repeating: "b", count: 64)
    record["preview_revision"] = secondRevision
    let overwritten = try decode(record)
    expect(overwritten.previewRevision != first.previewRevision)
    expect(overwritten.savedAt == first.savedAt && overwritten.pages == first.pages)
    expect(overwritten.text == first.text && overwritten.id == first.id)
    // A missing/unrenderable picture never changes load compatibility.
    record["preview_revision"] = NSNull()
    record["slot"] = 9
    let automatic = try decode(record)
    expect(automatic.previewRevision == nil && automatic.compatible)
    expect(automatic.name == "自动进度")
    record["compatible"] = false
    record["error"] = "资源不匹配"
    let invalid = try decode(record)
    expect(!invalid.compatible && invalid.error == "资源不匹配")
    expect(invalid.previewRevision == nil)
    record["exists"] = false
    record["error"] = NSNull()
    let empty = try decode(record)
    expect(!empty.exists && !empty.compatible && empty.previewRevision == nil)
    expect(empty.error == nil)
    print("\(checks) save catalog compatibility/revision assertions passed (synthetic JSON only)")
  }
}
