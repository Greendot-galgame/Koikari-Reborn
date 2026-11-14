// Synthetic Swift FFI stubs only. Compile without Koikari.h or the Rust library.
// No original game resources, live Host, Metal renderer, preferences or saves are used.
import Foundation
import ImageIO
import SwiftUI
import os

private final class RenderGate: Sendable {
  private struct State {
    var started = false
    var observers: [CheckedContinuation<Void, Never>] = []
  }
  private let state = OSAllocatedUnfairLock(initialState: State())
  private let releaseSignal = DispatchSemaphore(value: 0)

  func hold() {
    let observers = state.withLock { state in
      state.started = true
      let observers = state.observers
      state.observers.removeAll()
      return observers
    }
    for observer in observers { observer.resume() }
    releaseSignal.wait()
  }

  func waitForStart() async {
    await withCheckedContinuation { continuation in
      let ready = state.withLock { state in
        if state.started { return true }
        state.observers.append(continuation)
        return false
      }
      if ready { continuation.resume() }
    }
  }

  func release() { releaseSignal.signal() }
}

private final class PreviewBackend: Sendable {
  enum Response: Sendable {
    case bytes(Data)
    case invalidLength(Int)
    case null(Int)
  }

  struct Request: Equatable {
    let host: UInt
    let slot: UInt32
    let revision: String
  }

  struct Snapshot {
    let requests: [Request]
    let renders: Int
    let freedLeases: Int
    let liveLeases: Int
    let activeRenders: Int
    let byteFreeLengths: [Int]
  }

  private struct Lease {
    let response: Response
    let gate: RenderGate?
  }

  private struct State {
    var succeeds = true
    var response: Response = .bytes(Data([1, 2, 3]))
    var gate: RenderGate?
    var nextLease: UInt = 16
    var requests: [Request] = []
    var leases: [UInt: Lease] = [:]
    var active: Set<UInt> = []
    var renderThreads: [UInt: ObjectIdentifier] = [:]
    var renders = 0
    var freedLeases = 0
    var allocations: [UInt: Int] = [:]
    var byteFreeLengths: [Int] = []
  }

  private let state = OSAllocatedUnfairLock(initialState: State())

  func reset(
    succeeds: Bool = true, response: Response = .bytes(Data([1, 2, 3])), gate: RenderGate? = nil
  ) {
    state.withLock { state in
      precondition(state.leases.isEmpty && state.active.isEmpty && state.allocations.isEmpty)
      state = State()
      state.succeeds = succeeds
      state.response = response
      state.gate = gate
    }
  }

  func request(host: UInt, slot: UInt32, revision: String) -> OpaquePointer? {
    let identity: UInt? = state.withLock { state in
      state.requests.append(Request(host: host, slot: slot, revision: revision))
      guard state.succeeds else { return nil }
      let identity = state.nextLease
      state.nextLease += 16
      state.leases[identity] = Lease(response: state.response, gate: state.gate)
      return identity
    }
    return identity.flatMap { OpaquePointer(bitPattern: $0) }
  }

  func render(_ pointer: OpaquePointer, length: UnsafeMutablePointer<Int>) -> UnsafeMutablePointer<
    UInt8
  >? {
    precondition(!Thread.isMainThread, "save PNG must render off-main")
    let identity = UInt(bitPattern: pointer)
    let thread = ObjectIdentifier(Thread.current)
    let lease = state.withLock { state in
      precondition(!state.active.contains(identity), "lease cannot render concurrently")
      let lease = state.leases[identity]!
      state.active.insert(identity)
      state.renderThreads[identity] = thread
      state.renders += 1
      return lease
    }
    defer { state.withLock { _ = $0.active.remove(identity) } }
    lease.gate?.hold()
    let bytes: UnsafeMutablePointer<UInt8>
    switch lease.response {
    case .bytes(let data):
      length.pointee = data.count
      bytes = .allocate(capacity: max(1, data.count))
      for (index, byte) in data.enumerated() { bytes[index] = byte }
    case .invalidLength(let count):
      // Deliberately tiny allocation: the Swift guard must reject the reported length
      // before copying. Deallocation must nevertheless receive the original count.
      length.pointee = count
      bytes = .allocate(capacity: 1)
      bytes[0] = 255
    case .null(let count):
      length.pointee = count
      return nil
    }
    let address = UInt(bitPattern: bytes)
    let count = length.pointee
    state.withLock { $0.allocations[address] = count }
    return bytes
  }

  func freeBytes(_ bytes: UnsafeMutablePointer<UInt8>, count: Int) {
    let address = UInt(bitPattern: bytes)
    state.withLock { state in
      let original = state.allocations.removeValue(forKey: address)
      precondition(original == count, "return original allocation count exactly once")
      state.byteFreeLengths.append(count)
    }
    bytes.deallocate()
  }

