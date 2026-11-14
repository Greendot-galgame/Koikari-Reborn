import Foundation
import os

/// Captures an immutable save scene on MainActor. Only its owned lease enters the worker;
/// neither the live Host nor any gameplay action/error channel is captured by the work.
@MainActor
enum SaveThumbnailReader {
  static func prepare(
    handle: OpaquePointer, slot: UInt32, revision: String
  ) -> GameGalleryThumbnails.Work? {
    guard let pointer = revision.withCString({ kr_save_preview_request(handle, slot, $0) }) else {
      return nil
    }
    let lease = OwnedSavePreview(pointer)
    return { [lease] in lease.render() }
  }
}

/// The opaque lease owns an immutable Scene + Arc<Store>, not a Host. The invocation lock
/// permits exactly one exclusive render. The worker frees its borrowed lease after that
/// render; deinit frees only an uninvoked lease, including when preparation is abandoned.
private final class OwnedSavePreview: @unchecked Sendable {
  private let pointer: OpaquePointer
  private let invoked = OSAllocatedUnfairLock(initialState: false)
  private static let pngByteLimit = 1024 * 1024

  init(_ pointer: OpaquePointer) { self.pointer = pointer }

  deinit {
    if !invoked.withLock({ $0 }) { kr_save_preview_free(pointer) }
  }

  func render() -> Data? {
    guard
      invoked.withLock({ invoked in
        guard !invoked else { return false }
        invoked = true
        return true
      })
    else { return nil }
    return withExtendedLifetime(self) {
      // Once claimed, cleanup stays on this worker even if other owners outlive render.
      defer { kr_save_preview_free(pointer) }
      var count = 0
      let bytes = kr_save_preview_png(pointer, &count)
      // Preserve the original count, including invalid lengths, for the allocation owner.
      defer { if let bytes { kr_bytes_free(bytes, count) } }
      guard let bytes, count > 0, count <= Self.pngByteLimit else { return nil }
      return Data(bytes: bytes, count: count)
    }
  }
}
