// Synthetic fixtures only. No original game resources, preferences, saves, Host or FFI.
import Foundation
import ImageIO
import SwiftUI

/// Each decode stays suspended until explicitly released, so queue/reset tests do not
/// depend on how quickly ImageIO or the scheduler happens to run on a particular machine.
private actor DecodeGate {
  private let image: CGImage?
  private var started = 0
  private var active = 0
  private var maximumActive = 0
  private var releases: [Int: CheckedContinuation<Void, Never>] = [:]
  private var observers: [(Int, CheckedContinuation<Void, Never>)] = []
  private var inputs: [Data] = []

  init(image: CGImage?) { self.image = image }

  func decode(_ data: Data) async -> CGImage? {
    started += 1
    let number = started
    active += 1
    maximumActive = max(maximumActive, active)
    inputs.append(data)
    await withCheckedContinuation { continuation in
      releases[number] = continuation
      let ready = observers.filter { $0.0 <= started }
      observers.removeAll { $0.0 <= started }
      for (_, observer) in ready { observer.resume() }
    }
    active -= 1
    return image
  }

  func waitForStart(_ count: Int) async {
    if started >= count { return }
    await withCheckedContinuation { observers.append((count, $0)) }
  }

  func release(_ number: Int) {
    precondition(releases[number] != nil, "release only a started decode")
    releases.removeValue(forKey: number)!.resume()
  }

  func snapshot() -> (started: Int, active: Int, maximum: Int, inputs: [Data]) {
    (started, active, maximumActive, inputs)
  }
}

@main
@MainActor
struct GalleryThumbnailTests {
  private static var assertions = 0

  static func main() async {
    testDecode()
    await testDefaultDecoder()
    await testCoalescingAndSerialization()
    await testCancellationCoalescing()
    await testQueuedCancellation()
    await testCancellationBeforeActorHop()
    await testActiveCancellationAndClear()
    await testOwnedBytes()
    await testFailures()
    await testClear()
    await testClearAfterCompletion()
    await testCacheLimits()
    print("\(assertions) gallery thumbnail assertions passed (synthetic images only)")
  }