  func freeLease(_ pointer: OpaquePointer) {
    let identity = UInt(bitPattern: pointer)
    let thread = ObjectIdentifier(Thread.current)
    state.withLock { state in
      precondition(!state.active.contains(identity), "lease must outlive its borrowed render")
      if let owner = state.renderThreads.removeValue(forKey: identity) {
        precondition(owner == thread, "transferred lease is freed on its rendering worker")
      }
      precondition(
        state.leases.removeValue(forKey: identity) != nil, "free every lease exactly once")
      state.freedLeases += 1
    }
  }

  func snapshot() -> Snapshot {
    state.withLock {
      Snapshot(
        requests: $0.requests, renders: $0.renders, freedLeases: $0.freedLeases,
        liveLeases: $0.leases.count, activeRenders: $0.active.count,
        byteFreeLengths: $0.byteFreeLengths)
    }
  }
}

private let backend = PreviewBackend()

// Keep the exact imported C spellings so this test needs no native library.
// swift-format-ignore: AlwaysUseLowerCamelCase
@MainActor
func kr_save_preview_request(
  _ handle: OpaquePointer?, _ slot: UInt32, _ revision: UnsafePointer<CChar>?
) -> OpaquePointer? {
  MainActor.assertIsolated()
  precondition(Thread.isMainThread, "capture the scene on the main thread")
  return backend.request(
    host: UInt(bitPattern: handle!), slot: slot, revision: String(cString: revision!))
}

// swift-format-ignore: AlwaysUseLowerCamelCase
func kr_save_preview_png(
  _ preview: OpaquePointer?, _ length: UnsafeMutablePointer<Int>?
) -> UnsafeMutablePointer<UInt8>? {
  backend.render(preview!, length: length!)
}

// swift-format-ignore: AlwaysUseLowerCamelCase
func kr_save_preview_free(_ preview: OpaquePointer?) {
  if let preview { backend.freeLease(preview) }
}

// swift-format-ignore: AlwaysUseLowerCamelCase
func kr_bytes_free(_ bytes: UnsafeMutablePointer<UInt8>?, _ length: Int) {
  if let bytes { backend.freeBytes(bytes, count: length) }
}

@main
@MainActor
struct SaveThumbnailTests {
  private static var assertions = 0
  private static let host = OpaquePointer(bitPattern: 0x1234)!

  static func main() async {
    testMissingAndAbandoned()
    await testOwnedBytesAndOnce()
    await testByteBounds()
    await testExclusiveRenderLifetime()
    await testPipelineLifetime()
    await testReentrantClear()
    print("\(assertions) save thumbnail assertions passed (synthetic FFI only)")
  }

