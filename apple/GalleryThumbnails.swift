import Foundation
import ImageIO
import SwiftUI
import os

/// A lazy, serialized thumbnail pipeline. Preparation stays on MainActor; owned work and
/// bytes cross to the worker, which renders and decodes within one serialized turn. Work
/// must never capture a live Host. Call clear() whenever the connected game or content resets.
@MainActor
final class GameGalleryThumbnails {
  nonisolated static let encodedByteLimit = 32 * 1024 * 1024
  nonisolated static let dimensionLimit = 8192
  nonisolated static let pixelLimit = 16 * 1024 * 1024
  nonisolated static let thumbnailSide = 384
  nonisolated static let cacheByteLimit = 16 * 1024 * 1024
  nonisolated static let cacheEntryLimit = 32

  /// Synchronous owned work, invoked once off-main after lazy MainActor preparation.
  typealias Work = @Sendable () -> Data?

  /// The injectable decoder supports gated synthetic tests. Production always uses ImageIO.
  typealias Decoder = @Sendable (Data) async -> CGImage?

  private struct Waiter {
    let continuation: CheckedContinuation<Image?, Never>
    let cancelled: OSAllocatedUnfairLock<Bool>
    var isCancelled: Bool { cancelled.withLock { $0 } }
  }

  private final class Request {
    let id: String
    let generation: UUID
    var prepare: (@MainActor () -> Work?)?
    var waiters: [UUID: Waiter] = [:]

    init(id: String, generation: UUID, prepare: @escaping @MainActor () -> Work?) {
      self.id = id
      self.generation = generation
      self.prepare = prepare
    }
  }

  private struct Entry {
    let image: Image
    let bytes: Int
  }

  private let decoder: Decoder
  private var generation = UUID()
  private var pending: [String: Request] = [:]
  private var queue: [Request] = []
  private var worker: Task<Void, Never>?
  private var cache: [String: Entry] = [:]
  private var lru: [String] = []  // Least recently used first; at most 32 elements.

  // Read-only accounting also lets synthetic tests observe enqueuing without timing sleeps.
  private(set) var cachedByteCount = 0
  var cachedEntryCount: Int { cache.count }
  var pendingWaiterCount: Int { pending.values.reduce(0) { $0 + $1.waiters.count } }

  init(decoder: @escaping Decoder = { GameGalleryThumbnails.decode($0) }) {
    self.decoder = decoder
  }

  func image(for id: String, read: @escaping @MainActor () -> Data?) async -> Image? {
    await image(
      for: id,
      prepare: {
        // A legacy reader can borrow mutable Host storage. Snapshot it on MainActor,
        // before yielding or allowing any subsequent Host operation to change the bytes.
        guard let owned = Self.ownedData(read()) else { return nil }
        return { owned }
      })
  }

  // Prefer the legacy reader for existing unlabeled trailing closures returning nil.
  @_disfavoredOverload
  func image(for id: String, prepare: @escaping @MainActor () -> Work?) async -> Image? {
    guard !Task.isCancelled else { return nil }
    if let entry = cache[id] {
      touch(id)
      return entry.image
    }

    let requestedGeneration = generation
    let waiterID = UUID()
    let cancelled = OSAllocatedUnfairLock(initialState: false)
    let result: Image? = await withTaskCancellationHandler {
      await withCheckedContinuation { continuation in
        guard !Task.isCancelled else {
          continuation.resume(returning: nil)
          return
        }
        let waiter = Waiter(continuation: continuation, cancelled: cancelled)
        if let request = pending[id] {
          request.waiters[waiterID] = waiter
        } else {
          let request = Request(id: id, generation: generation, prepare: prepare)
          request.waiters[waiterID] = waiter
          pending[id] = request
          queue.append(request)
        }
        if worker == nil {
          worker = Task { await drain() }
        }
      }
    } onCancel: {
      // Mark synchronously: the queue may reach this reader before the actor hop below.
      // OSAllocatedUnfairLock is available on the minimum macOS 13 / iOS 16 targets.
      cancelled.withLock { $0 = true }
      Task { @MainActor [weak self] in
        self?.cancelWaiter(waiterID, for: id, generation: requestedGeneration)
      }
    }
    // A continuation may already have been resumed when clear() runs, but its caller
    // might not resume until afterwards. Do not let that completed old result escape.
    guard generation == requestedGeneration, !Task.isCancelled else { return nil }
    return result
  }

  func clear() {
    generation = UUID()
    cache.removeAll(keepingCapacity: false)
    lru.removeAll(keepingCapacity: false)
    cachedByteCount = 0
    let invalidated = Array(pending.values)
    pending.removeAll(keepingCapacity: false)
    queue.removeAll(keepingCapacity: false)
    for request in invalidated {
      request.prepare = nil
      finish(request, with: nil)
    }
    // Intentionally keep worker alive. Synchronous ImageIO cannot be cancelled. Fresh
    // requests must wait for that decode to really finish, including after repeated clears.
  }