  private static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    precondition(condition(), message)
    assertions += 1
  }

  private static func pixels(width: Int, height: Int, bytesPerRow: Int = 0) -> CGImage {
    let context = CGContext(
      data: nil, width: width, height: height, bitsPerComponent: 8,
      bytesPerRow: bytesPerRow, space: CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    // Different colors on both ends make a full-frame, non-cropped thumbnail observable.
    context.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: width / 2, height: height))
    context.setFillColor(CGColor(red: 0, green: 0, blue: 1, alpha: 1))
    context.fill(CGRect(x: width / 2, y: 0, width: width - width / 2, height: height))
    return context.makeImage()!
  }

  private static func encoded(
    _ image: CGImage, type: String = "public.png", frames: Int = 1, orientation: Int = 1
  ) -> Data {
    let data = NSMutableData()
    let destination = CGImageDestinationCreateWithData(data, type as CFString, frames, nil)!
    for _ in 0..<frames {
      CGImageDestinationAddImage(
        destination, image, [kCGImagePropertyOrientation: orientation] as CFDictionary)
    }
    precondition(CGImageDestinationFinalize(destination))
    return data as Data
  }

  private static func testDecode() {
    for (width, height, expected) in [
      (1, 1, true), (8192, 1, true), (1, 8192, true), (4096, 4096, true),
      (8192, 2048, true), (8192, 2049, false), (4097, 4096, false),
      (8193, 1, false), (1, 8193, false), (0, 1, false), (1, 0, false),
      (-1, 1, false), (1, -1, false), (Int.max, Int.max, false), (Int.min, 1, false),
    ] {
      check(
        GameGalleryThumbnails.permitsDimensions(width: width, height: height) == expected,
        "source dimension/pixel bounds \(width)x\(height)")
    }
    for (width, height, expectedWidth, expectedHeight) in [
      (16, 8, 16, 8), (1536, 384, 384, 96), (384, 1536, 96, 384),
      (768, 768, 384, 384), (8192, 64, 384, 3),
    ] {
      let thumbnail = GameGalleryThumbnails.decode(encoded(pixels(width: width, height: height)))
      check(
        thumbnail?.width == expectedWidth && thumbnail?.height == expectedHeight,
        "full image scales proportionally within 384: \(width)x\(height)")
    }
    let png = encoded(pixels(width: 16, height: 8))
    check(GameGalleryThumbnails.decode(Data()) == nil, "reject empty data")
    check(GameGalleryThumbnails.decode(Data([0, 1, 2, 3])) == nil, "reject corrupt data")
    check(GameGalleryThumbnails.decode(png.prefix(24)) == nil, "reject truncated PNG")
    check(
      GameGalleryThumbnails.decode(encoded(pixels(width: 8193, height: 1))) == nil,
      "reject encoded excessive width before thumbnail creation")
    check(
      GameGalleryThumbnails.decode(encoded(pixels(width: 1, height: 8193))) == nil,
      "reject encoded excessive height before thumbnail creation")
    for type in ["com.compuserve.gif", "public.tiff"] {
      check(
        GameGalleryThumbnails.decode(encoded(pixels(width: 8, height: 8), type: type, frames: 2))
          == nil, "reject every multi-frame source: \(type)")
    }
    var capped = png
    capped.append(Data(repeating: 0, count: GameGalleryThumbnails.encodedByteLimit - png.count))
    check(GameGalleryThumbnails.decode(capped) != nil, "encoded byte cap is inclusive")
    capped.append(0)
    check(GameGalleryThumbnails.decode(capped) == nil, "reject one byte above encoded cap")

    let oriented = GameGalleryThumbnails.decode(
      encoded(pixels(width: 1536, height: 384), type: "public.jpeg", orientation: 6))
    check(
      oriented?.width == 96 && oriented?.height == 384, "honor EXIF orientation without cropping")
    let wide = GameGalleryThumbnails.decode(encoded(pixels(width: 1536, height: 384)))!
    let context = CGContext(
      data: nil, width: wide.width, height: wide.height, bitsPerComponent: 8,
      bytesPerRow: wide.width * 4, space: CGColorSpaceCreateDeviceRGB(),
      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
    )!
    context.draw(wide, in: CGRect(x: 0, y: 0, width: wide.width, height: wide.height))
    let rgba = context.data!.assumingMemoryBound(to: UInt8.self)
    let left = (wide.height / 2 * wide.width + 1) * 4
    let right = (wide.height / 2 * wide.width + wide.width - 2) * 4
    check(rgba[left] > 240 && rgba[left + 2] < 10, "retain left image edge")
    check(rgba[right] < 10 && rgba[right + 2] > 240, "retain right image edge")
  }

  private static func testDefaultDecoder() async {
    let loader = GameGalleryThumbnails()
    let png = encoded(pixels(width: 1536, height: 384))
    var reads = 0
    let image = await loader.image(for: "real-imageio") {
      MainActor.assertIsolated()
      reads += 1
      return png
    }
    check(image != nil && reads == 1, "production initializer uses real ImageIO")
    let cached = await loader.image(for: "real-imageio") {
      preconditionFailure("cache hit cannot read")
    }
    check(cached != nil && loader.cachedEntryCount == 1, "production thumbnail cached")
    check(loader.cachedByteCount > 0, "cache accounts real decoded bytes")
    loader.clear()
    check(loader.cachedEntryCount == 0 && loader.cachedByteCount == 0, "clear empties cache")
  }

  /// Observe exact queue state rather than sleeping for an assumed scheduling interval.
  private static func waitForWaiters(_ count: Int, in loader: GameGalleryThumbnails) async {
    for _ in 0..<100_000 {
      if loader.pendingWaiterCount == count { return }
      await Task.yield()
    }
    preconditionFailure("queue did not reach \(count) waiting callers")
  }

  private static func testCoalescingAndSerialization() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    var reads: [String] = []
    let first = Task {
      await loader.image(for: "a") {
        MainActor.assertIsolated()
        reads.append("a")
        return Data([1])
      }
    }
    await gate.waitForStart(1)
    let duplicate = Task {
      await loader.image(for: "a") {
        preconditionFailure("coalesced duplicate must never read")
      }
    }
    await waitForWaiters(2, in: loader)
    let second = Task {
      await loader.image(for: "b") {
        MainActor.assertIsolated()
        reads.append("b")
        return Data([2])
      }
    }
    await waitForWaiters(3, in: loader)
    let queuedDuplicate = Task {
      await loader.image(for: "b") {
        preconditionFailure("queued duplicate must never read")
      }
    }
    await waitForWaiters(4, in: loader)
    check(reads == ["a"], "queued readers remain lazy throughout the active decode")
    var state = await gate.snapshot()
    check(state.started == 1 && state.active == 1, "only one active decode")
    await gate.release(1)
    await gate.waitForStart(2)
    check(reads == ["a", "b"], "FIFO reads only when their decode turn arrives")
    let firstImage = await first.value
    let duplicateImage = await duplicate.value
    check(firstImage != nil && duplicateImage != nil, "active duplicate receives shared result")
    state = await gate.snapshot()
    check(state.maximum == 1 && state.inputs == [Data([1]), Data([2])], "serialized owned inputs")
    await gate.release(2)
    let secondImage = await second.value
    let queuedImage = await queuedDuplicate.value
    check(secondImage != nil && queuedImage != nil, "queued duplicate receives shared result")
    check(loader.pendingWaiterCount == 0, "all coalesced continuations finish")
    let cached = await loader.image(for: "a") { preconditionFailure("cache cannot read") }
    check(cached != nil && reads.count == 2, "cache bypasses read and decode")
  }

  private static func testCancellationCoalescing() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    let alreadyCancelled = Task {
      await loader.image(for: "pre-cancelled") {
        preconditionFailure("cancelled caller cannot read")
      }
    }
    alreadyCancelled.cancel()
    let skipped = await alreadyCancelled.value
    check(skipped == nil && loader.pendingWaiterCount == 0, "already-cancelled caller never queues")
    var reads = 0
    let first = Task {
      await loader.image(for: "shared") {
        reads += 1
        return Data([1])
      }
    }
    await gate.waitForStart(1)
    let survivor = Task {
      await loader.image(for: "shared") { preconditionFailure("surviving duplicate cannot reread") }
    }
    await waitForWaiters(2, in: loader)
    first.cancel()
    let cancelled = await first.value
    check(cancelled == nil, "cancelled active caller resolves before decode finishes")
    check(loader.pendingWaiterCount == 1, "cancel only one coalesced waiter")
    let state = await gate.snapshot()
    check(
      state.started == 1 && state.active == 1, "one cancellation cannot interrupt shared decode")
    await gate.release(1)
    let survived = await survivor.value
    check(survived != nil && reads == 1, "live coalesced caller retains the shared result")
    check(
      loader.pendingWaiterCount == 0 && loader.cachedEntryCount == 1, "shared result remains cached"
    )
  }

  private static func testQueuedCancellation() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    var reads: [String] = []
    let blocker = Task {
      await loader.image(for: "blocker") {
        reads.append("blocker")
        return Data([1])
      }
    }
    await gate.waitForStart(1)
    let firstQueued = Task {
      await loader.image(for: "shared-queued") {
        reads.append("shared")
        return Data([2])
      }
    }
    await waitForWaiters(2, in: loader)
    let survivingQueued = Task {
      await loader.image(for: "shared-queued") {
        preconditionFailure("queued duplicate cannot reread")
      }
    }
    await waitForWaiters(3, in: loader)
    let offscreen = Task {
      await loader.image(for: "offscreen") {
        preconditionFailure("unobserved queued request cannot read")
      }
    }
    await waitForWaiters(4, in: loader)
    firstQueued.cancel()
    offscreen.cancel()
    let cancelledShared = await firstQueued.value
    let cancelledOffscreen = await offscreen.value
    check(
      cancelledShared == nil && cancelledOffscreen == nil, "queued cancellations resolve promptly")
    check(loader.pendingWaiterCount == 2, "drop unobserved request but preserve live shared waiter")
    check(reads == ["blocker"], "cancelled queued readers have not run")
    let visibleAgain = Task {
      await loader.image(for: "offscreen") {
        reads.append("visible-again")
        return Data([3])
      }
    }
    await waitForWaiters(3, in: loader)
    await gate.release(1)
    await gate.waitForStart(2)
    check(reads == ["blocker", "shared"], "preserve FIFO position of surviving coalesced request")
    await gate.release(2)
    await gate.waitForStart(3)
    check(
      reads == ["blocker", "shared", "visible-again"], "cancelled id can enqueue a fresh reader")
    await gate.release(3)
    let blockerImage = await blocker.value
    let survivorImage = await survivingQueued.value
    let visibleImage = await visibleAgain.value
    check(
      blockerImage != nil && survivorImage != nil && visibleImage != nil, "all live callers finish")
    let state = await gate.snapshot()
    check(
      state.started == 3 && state.maximum == 1,
      "dropped request never decodes or breaks serialization")
    check(loader.pendingWaiterCount == 0, "queued cancellation leaves no continuations behind")
  }

  private static func testCancellationBeforeActorHop() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    let blocker = Task { await loader.image(for: "blocker") { Data([1]) } }
    await gate.waitForStart(1)
    var offscreen: Task<Image?, Never>?
    let trigger = Task {
      await loader.image(for: "cancel-next") {
        // The cancellation handler's MainActor task cannot run until this worker yields.
        // The next queued read still must see cancellation synchronously and be skipped.
        offscreen!.cancel()
        blocker.cancel()  // Its continuation was resumed, but its result has not returned.
        return nil
      }
    }
    await waitForWaiters(2, in: loader)
    offscreen = Task {
      await loader.image(for: "offscreen") {
        preconditionFailure("cancellation must suppress queued read before actor-hop cleanup")
      }
    }
    await waitForWaiters(3, in: loader)
    await gate.release(1)
    let blockerImage = await blocker.value
    let triggerImage = await trigger.value
    let offscreenImage = await offscreen!.value
    check(blockerImage == nil, "cancellation invalidates completed but not yet returned result")
    check(
      triggerImage == nil && offscreenImage == nil, "queued cancellation precedes actor-hop cleanup"
    )
    check(
      loader.pendingWaiterCount == 0, "deferred cancellation cleanup does not double-resume waiters"
    )
    let state = await gate.snapshot()
    check(state.started == 1, "cancelled next request never reaches decoder")
  }

  private static func testActiveCancellationAndClear() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    var reads: [String] = []
    let abandoned = Task {
      await loader.image(for: "same-id") {
        reads.append("abandoned")
        return Data([1])
      }
    }
    await gate.waitForStart(1)
    abandoned.cancel()
    let cancelled = await abandoned.value
    check(
      cancelled == nil && loader.pendingWaiterCount == 0, "last active waiter resolves immediately")
    var state = await gate.snapshot()
    check(state.active == 1, "last waiter cancellation cannot stop active decoder")
    let replacement = Task {
      await loader.image(for: "same-id") {
        preconditionFailure("clear cancels replacement before read")
      }
    }
    await waitForWaiters(1, in: loader)
    check(reads == ["abandoned"], "replacement waits for abandoned active decode")
    // Race a second caller's cancellation cleanup with clear and another same-id request.
    replacement.cancel()
    loader.clear()
    let fresh = Task {
      await loader.image(for: "same-id") {
        reads.append("fresh")
        return Data([2])
      }
    }
    await waitForWaiters(1, in: loader)
    let replacementImage = await replacement.value
    check(replacementImage == nil, "clear and cancellation safely share continuation cleanup")
    check(reads == ["abandoned"], "clear after cancellation retains the active serialization tail")
    await gate.release(1)
    await gate.waitForStart(2)
    check(loader.cachedEntryCount == 0, "abandoned result cannot populate reset cache")
    check(
      reads == ["abandoned", "fresh"], "old cancellation cannot discard fresh-generation reader")
    await gate.release(2)
    let freshImage = await fresh.value
    check(
      freshImage != nil && loader.cachedEntryCount == 1,
      "fresh generation survives cancellation cleanup")
    state = await gate.snapshot()
    check(
      state.started == 2 && state.maximum == 1, "cancellation plus reset cannot overlap decoding")
  }

  private static func testOwnedBytes() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in
      precondition(!Thread.isMainThread, "decoder must not execute on the main thread")
      return await gate.decode(data)
    }
    let storage = UnsafeMutableRawPointer.allocate(byteCount: 1024, alignment: 1)
    storage.initializeMemory(as: UInt8.self, repeating: 7, count: 1024)
    defer { storage.deallocate() }
    let borrowed = Data(bytesNoCopy: storage, count: 1024, deallocator: .none)
    let request = Task {
      await loader.image(for: "borrowed") {
        MainActor.assertIsolated()
        return borrowed
      }
    }
    await gate.waitForStart(1)
    storage.storeBytes(of: UInt8(9), as: UInt8.self)
    let state = await gate.snapshot()
    check(
      state.inputs.first == Data(repeating: 7, count: 1024), "decode owns a copy of borrowed input")
    await gate.release(1)
    let image = await request.value
    check(image != nil, "owned bytes remain valid throughout asynchronous decode")
  }

  private static func testFailures() async {
    let gate = DecodeGate(image: nil)
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    let absent = await loader.image(for: "missing") { nil }
    let empty = await loader.image(for: "empty") { Data() }
    let huge = await loader.image(for: "huge") {
      Data(repeating: 0, count: GameGalleryThumbnails.encodedByteLimit + 1)
    }
    check(absent == nil && empty == nil && huge == nil, "reader errors and byte bounds fail closed")
    let initial = await gate.snapshot()
    check(initial.started == 0, "invalid encoded lengths never reach injected decoder")
    var reads = 0
    let first = Task {
      await loader.image(for: "bad") {
        reads += 1
        return Data([1])
      }
    }
    await gate.waitForStart(1)
    let duplicate = Task {
      await loader.image(for: "bad") { preconditionFailure("failed decode still coalesces") }
    }
    await waitForWaiters(2, in: loader)
    await gate.release(1)
    let firstImage = await first.value
    let duplicateImage = await duplicate.value
    check(firstImage == nil && duplicateImage == nil, "failed decode finishes every waiter")
    check(loader.cachedEntryCount == 0 && loader.cachedByteCount == 0, "failure is not cached")
    let retry = Task {
      await loader.image(for: "bad") {
        reads += 1
        return Data([2])
      }
    }
    await gate.waitForStart(2)
    await gate.release(2)
    let retried = await retry.value
    check(retried == nil && reads == 2, "a subsequent request may retry a failed decode")

    let oversized = pixels(width: 385, height: 1)
    let badDecoder = GameGalleryThumbnails { _ in oversized }
    let invalidThumbnail = await badDecoder.image(for: "oversized-thumbnail") { Data([1]) }
    check(invalidThumbnail == nil && badDecoder.cachedEntryCount == 0, "validate decoder output")

    let reentrant = GameGalleryThumbnails()
    let clearedDuringRead = await reentrant.image(for: "reentrant") {
      reentrant.clear()
      return encoded(pixels(width: 8, height: 4))
    }
    check(clearedDuringRead == nil, "reader-triggered clear also invalidates its own request")
  }

  private static func testClear() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    var reads: [String] = []
    let old = Task {
      await loader.image(for: "same-id") {
        reads.append("old")
        return Data([1])
      }
    }
    await gate.waitForStart(1)
    let oldDuplicate = Task {
      await loader.image(for: "same-id") { preconditionFailure("old duplicate cannot read") }
    }
    await waitForWaiters(2, in: loader)
    let queued = Task {
      await loader.image(for: "queued-old") { preconditionFailure("cleared queue cannot read") }
    }
    await waitForWaiters(3, in: loader)
    loader.clear()
    let oldImage = await old.value
    let oldDuplicateImage = await oldDuplicate.value
    let queuedImage = await queued.value
    check(
      oldImage == nil && oldDuplicateImage == nil && queuedImage == nil, "clear cancels all waiters"
    )
    check(
      loader.pendingWaiterCount == 0 && loader.cachedByteCount == 0, "reset queue and accounting")
    var state = await gate.snapshot()
    check(state.active == 1, "old synchronous-equivalent decode is still running after clear")

    let middle = Task {
      await loader.image(for: "middle") { preconditionFailure("second clear cancels fresh queue") }
    }
    await waitForWaiters(1, in: loader)
    check(reads == ["old"], "new generation must wait for old decode even after clear")
    loader.clear()
    let middleImage = await middle.value
    check(middleImage == nil, "repeated clear cancels intermediate generation")
    let fresh = Task {
      await loader.image(for: "same-id") {
        MainActor.assertIsolated()
        reads.append("fresh")
        return Data([2])
      }
    }
    await waitForWaiters(1, in: loader)
    check(reads == ["old"], "repeated clear cannot discard the serialization tail")
    state = await gate.snapshot()
    check(state.started == 1 && state.maximum == 1, "no concurrent decode across generations")
    await gate.release(1)
    await gate.waitForStart(2)
    check(loader.cachedEntryCount == 0, "late old decode cannot repopulate cache")
    check(
      reads == ["old", "fresh"], "same-id request uses fresh reader only after old decode finishes")
    await gate.release(2)
    let freshImage = await fresh.value
    check(freshImage != nil && loader.cachedEntryCount == 1, "fresh generation succeeds")
    state = await gate.snapshot()
    check(state.maximum == 1 && state.started == 2, "serialization survives multiple resets")
    loader.clear()
    let reload = Task {
      await loader.image(for: "same-id") {
        reads.append("reload")
        return Data([3])
      }
    }
    await gate.waitForStart(3)
    await gate.release(3)
    let reloadedImage = await reload.value
    check(
      reloadedImage != nil && reads == ["old", "fresh", "reload"], "clear invalidates cached hits")
  }

  private static func testClearAfterCompletion() async {
    let gate = DecodeGate(image: pixels(width: 8, height: 4))
    let loader = GameGalleryThumbnails { data in await gate.decode(data) }
    let first = Task { await loader.image(for: "completed") { Data([1]) } }
    await gate.waitForStart(1)
    let next = Task {
      await loader.image(for: "clear-next") {
        // The worker resumed the first continuation, but has not yielded MainActor yet.
        // This reset must invalidate even that already-completed, not-yet-returned result.
        check(loader.cachedEntryCount == 1, "prior request completed before reset reader")
        loader.clear()
        return nil
      }
    }
    await waitForWaiters(2, in: loader)
    await gate.release(1)
    let firstImage = await first.value
    let nextImage = await next.value
    check(firstImage == nil, "clear rejects completed result whose caller resumes after reset")
    check(nextImage == nil && loader.cachedEntryCount == 0, "reset reader leaves no stale cache")
  }

  private static func testCacheLimits() async {
    let small = pixels(width: 8, height: 4)
    let loader = GameGalleryThumbnails { _ in small }
    var reads: [String: Int] = [:]
    func fetch(_ id: String) async -> Image? {
      await loader.image(for: id) {
        reads[id, default: 0] += 1
        return Data([1])
      }
    }
    for index in 0..<32 {
      let image = await fetch("\(index)")
      check(image != nil, "fill entry-limited cache")
    }
    check(loader.cachedEntryCount == 32, "allow exactly 32 entries")
    check(
      loader.cachedByteCount == 32 * small.bytesPerRow * small.height, "actual row-byte accounting")
    _ = await fetch("0")
    _ = await fetch("32")
    _ = await fetch("0")
    check(reads["0"] == 1, "cache hit refreshes least-recently-used position")
    _ = await fetch("1")
    check(reads["1"] == 2 && loader.cachedEntryCount == 32, "evict least recent entry at count cap")

    // Deliberately padded rows distinguish the actual CGImage cost from width*height*4.
    let padded = pixels(width: 384, height: 384, bytesPerRow: 4096)
    let byteCost = padded.bytesPerRow * padded.height
    check(byteCost > padded.width * padded.height * 4, "fixture has padded row storage")
    let budget = GameGalleryThumbnails { _ in padded }
    var budgetReads: [String: Int] = [:]
    func fetchBudget(_ id: String) async -> Image? {
      await budget.image(for: id) {
        budgetReads[id, default: 0] += 1
        return Data([1])
      }
    }
    let capacity = min(32, GameGalleryThumbnails.cacheByteLimit / byteCost)
    check(capacity < 32, "byte budget binds before entry count")
    for index in 0..<capacity {
      let image = await fetchBudget("\(index)")
      check(image != nil, "fill byte-limited cache")
    }
    check(budget.cachedByteCount == capacity * byteCost, "account padded rows, not nominal pixels")
    _ = await fetchBudget("0")
    _ = await fetchBudget("extra")
    _ = await fetchBudget("0")
    check(budgetReads["0"] == 1, "recent item survives byte eviction")
    _ = await fetchBudget("1")
    check(budgetReads["1"] == 2, "byte budget evicts oldest item")
    check(
      budget.cachedEntryCount == capacity && budget.cachedByteCount <= 16 * 1024 * 1024,
      "cache never exceeds 16 MiB actual storage")
    budget.clear()
    check(
      budget.cachedEntryCount == 0 && budget.cachedByteCount == 0, "clear resets budget accounting")
  }
}