  private static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    precondition(condition(), message)
    assertions += 1
  }

  private static func prepare(slot: UInt32 = 9, revision: String = "revision-雪")
    -> GameGalleryThumbnails.Work?
  {
    SaveThumbnailReader.prepare(handle: host, slot: slot, revision: revision)
  }

  private static func testMissingAndAbandoned() {
    backend.reset(succeeds: false)
    check(prepare() == nil, "missing or stale save cannot create work")
    var state = backend.snapshot()
    check(
      state.requests == [.init(host: UInt(bitPattern: host), slot: 9, revision: "revision-雪")],
      "forward exact Host, slot and UTF-8 revision synchronously")
    check(
      state.renders == 0 && state.freedLeases == 0,
      "failed capture neither renders nor frees a missing lease")
    backend.reset()
    do {
      let work = prepare()
      check(work != nil, "successful capture returns owned work")
      state = backend.snapshot()
      check(state.liveLeases == 1 && state.renders == 0, "preparation does not render eagerly")
      withExtendedLifetime(work) {}
    }
    state = backend.snapshot()
    check(
      state.liveLeases == 0 && state.freedLeases == 1, "uninvoked work frees abandoned lease once")
    check(
      state.renders == 0 && state.byteFreeLengths.isEmpty, "abandoned work allocates no image bytes"
    )
  }

  private static func testOwnedBytesAndOnce() async {
    backend.reset(response: .bytes(Data([9, 8, 7, 6])))
    do {
      let work = prepare()!
      let data = await Task.detached { work() }.value
      var state = backend.snapshot()
      check(data == Data([9, 8, 7, 6]), "copy PNG before freeing backend allocation")
      check(state.byteFreeLengths == [4], "free successful bytes with original length")
      check(
        state.liveLeases == 0 && state.freedLeases == 1,
        "worker frees the borrowed lease after PNG returns"
      )
      let second = await Task.detached { work() }.value
      state = backend.snapshot()
      check(second == nil && state.renders == 1, "defensive once guard rejects repeated invocation")
      withExtendedLifetime(work) {}
    }
    let state = backend.snapshot()
    check(
      state.liveLeases == 0 && state.freedLeases == 1,
      "completed work frees its borrowed lease exactly once")
  }

  private static func invokePrepared() async -> Data? {
    guard let work = prepare() else { return nil }
    return await Task.detached { work() }.value
  }

  private static func testByteBounds() async {
    for count in [0, -1, 1024 * 1024 + 1, Int.max] {
      backend.reset(response: .invalidLength(count))
      let data = await invokePrepared()
      let state = backend.snapshot()
      check(data == nil, "reject invalid returned byte count \(count) before any copy")
      check(
        state.byteFreeLengths == [count], "invalid buffer returns exact original count \(count)")
      check(state.freedLeases == 1 && state.liveLeases == 0, "failure releases owned lease")
    }
    for count in [0, 20] {
      backend.reset(response: .null(count))
      let data = await invokePrepared()
      let state = backend.snapshot()
      check(
        data == nil && state.byteFreeLengths.isEmpty,
        "null bytes fail quietly without freeing a fabricated pointer")
      check(state.freedLeases == 1, "null PNG result still releases its lease")
    }
    for count in [1, 1024 * 1024] {
      backend.reset(response: .bytes(Data(repeating: 3, count: count)))
      let data = await invokePrepared()
      let state = backend.snapshot()
      check(
        data?.count == count && data?.first == 3 && data?.last == 3,
        "PNG byte bounds include \(count)")
      check(
        state.byteFreeLengths == [count] && state.freedLeases == 1,
        "inclusive bound releases bytes and lease")
    }
  }

  private static func startPrepared() -> Task<Data?, Never> {
    let work = prepare()!
    return Task.detached { work() }
  }

  private static func testExclusiveRenderLifetime() async {
    let gate = RenderGate()
    backend.reset(gate: gate)
    do {
      let work = prepare()!
      let first = Task.detached { work() }
      await gate.waitForStart()
      var state = backend.snapshot()
      check(
        state.activeRenders == 1 && state.freedLeases == 0,
        "borrowed lease remains alive during PNG render")
      let duplicate = await Task.detached { work() }.value
      state = backend.snapshot()
      check(
        duplicate == nil && state.renders == 1 && state.activeRenders == 1,
        "concurrent invocation cannot enter render twice")
      first.cancel()
      check(backend.snapshot().freedLeases == 0, "task cancellation cannot free an in-use lease")
      gate.release()
      let firstData = await first.value
      check(firstData == Data([1, 2, 3]), "uncancellable synchronous render finishes safely")
      withExtendedLifetime(work) {}
    }
    check(
      backend.snapshot().freedLeases == 1,
      "completed render and final owner release cannot double-free")
    let soleOwnerGate = RenderGate()
    backend.reset(gate: soleOwnerGate)
    let onlyWorker = startPrepared()
    await soleOwnerGate.waitForStart()
    check(backend.snapshot().liveLeases == 1, "worker alone retains captured lease")
    soleOwnerGate.release()
    _ = await onlyWorker.value
    check(
      backend.snapshot().freedLeases == 1, "worker completion releases its sole lease ownership")
  }

  private static func waitForWaiters(_ count: Int, in loader: GameGalleryThumbnails) async {
    for _ in 0..<100_000 {
      if loader.pendingWaiterCount == count { return }
      await Task.yield()
    }
    preconditionFailure("queue did not reach \(count) waiting callers")
  }

  private static func waitForFree() async {
    for _ in 0..<100_000 {
      if backend.snapshot().freedLeases == 1 { return }
      await Task.yield()
    }
    preconditionFailure("completed worker did not free lease")
  }

  private static func testPipelineLifetime() async {
    let gate = RenderGate()
    backend.reset(gate: gate)
    let loader = GameGalleryThumbnails { _ in nil }
    let old = Task { await loader.image(for: "old", prepare: { prepare() }) }
    await gate.waitForStart()
    let queued = Task {
      await loader.image(for: "queued", prepare: { prepare(slot: 10) })
    }
    await waitForWaiters(2, in: loader)
    check(
      backend.snapshot().requests.count == 1,
      "queued save does not capture a lease before its worker turn")
    queued.cancel()
    let queuedImage = await queued.value
    check(
      queuedImage == nil && backend.snapshot().requests.count == 1,
      "offscreen save cancellation suppresses lease capture")
    loader.clear()
    let oldImage = await old.value
    check(oldImage == nil, "clear immediately resolves old save waiter")
    check(
      backend.snapshot().liveLeases == 1 && backend.snapshot().freedLeases == 0,
      "clear cannot free actively rendered save lease")
    gate.release()
    await waitForFree()
    check(
      backend.snapshot().freedLeases == 1 && loader.cachedEntryCount == 0,
      "stale worker releases its lease without caching")
  }

  private static func testReentrantClear() async {
    backend.reset()
    let loader = GameGalleryThumbnails { _ in preconditionFailure("abandoned save cannot decode") }
    let image = await loader.image(
      for: "reentrant",
      prepare: {
        let work = prepare()
        loader.clear()
        return work
      })
    let state = backend.snapshot()
    check(image == nil && state.renders == 0, "clear during capture prevents PNG render")
    check(
      state.freedLeases == 1 && state.liveLeases == 0,
      "clear during capture releases uninvoked save lease")
  }
}