  private func drain() async {
    while !queue.isEmpty {
      let request = queue.removeFirst()
      discardCancelledWaiters(in: request)
      guard isCurrent(request) else { continue }
      guard let work = prepareWork(for: request) else {
        complete(request, image: nil)
        continue
      }
      // No prepare closure, request or live Host crosses to the worker. Rendering and
      // decoding share one turn; clear/cancellation must preserve this serialization tail.
      let decoder = decoder
      let decoded = await Task.detached(priority: .utility) {
        guard let owned = Self.ownedData(work()) else { return nil as CGImage? }
        return await decoder(owned)
      }.value
      discardCancelledWaiters(in: request)
      guard isCurrent(request) else { continue }
      complete(request, image: decoded.flatMap { insert($0, for: request.id) })
    }
    worker = nil
  }

  private func prepareWork(for request: Request) -> Work? {
    // Prepare only at the front of the queue and release the MainActor closure before
    // suspending. A preparation that reenters clear/cancellation abandons its owned work.
    let prepare = request.prepare
    request.prepare = nil
    let work = prepare?()
    discardCancelledWaiters(in: request)
    guard isCurrent(request) else { return nil }
    return work
  }

  nonisolated private static func ownedData(_ data: Data?) -> Data? {
    guard let data, !data.isEmpty, data.count <= encodedByteLimit else { return nil }
    return data.withUnsafeBytes { Data(bytes: $0.baseAddress!, count: $0.count) }
  }

  private func isCurrent(_ request: Request) -> Bool {
    request.generation == generation && pending[request.id] === request
  }

  private func cancelWaiter(_ waiterID: UUID, for id: String, generation: UUID) {
    guard generation == self.generation, let request = pending[id],
      let waiter = request.waiters.removeValue(forKey: waiterID)
    else { return }
    waiter.continuation.resume(returning: nil)
    discardIfUnobserved(request)
  }

  private func discardCancelledWaiters(in request: Request) {
    guard isCurrent(request) else { return }
    for (id, waiter) in request.waiters where waiter.isCancelled {
      request.waiters.removeValue(forKey: id)
      waiter.continuation.resume(returning: nil)
    }
    discardIfUnobserved(request)
  }

  private func discardIfUnobserved(_ request: Request) {
    guard request.waiters.isEmpty, isCurrent(request) else { return }
    pending.removeValue(forKey: request.id)
    queue.removeAll { $0 === request }
    request.prepare = nil
    // Never cancel the worker or detached decode. If already active, this request's
    // result is discarded, but the next request still waits for actual decode completion.
  }

  private func complete(_ request: Request, image: Image?) {
    guard isCurrent(request) else { return }
    pending.removeValue(forKey: request.id)
    finish(request, with: image)
  }

  private func finish(_ request: Request, with image: Image?) {
    let waiters = request.waiters
    request.waiters.removeAll(keepingCapacity: false)
    for waiter in waiters.values {
      waiter.continuation.resume(returning: waiter.isCancelled ? nil : image)
    }
  }

  private func touch(_ id: String) {
    lru.removeAll { $0 == id }
    lru.append(id)
  }

  private func insert(_ decoded: CGImage, for id: String) -> Image? {
    guard decoded.width > 0, decoded.height > 0,
      decoded.width <= Self.thumbnailSide, decoded.height <= Self.thumbnailSide,
      decoded.bytesPerRow > 0
    else { return nil }
    let (bytes, overflow) = decoded.bytesPerRow.multipliedReportingOverflow(by: decoded.height)
    guard !overflow, bytes > 0, bytes <= Self.cacheByteLimit else { return nil }
    while cache.count >= Self.cacheEntryLimit || cachedByteCount > Self.cacheByteLimit - bytes {
      let oldest = lru.removeFirst()
      if let removed = cache.removeValue(forKey: oldest) { cachedByteCount -= removed.bytes }
    }
    // ImageIO transforms orientation and scales the entire image; it never crops to a square.
    let image = Image(decorative: decoded, scale: 1, orientation: .up)
    cache[id] = Entry(image: image, bytes: bytes)
    cachedByteCount += bytes
    touch(id)
    return image
  }

  nonisolated static func permitsDimensions(width: Int, height: Int) -> Bool {
    width > 0 && height > 0 && width <= dimensionLimit && height <= dimensionLimit
      && width * height <= pixelLimit
  }

  /// Validate the encoded source before allocating pixels. Thumbnail creation requests the
  /// full frame (with EXIF orientation), bounded to 384 on its longest side, not a crop.
  nonisolated static func decode(_ data: Data) -> CGImage? {
    autoreleasepool {
      guard !data.isEmpty, data.count <= encodedByteLimit,
        let source = CGImageSourceCreateWithData(
          data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
        CGImageSourceGetStatus(source) == .statusComplete,
        CGImageSourceGetCount(source) == 1,
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
        let encodedWidth = properties[kCGImagePropertyPixelWidth] as? NSNumber,
        let encodedHeight = properties[kCGImagePropertyPixelHeight] as? NSNumber,
        let width = Int(exactly: encodedWidth.doubleValue),
        let height = Int(exactly: encodedHeight.doubleValue),
        permitsDimensions(width: width, height: height),
        CGImageSourceGetStatusAtIndex(source, 0) == .statusComplete
      else { return nil }
      let options: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceThumbnailMaxPixelSize: thumbnailSide,
        kCGImageSourceShouldCacheImmediately: true,
        kCGImageSourceShouldAllowFloat: false,
      ]
      guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary),
        image.width > 0, image.height > 0,
        image.width <= thumbnailSide, image.height <= thumbnailSide
      else { return nil }
      return image
    }
  }
}
