import AVFoundation
@preconcurrency import MetalKit
import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
  import AppKit
#endif

struct Clip: Decodable, Sendable, Equatable {
  var asset: String
  var sequence: UInt64
  var looping: Bool
  var volume: Float?
  var fadeMs: UInt64?
  enum CodingKeys: String, CodingKey {
    case asset, sequence, looping, volume
    case fadeMs = "fade_ms"
  }
}
struct StoryChoice: Decodable, Identifiable {
  var id: UInt64
  var text: String
  var enabled: Bool
}
struct StoryFlow: Decodable {
  var choices: [StoryChoice]?
  var waitMs: UInt64?
  var waitSkippable: Bool?
  var ending: String?
  var replay: Bool?
  var acceptsAdvanceWait: Bool { (waitMs ?? 0) == 0 || waitSkippable == true }
  enum CodingKeys: String, CodingKey {
    case choices, ending, replay
    case waitMs = "wait_ms"
    case waitSkippable = "wait_skippable"
  }
}
struct StoryProgress: Decodable {
  var events: [String]?
  var cg: [String]?
  var endings: [String]?
}
struct VideoClip: Decodable, Equatable {
  var asset: String
  var sequence: UInt64
  var skippable: Bool
  var looping: Bool
}
struct CollectionEntry: Decodable, Identifiable {
  var id: String
  var title: String
  var asset: String
  var unlocked: Bool
}
struct Dialogue: Decodable {
  struct Visual: Decodable {
    var elapsedMs: UInt64
    enum CodingKeys: String, CodingKey { case elapsedMs = "elapsed_ms" }
  }
  var speaker: String
  var text: String
  var messageVisible: Bool
  var bgm: Clip
  var voice: Clip
  var se: Clip
  var video: VideoClip?
  var credits: CreditsState?
  var visual: Visual
  enum CodingKeys: String, CodingKey {
    case speaker, text, bgm, voice, se, video, credits, visual
    case messageVisible = "message_visible"
  }
}
struct SpritePlacement: Decodable {
  var position: String
}
struct Snapshot: Decodable {
  var scene: Dialogue
  var sprites: [String: SpritePlacement]
  var title: Bool
  var finished: Bool
  var pages: UInt64
  var blocked: String?
  var warnings: [String]
  var history: [String]
  var interlude: Interlude?
  var archives: [String]?
  var flow: StoryFlow?
  var progress: StoryProgress?
}
struct Interlude: Decodable {
  var background: String
  var bgm: String
}
struct PastPage: Decodable, Identifiable {
  var page: UInt64
  var speaker: String
  var text: String
  var interlude: Bool
  var id: UInt64 { page }
}
struct SaveSlot: Decodable, Identifiable {
  var slot: Int
  var exists: Bool
  var compatible: Bool
  var savedAt: UInt64
  var pages: UInt64
  var speaker: String
  var text: String
  var error: String?
  var previewRevision: String? = nil
  var id: Int { slot }
  var name: String { slot == 9 ? "自动进度" : "手动槽位 \(slot)" }
  enum CodingKeys: String, CodingKey {
    case slot, exists, compatible, pages, speaker, text, error
    case savedAt = "saved_at"
    case previewRevision = "preview_revision"
  }
}

// BEGIN MEDIA LIFECYCLE POLICY
// Pure shared policy: no player, notification, UI, clock or session side effects.
// The no-UI regression test compiles this exact region from the app source.
struct MediaLifecyclePolicy: Equatable {
  enum ResumeReason: Equatable {
    case interruption, routeLost, activationFailed
  }
  private(set) var foreground = true
  private(set) var applicationActive = true
  private(set) var blocked = false
  private(set) var interrupted = false
  private(set) var resumeReason: ResumeReason?

  var requiresUserResume: Bool { resumeReason != nil }
  var canRequestUserResume: Bool {
    foreground && applicationActive && !blocked && !interrupted && requiresUserResume
  }
  var permitsPlayback: Bool {
    foreground && applicationActive && !blocked && !interrupted && !requiresUserResume
  }

  mutating func setForeground(_ value: Bool) { foreground = value }
  mutating func setApplicationActive(_ value: Bool) { applicationActive = value }
  mutating func setBlocked(_ value: Bool) { blocked = value }
  mutating func beginInterruption() { interrupted = true }
  mutating func endInterruption(shouldResume: Bool) {
    interrupted = false
    if !shouldResume && resumeReason == nil { resumeReason = .interruption }
  }
  mutating func loseOutputRoute() { resumeReason = .routeLost }
  mutating func activationFailed() { resumeReason = .activationFailed }
  @discardableResult mutating func requestUserResume() -> Bool {
    guard canRequestUserResume else { return false }
    resumeReason = nil
    return true
  }
}
// END MEDIA LIFECYCLE POLICY

// BEGIN AUDIO FAILURE POLICY
enum AudioPlaybackGate {
  static func allows(muted: Bool, volume: Float, outputFailed: Bool) -> Bool {
    !muted && volume.isFinite && volume > 0 && !outputFailed
  }
}
struct AudioFailurePolicy {
  private var failed = Set<String>()
  private var skipped = Set<String>()
  mutating func record(_ asset: String) { failed.insert(asset) }
  func mayDecode(_ asset: String) -> Bool { !failed.contains(asset) }
  func needsAcknowledgement(_ asset: String) -> Bool {
    failed.contains(asset) && !skipped.contains(asset)
  }
  mutating func skip(_ asset: String) { skipped.insert(asset) }
  mutating func retry(_ asset: String) {
    failed.remove(asset)
    skipped.remove(asset)
  }
}
// END AUDIO FAILURE POLICY

@MainActor final class Game: ObservableObject {
  @Published var snapshot: Snapshot?
  @Published var error: String? {
    didSet {
      if error != nil { revealInterface() }
      reconcileMediaPlayback()
    }
  }
  @Published var importing = false {
    didSet {
      if importing { revealInterface() }
      reconcileMediaPlayback()
    }
  }
  @Published var hasSave = false
  @Published var slots: [SaveSlot] = []
  @Published private(set) var savePreviewGeneration = UUID()
  private let saveThumbnails = GameGalleryThumbnails()
  private var savePreviewsVisible = false
  @Published var timeline: [PastPage] = []
  @Published var fastForward = false
  @Published private(set) var interfaceHidden = false
  @Published var modalOpen = false {
    didSet {
      if modalOpen { revealInterface() }
      reconcileMediaPlayback()
    }
  }
  @Published private(set) var mediaPolicy = MediaLifecyclePolicy()
  var mediaPlaybackAllowed: Bool { mediaPolicy.permitsPlayback && active }
  var canResumeMedia: Bool { mediaPolicy.canRequestUserResume }
  var mediaPauseMessage: String {
    switch mediaPolicy.resumeReason {
    case .routeLost: return "音频设备已断开，音视频已暂停。"
    case .activationFailed: return "音频未能恢复，音视频保持暂停。"
    default: return "系统中断了播放，音视频保持暂停。"
    }
  }
  @Published var gallery: [CollectionEntry] = []
  @Published var replays: [CollectionEntry] = []
  @Published private(set) var collectionGeneration = UUID()
  private let galleryThumbnails = GameGalleryThumbnails()
  @Published var videoURL: URL?
  @Published var videoIdentity = UUID()
  private var exportedVideo: VideoClip?
  private let videoExportQueue: OperationQueue = {
    let queue = OperationQueue()
    queue.maxConcurrentOperationCount = 1
    queue.qualityOfService = .userInitiated
    return queue
  }()
  private var lastFrame: CFTimeInterval?
  private var clockRemainder = 0.0
  private var refreshPending = false
  private var audioFailures = AudioFailurePolicy()
  private var failedAudioClip: (clip: Clip, channel: String)?
  @Published private(set) var audioOutputError: String?
  @Published private(set) var originalUI = OriginalUIArtwork()
  private(set) var creditImages: [String: Image] = [:]
  private(set) var messageGlyph: MessageGlyphState?
  private(set) var messageGlyphSheet: Image?
  var hasAudioFailure: Bool { failedAudioClip != nil }
  private var pendingVideoCompletion: UInt64?
  var choices: [StoryChoice] { snapshot?.flow?.choices ?? [] }
  var canAdvance: Bool {
    guard active, !interfaceHidden, !modalOpen, !importing, error == nil, let state = snapshot
    else {
      return false
    }
    return !state.title && !state.finished && state.blocked == nil && choices.isEmpty
      && state.scene.video == nil
      && (state.flow?.acceptsAdvanceWait ?? true)
  }
  var canChoose: Bool {
    guard active, !interfaceHidden, !modalOpen, !importing, error == nil, let state = snapshot
    else {
      return false
    }
    return !state.title && !state.finished && state.blocked == nil && state.scene.video == nil
      && (state.flow?.waitMs ?? 0) == 0
  }
  var canHideInterface: Bool {
    ReadingEligibility(
      active: active, hasScene: snapshot != nil, title: snapshot?.title ?? true,
      finished: snapshot?.finished ?? false, blocked: snapshot?.blocked != nil,
      messageVisible: snapshot?.scene.messageVisible ?? false, hasChoices: !choices.isEmpty,
      hasVideo: snapshot?.scene.video != nil, hasCredits: snapshot?.scene.credits != nil,
      hasInterlude: snapshot?.interlude != nil, waiting: (snapshot?.flow?.waitMs ?? 0) != 0,
      modalOpen: modalOpen, importing: importing, hasError: error != nil
    ).canHide
  }
  func hideInterface() {
    stopFastForward()
    guard canHideInterface else { return }
    interfaceHidden = true
    lastFrame = nil
    clockRemainder = 0
  }
  func revealInterface() {
    guard interfaceHidden else { return }
    interfaceHidden = false
    lastFrame = nil
    clockRemainder = 0
  }
  func stopReadingMode() {
    stopFastForward()
    revealInterface()
  }
  private var fastTask: Task<Void, Never>?
  @Published var notice: String?
  @Published var saveWarning: String?
  @Published var importProgress = 0.0
  @Published var importDetail = "正在检查资源…"
  @Published var resourceStatus = "尚未找到 data.xp3，请先放入资源文件。"
  @Published var bgmVolume = Game.preference("bgmVolume", fallback: 0.35) {
    didSet {
      UserDefaults.standard.set(bgmVolume, forKey: "bgmVolume")
      applyVolume()
    }
  }
  @Published var voiceVolume = Game.preference("voiceVolume", fallback: 1) {
    didSet {
      UserDefaults.standard.set(voiceVolume, forKey: "voiceVolume")
      applyVolume()
    }
  }
  @Published var seVolume = Game.preference("seVolume", fallback: 0.7) {
    didSet {
      UserDefaults.standard.set(seVolume, forKey: "seVolume")
      applyVolume()
    }
  }
  @Published var textScale = Game.preference("textScale", fallback: 1, range: 0.8...1.6) {
    didSet { UserDefaults.standard.set(textScale, forKey: "textScale") }
  }
  @Published var dialogueOpacity = Game.preference(
    "dialogueOpacity", fallback: 0.9, range: 0.65...1)
  {
    didSet { UserDefaults.standard.set(dialogueOpacity, forKey: "dialogueOpacity") }
  }
  @Published var dialogueLineSpacing = Game.preference(
    "dialogueLineSpacing", fallback: 5, range: 2...12)
  {
    didSet { UserDefaults.standard.set(dialogueLineSpacing, forKey: "dialogueLineSpacing") }
  }
  @Published var showPageNumber = UserDefaults.standard.bool(forKey: "showPageNumber") {
    didSet { UserDefaults.standard.set(showPageNumber, forKey: "showPageNumber") }
  }
  @Published var muted = UserDefaults.standard.bool(forKey: "muted")
  var handle: OpaquePointer?
  private var players: [String: AVAudioPlayer] = [:]
  private var sequences: [String: UInt64] = [:]
  private var clips: [String: Clip] = [:]
  private var gains: [String: Float] = [:]
  private var fadeTasks: [String: Task<Void, Never>] = [:]
  private var cache: [String: Data] = [:]
  private var cacheOrder: [String] = []
  private var renderFailed = false
  private var requests: [String: UUID] = [:]
  private var audioQueues: [String: OperationQueue] = [:]
  private var active = true
  private var pausedChannels: Set<String> = []
  private var importJob: AssetImport?
  private var noticeTask: Task<Void, Never>?
  #if os(macOS)
    private var escapeMonitor: Any?
  #endif
  private let support = FileManager.default.urls(
    for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent(
      "KoikariReborn")
  private let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]

  nonisolated private static func preference(
    _ key: String, fallback: Double, range: ClosedRange<Double> = 0...1
  ) -> Double {
    guard let number = UserDefaults.standard.object(forKey: key) as? NSNumber,
      number.doubleValue.isFinite
    else { return fallback }
    return min(range.upperBound, max(range.lowerBound, number.doubleValue))
  }

  func inform(_ text: String) {
    noticeTask?.cancel()
    notice = text
    noticeTask = Task { [weak self] in
      try? await Task.sleep(nanoseconds: 3_000_000_000)
      if !Task.isCancelled { self?.notice = nil }
    }
  }

  init() {
    #if os(macOS)
      mediaPolicy.setApplicationActive(NSApplication.shared.isActive)
      escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
        let handled = MainActor.assumeIsolated {
          guard event.keyCode == 53, let self, self.fastForward || self.interfaceHidden else {
            return false
          }
          self.stopReadingMode()
          return true
        }
        return handled ? nil : event
      }
    #endif
    #if os(iOS)
      AssetImport.cleanAbandoned(in: support)
      do {
        try AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        if !muted { try AVAudioSession.sharedInstance().setActive(true) }
      } catch { failAudioOutput("音频会话未能启动：\(error.localizedDescription)") }
      do {
        let folder = try AssetLocation.prepareDocumentResources(in: documents)
        if FileManager.default.fileExists(atPath: folder.appendingPathComponent("data.xp3").path) {
          reloadDocumentResources()
          return
        }
      } catch { self.error = "无法创建资源文件夹：\(error.localizedDescription)" }
    #endif
    let args = CommandLine.arguments
    if let index = args.firstIndex(of: "--assets"), index + 1 < args.count {
      connect(URL(fileURLWithPath: args[index + 1]))
    } else if let path = UserDefaults.standard.string(forKey: "assets") {
      #if os(iOS)
        if path == AssetLocation.documentIdentifier {
          resourceStatus = "GameResources 中未找到 data.xp3，请先放入资源文件。"
        } else if let directory = AssetLocation.resolve(
          path, relativeTo: support, documents: documents)
        {
          connect(directory)
        } else {
          error = "无法恢复旧资源位置。原存档仍保留，请将 XP3 放入本应用文稿的 GameResources 文件夹。"
        }
      #else
        connect(URL(fileURLWithPath: path))
      #endif
    }
  }
  deinit {
    #if os(macOS)
      if let escapeMonitor { NSEvent.removeMonitor(escapeMonitor) }
    #endif
  }
  nonisolated private func rustError() -> String {
    guard let text = kr_error() else { return "Rust 核心返回未知错误" }
    defer { kr_string_free(text) }
    return String(cString: text)
  }
  @discardableResult func connect(_ directory: URL) -> Bool {
    #if os(iOS)
      guard
        let location = AssetLocation.identifier(
          for: directory, relativeTo: support, documents: documents)
      else {
        error = "请将 XP3 放入本应用文稿的 GameResources 文件夹。"
        return false
      }
    #else
      let location = directory.path
    #endif
    let saves = support.appendingPathComponent("Saves")
    let next = directory.path.withCString { path in saves.path.withCString { kr_open(path, $0) } }
    guard let next else {
      error = "资源载入失败：\(rustError())\n请选择包含 data.xp3（及可选 patch.xp3）的文件夹。"
      return false
    }
    stopFastForward()
    clearGalleryThumbnails()
    clearSaveThumbnails()
    slots.removeAll()
    gallery.removeAll()
    replays.removeAll()
    if let old = handle { kr_close(old) }
    stopAudio()
    cache.removeAll()
    cacheOrder.removeAll()
    creditImages.removeAll()
    messageGlyph = nil
    messageGlyphSheet = nil
    audioFailures = AudioFailurePolicy()
    failedAudioClip = nil
    handle = next
    // Main-thread-only handle use; replace the entire decoration set on reconnect.
    // Artwork failure is nonfatal and visibly falls back to native controls.
    originalUI = OriginalUIArtwork.load { name in
      var count = 0
      guard let bytes = name.withCString({ kr_ui_asset(next, $0, &count) }) else { return nil }
      defer { kr_bytes_free(bytes, count) }
      guard count > 0, count <= OriginalUIArtwork.byteLimit else { return nil }
      return Data(bytes: bytes, count: count)
    }
    lastFrame = nil
    exportedVideo = nil
    videoURL = nil
    pendingVideoCompletion = nil
    renderFailed = false
    error = nil
    saveWarning = nil
    UserDefaults.standard.set(location, forKey: "assets")
    refresh(forceAudio: true)
    reloadSaves()
    #if os(iOS)
      resourceStatus =
        location == AssetLocation.documentIdentifier
        ? "已直接载入 GameResources 中的资源，不创建额外副本。"
        : "当前使用以前导入的资源；原资源和存档已保留。"
    #else
      resourceStatus = "已载入所选文件夹中的资源；直接只读访问，不创建额外副本。"
    #endif
    return true
  }
  #if os(iOS)
    func reloadDocumentResources() {
      guard !importing, snapshot?.title != false else { return }
      do {
        let folder = try AssetLocation.prepareDocumentResources(in: documents)
        guard
          FileManager.default.fileExists(
            atPath: folder.appendingPathComponent("data.xp3").path)
        else {
          resourceStatus = "GameResources 中未找到 data.xp3。请等待文件移动完成后重新检查。"
          return
        }
        resourceStatus = "正在检查并载入资源…"
        if !connect(folder) { resourceStatus = "资源载入失败，请查看提示；原资源与存档未删除。" }
      } catch {
        resourceStatus = "无法访问资源文件夹。"
        self.error = "资源检查失败：\(error.localizedDescription)"
      }
    }
  #endif
  func importDirectory(_ url: URL) {
    guard !importing else { return }
    #if os(macOS)
      let scoped = url.startAccessingSecurityScopedResource()
      defer { if scoped { url.stopAccessingSecurityScopedResource() } }
      if connect(url) { inform("资源已载入，可开始或继续游戏") }
    #else
      importing = true
      importProgress = 0
      importDetail = "正在检查 XP3 和可用空间…"
      let job = AssetImport()
      importJob = job
      let scoped = url.startAccessingSecurityScopedResource()
      let support = self.support
      let target = support.appendingPathComponent("Assets-\(UUID().uuidString)")
      let staging = support.appendingPathComponent("Import-\(UUID().uuidString)")
      DispatchQueue.global(qos: .userInitiated).async {
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
          try job.copy(from: url, to: staging) { copied, total in
            DispatchQueue.main.async {
              guard self.importJob === job else { return }
              self.importProgress = total > 0 ? Double(copied) / Double(total) : 0
              self.importDetail =
                "\(ByteCountFormatter.string(fromByteCount: copied, countStyle: .file)) / \(ByteCountFormatter.string(fromByteCount: total, countStyle: .file))"
            }
          }
          try FileManager.default.moveItem(at: staging, to: target)
          DispatchQueue.main.async {
            guard !job.isCancelled else {
              try? FileManager.default.removeItem(at: target)
              self.importing = false
              self.importJob = nil
              self.inform("已取消导入，原资源未更改")
              return
            }
            let previous = UserDefaults.standard.string(forKey: "assets").flatMap {
              AssetLocation.resolve($0, relativeTo: support)
            }
            if !self.connect(target) {
              try? FileManager.default.removeItem(at: target)
            } else if let previous, previous.path != target.path {
              try? FileManager.default.removeItem(at: previous)
            }
            self.importing = false
            self.importJob = nil
            if self.error == nil { self.inform("资源已导入，可开始或继续游戏") }
          }
        } catch {
          try? FileManager.default.removeItem(at: staging)
          try? FileManager.default.removeItem(at: target)
          DispatchQueue.main.async {
            self.importing = false
            self.importJob = nil
            if job.isCancelled {
              self.inform("已取消导入，原资源未更改")
            } else {
              self.error = "XP3 导入失败：\(error.localizedDescription)"
            }
          }
        }
      }
    #endif
  }
  func cancelImport() {
    importJob?.cancel()
    importDetail = "正在取消导入…"
  }
  func reloadSaves() {
    clearSaveThumbnails()
    guard let handle else { return }
    guard let json = kr_saves(handle) else {
      error = "存档列表读取失败：\(rustError())"
      return
    }
    defer { kr_string_free(json) }
    do {
      slots = try JSONDecoder().decode([SaveSlot].self, from: Data(String(cString: json).utf8))
      hasSave = slots.contains { $0.compatible }
    } catch { self.error = "存档列表读取失败：\(error.localizedDescription)" }
  }
  // Prepare an immutable lease from the exact saved file, not from the live scene.
  // GPU work and PNG decoding stay on the cancellable serial thumbnail worker.
  func saveThumbnail(_ slot: SaveSlot) async -> Image? {
    guard slot.exists, slot.compatible, let revision = slot.previewRevision,
      let number = UInt32(exactly: slot.slot),
      slots.contains(where: {
        $0.slot == slot.slot && $0.compatible && $0.previewRevision == revision
      })
    else { return nil }
    let generation = savePreviewGeneration
    let image = await saveThumbnails.image(
      for: "\(slot.slot):\(revision)",
      prepare: { [weak self] in
        guard let self, self.savePreviewGeneration == generation, let handle = self.handle,
          self.slots.contains(where: {
            $0.slot == slot.slot && $0.compatible && $0.previewRevision == revision
          })
        else { return nil }
        return SaveThumbnailReader.prepare(handle: handle, slot: number, revision: revision)
      })
    guard !Task.isCancelled, savePreviewGeneration == generation else { return nil }
    return image
  }
  func beginSavePreviews() { savePreviewsVisible = true }
  func endSavePreviews() {
    savePreviewsVisible = false
    clearSaveThumbnails()
  }
  func clearSaveThumbnails() {
    saveThumbnails.clear()
    savePreviewGeneration = UUID()
  }
  @discardableResult func autoSave() -> Bool {
    guard let handle, let state = snapshot, !state.title, state.blocked == nil else { return false }
    if "save:9".withCString({ kr_action(handle, $0) }) == 0 {
      saveWarning = "自动进度保存失败：\(rustError())"
      return false
    } else {
      saveWarning = nil
      hasSave = true
      // Audio interruptions can autosave while this sheet stays visible/active.
      // Refresh its exact-byte revision, but never reread all slots per ordinary page.
      if savePreviewsVisible { reloadSaves() } else { clearSaveThumbnails() }
      return true
    }
  }
  @discardableResult func action(_ action: String) -> Bool {
    guard let handle, !importing else { return false }
    if action != "advance" { stopFastForward() }
    if action == "advance", !canAdvance { return false }
    if action.hasPrefix("choose:"), !canChoose { return false }
    if action == "title" || action == "new" || action == "continue" || action == "load"
      || action.hasPrefix("load:") || action == "back" || action.hasPrefix("rewind:")
      || action.hasPrefix("replay:")
    {
      revealInterface()
    }
    let ok = action.withCString { kr_action(handle, $0) } != 0
    if action == "title" || action == "new" { renderFailed = false }
    if !ok {
      error = rustError()
      stopFastForward()
    }
    let loading = action == "continue" || action == "load" || action.hasPrefix("load:")
    let rewinding = action == "back" || action.hasPrefix("rewind:")
    if loading || rewinding { renderFailed = false }
    refresh(
      forceAudio: loading || rewinding || action == "new" || action == "title"
        || action.hasPrefix("replay:"))
    lastFrame = nil
    if ok && !fastForward
      && (loading || rewinding || action == "new" || action == "advance"
        || action.hasPrefix("choose:") || action.hasPrefix("video-complete:")
        || action.hasPrefix("replay:"))
    {
      autoSave()
    }
    if action != "advance" { reloadSaves() }
    if ok && action.hasPrefix("save:") { inform("已保存到手动槽位 \(action.dropFirst(5))") }
    if ok && loading { inform("已恢复第 \(snapshot?.pages ?? 0) 页") }
    return ok
  }
  func next() {
    switch ReadingInputPolicy.nextIntent(
      interfaceHidden: interfaceHidden, fastForward: fastForward,
      blocked: modalOpen || importing || error != nil || !active, canAdvance: canAdvance)
    {
    case .none: return
    case .reveal: stopReadingMode()
    case .stopFastForward: stopFastForward()
    case .advance: action("advance")
    }
  }
  func toggleFastForward() {
    if fastForward {
      stopFastForward()
      return
    }
    guard canAdvance else { return }
    fastForward = true
    stopAudio()
    fastTask = Task { [weak self] in
      while !Task.isCancelled {
        try? await Task.sleep(nanoseconds: 250_000_000)
        guard let self, !Task.isCancelled, self.fastForward else { return }
        let ok = self.action("advance")
        if !ok || self.error != nil || self.snapshot?.finished == true
          || self.snapshot?.blocked != nil
          || self.snapshot?.interlude != nil
          || !self.choices.isEmpty || self.snapshot?.scene.video != nil
          || self.snapshot?.flow?.acceptsAdvanceWait == false
        {
          self.stopFastForward()
          return
        }
      }
    }
  }
  func stopFastForward() {
    guard fastForward else { return }
    fastForward = false
    fastTask?.cancel()
    fastTask = nil
    autoSave()
    refresh(forceAudio: true)
  }
  private func refresh(forceAudio: Bool) {
    guard let handle, let json = kr_snapshot(handle) else {
      error = rustError()
      return
    }
    defer { kr_string_free(json) }
    do {
      let data = Data(String(cString: json).utf8)
      snapshot = try JSONDecoder().decode(Snapshot.self, from: data)
      if !canHideInterface { revealInterface() }
      if messageGlyph == nil, snapshot?.title == false {
        guard let json = kr_message_glyph(handle) else {
          error = "阅读标记加载失败：\(rustError())"
          return
        }
        defer { kr_string_free(json) }
        let glyph = try JSONDecoder().decode(
          MessageGlyphState.self, from: Data(String(cString: json).utf8))
        var count = 0
        guard let bytes = glyph.asset.withCString({ kr_asset(handle, $0, &count) }) else {
          error = "阅读标记图像读取失败：\(rustError())"
          return
        }
        let imageData = Data(bytes: bytes, count: count)
        kr_bytes_free(bytes, count)
        #if os(macOS)
          guard let image = NSImage(data: imageData) else {
            error = "阅读标记图像解码失败"
            return
          }
          messageGlyphSheet = Image(nsImage: image)
        #else
          guard let image = UIImage(data: imageData) else {
            error = "阅读标记图像解码失败"
            return
          }
          messageGlyphSheet = Image(uiImage: image)
        #endif
        messageGlyph = glyph
      }
      if let credits = snapshot?.scene.credits {
        for asset in credits.images where creditImages[asset] == nil {
          var count = 0
          guard let bytes = asset.withCString({ kr_asset(handle, $0, &count) }) else {
            error = "制作人员名单图像读取失败：\(rustError())"
            return
          }
          let imageData = Data(bytes: bytes, count: count)
          kr_bytes_free(bytes, count)
          #if os(macOS)
            guard let image = NSImage(data: imageData) else {
              error = "制作人员名单图像解码失败"
              return
            }
            creditImages[asset] = Image(nsImage: image)
          #else
            guard let image = UIImage(data: imageData) else {
              error = "制作人员名单图像解码失败"
              return
            }
            creditImages[asset] = Image(uiImage: image)
          #endif
        }
      } else {
        creditImages.removeAll()
      }
      if let json = kr_timeline(handle) {
        defer { kr_string_free(json) }
        timeline = try JSONDecoder().decode([PastPage].self, from: Data(String(cString: json).utf8))
      }
      updateCurrentAudio(force: forceAudio)
      if forceAudio {
        exportedVideo = nil
        pendingVideoCompletion = nil
        videoIdentity = UUID()
      }
      if let video = snapshot?.scene.video {
        if exportedVideo != video {
          exportedVideo = video
          videoURL = nil
          videoIdentity = UUID()
          let identity = videoIdentity
          guard let lease = kr_video_request(handle) else {
            error = "视频读取失败：\(rustError())"
            return
          }
          let leaseAddress = UInt(bitPattern: lease)
          videoExportQueue.addOperation { [self] in
            let path = kr_video_export(OpaquePointer(bitPattern: leaseAddress))
            let url = path.map { URL(fileURLWithPath: String(cString: $0)) }
            let failure = url == nil ? rustError() : nil
            if let path { kr_string_free(path) }
            DispatchQueue.main.async { [self] in
              guard self.videoIdentity == identity,
                self.snapshot?.scene.video == video
              else { return }
              if let url { self.videoURL = url } else { self.error = "视频导出失败：\(failure ?? "未知错误")" }
            }
          }
        }
      } else {
        if exportedVideo != nil { videoIdentity = UUID() }
        exportedVideo = nil
        videoURL = nil
        pendingVideoCompletion = nil
      }
    } catch { self.error = error.localizedDescription }
  }
  func choose(_ choice: StoryChoice) {
    guard choice.enabled, canChoose else { return }
    action("choose:\(choice.id)")
  }
  func videoCompleted(_ sequence: UInt64) {
    guard snapshot?.scene.video?.sequence == sequence else { return }
    guard mediaPlaybackAllowed, !modalOpen, !importing, error == nil else {
      pendingVideoCompletion = sequence
      return
    }
    pendingVideoCompletion = nil
    action("video-complete:\(sequence)")
  }
  func resumePendingVideo() {
    lastFrame = nil
    guard mediaPlaybackAllowed else { return }
    if let sequence = pendingVideoCompletion { videoCompleted(sequence) }
  }
  func retryVideo() {
    error = nil
    exportedVideo = nil
    videoIdentity = UUID()
    pendingVideoCompletion = nil
    refresh(forceAudio: false)
  }
  @discardableResult func reloadCollection(replay: Bool) -> Bool {
    stopFastForward()
    guard let handle else { return false }
    guard let json = replay ? kr_replays(handle) : kr_gallery(handle) else {
      error = rustError()
      return false
    }
    defer { kr_string_free(json) }
    do {
      let entries = try JSONDecoder().decode(
        [CollectionEntry].self, from: Data(String(cString: json).utf8))
      clearGalleryThumbnails()
      if replay { replays = entries } else { gallery = entries }
      return true
    } catch {
      self.error = "鉴赏列表读取失败：\(error.localizedDescription)"
      return false
    }
  }
  // Reads owned bytes on the main actor; the bounded thumbnail worker decodes off-thread.
  // Neither thumbnails nor their failures invoke a story action or change the CG preview.
  func galleryThumbnail(_ entry: CollectionEntry) async -> Image? {
    guard entry.unlocked, gallery.contains(where: { $0.id == entry.id && $0.unlocked }) else {
      return nil
    }
    let generation = collectionGeneration
    let image = await galleryThumbnails.image(for: entry.id) { [weak self] in
      guard let self, self.collectionGeneration == generation, let handle = self.handle,
        self.gallery.contains(where: { $0.id == entry.id && $0.unlocked })
      else { return nil }
      var count = 0
      guard let bytes = entry.id.withCString({ kr_gallery_image(handle, $0, &count) }) else {
        return nil
      }
      defer { kr_bytes_free(bytes, count) }
      guard count > 0, count <= 32 * 1024 * 1024 else { return nil }
      return Data(bytes: bytes, count: count)
    }
    guard !Task.isCancelled, collectionGeneration == generation else { return nil }
    return image
  }
  func clearGalleryThumbnails() {
    galleryThumbnails.clear()
    collectionGeneration = UUID()
  }
  func showCG(_ entry: CollectionEntry) -> Bool {
    guard entry.unlocked, let handle else { return false }
    if entry.id.withCString({ kr_gallery_show(handle, $0) }) == 0 {
      error = rustError()
      return false
    }
    renderFailed = false
    return true
  }
  func hideCG() {
    guard let handle else { return }
    if kr_gallery_hide(handle) == 0 { error = rustError() } else { renderFailed = false }
  }
  private func updateCurrentAudio(force: Bool = false) {
    guard !fastForward, let scene = snapshot?.scene else { return }
    updateAudio(scene.bgm, channel: "bgm", force: force)
    updateAudio(scene.voice, channel: "voice", force: force)
    updateAudio(scene.se, channel: "se", force: force)
  }
  private func silenceChannel(_ channel: String) {
    requests.removeValue(forKey: channel)
    audioQueues[channel]?.cancelAllOperations()
    fadeTasks[channel]?.cancel()
    players.removeValue(forKey: channel)?.stop()
    pausedChannels.remove(channel)
  }
  private func wantsAudio(_ channel: String) -> Bool {
    AudioPlaybackGate.allows(
      muted: muted, volume: volume(channel), outputFailed: audioOutputError != nil)
  }
  private func updateAudio(_ clip: Clip, channel: String, force: Bool) {
    guard wantsAudio(channel) else {
      silenceChannel(channel)
      clips[channel] = clip
      sequences[channel] = clip.sequence
      return
    }
    guard audioFailures.mayDecode(clip.asset) else {
      requests[channel] = UUID()
      audioQueues[channel]?.cancelAllOperations()
      fadeTasks[channel]?.cancel()
      players[channel]?.stop()
      players[channel] = nil
      pausedChannels.remove(channel)
      clips[channel] = clip
      sequences[channel] = clip.sequence
      if audioFailures.needsAcknowledgement(clip.asset) {
        failedAudioClip = (clip, channel)
        error = "此音频片段校验或解码失败，请跳过此片段或明确重试。"
      }
      return
    }
    if !force && clips[channel] == clip
      && (players[channel] != nil || requests[channel] != nil || clip.asset.isEmpty)
    {
      return
    }
    let previous = clips[channel]
    clips[channel] = clip
    if !force, previous?.asset == clip.asset, previous?.sequence == clip.sequence,
      let player = players[channel], !clip.asset.isEmpty
    {
      player.numberOfLoops = clip.looping ? -1 : 0
      fade(channel: channel, to: clip.volume ?? 1, milliseconds: clip.fadeMs ?? 0)
      return
    }
    sequences[channel] = clip.sequence
    let request = UUID()
    requests[channel] = request
    audioQueues[channel]?.cancelAllOperations()
    if clip.asset.isEmpty {
      fade(channel: channel, to: 0, milliseconds: clip.fadeMs ?? 0, stop: true)
      return
    }
    fadeTasks[channel]?.cancel()
    players[channel]?.stop()
    players[channel] = nil
    pausedChannels.remove(channel)
    guard !clip.asset.isEmpty, let handle else { return }
    if let cached = cache[clip.asset] {
      play(cached, clip: clip, channel: channel)
      return
    }
    var count = 0
    guard let bytes = clip.asset.withCString({ kr_asset(handle, $0, &count) }) else {
      failAudioClip(clip, channel: channel, message: "音频读取失败：\(rustError())")
      return
    }
    let compressed = Data(bytes: bytes, count: count)
    kr_bytes_free(bytes, count)
    let queue: OperationQueue
    if let existing = audioQueues[channel] {
      queue = existing
    } else {
      queue = OperationQueue()
      queue.maxConcurrentOperationCount = 1
      queue.qualityOfService = .userInitiated
      audioQueues[channel] = queue
    }
    queue.addOperation { [self] in
      var size = 0
      let wave = compressed.withUnsafeBytes { buffer in
        kr_decode_audio(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &size)
      }
      let decoded = wave.map { Data(bytes: $0, count: size) }
      let failure = decoded == nil ? rustError() : nil
      if let wave { kr_bytes_free(wave, size) }
      DispatchQueue.main.async { [self] in
        guard self.requests[channel] == request else { return }
        guard let decoded else {
          self.failAudioClip(
            clip, channel: channel, message: "音频解码失败：\(failure ?? "未知错误")")
          return
        }
        self.cache[clip.asset] = decoded
        self.cacheOrder.append(clip.asset)
        while self.cacheOrder.count > 12
          || self.cache.values.reduce(0, { $0 + $1.count }) > 48 * 1024 * 1024
        {
          guard !self.cacheOrder.isEmpty else { break }
          self.cache.removeValue(forKey: self.cacheOrder.removeFirst())
        }
        self.play(decoded, clip: self.clips[channel] ?? clip, channel: channel)
      }
    }
  }
  private func play(_ data: Data, clip: Clip, channel: String) {
    guard wantsAudio(channel) else {
      silenceChannel(channel)
      return
    }
    do {
      let player = try AVAudioPlayer(data: data)
      player.numberOfLoops = clip.looping ? -1 : 0
      gains[channel] = (clip.fadeMs ?? 0) > 0 ? 0 : (clip.volume ?? 1)
      player.volume = volume(channel) * (gains[channel] ?? 1)
      player.prepareToPlay()
      players[channel] = player
      if active {
        if !player.play() {
          failAudioOutput("音频输出未能启动，请检查输出设备，或选择静音继续。")
          return
        }
      } else {
        pausedChannels.insert(channel)
      }
      fade(channel: channel, to: clip.volume ?? 1, milliseconds: clip.fadeMs ?? 0)
    } catch {
      failAudioClip(clip, channel: channel, message: "音频片段播放失败：\(error.localizedDescription)")
    }
  }
  private func fade(channel: String, to target: Float, milliseconds: UInt64, stop: Bool = false) {
    fadeTasks[channel]?.cancel()
    let target = min(1, max(0, target.isFinite ? target : 1))
    guard let player = players[channel] else {
      gains[channel] = target
      return
    }
    let start = gains[channel] ?? 1
    if milliseconds == 0 {
      gains[channel] = target
      player.volume = volume(channel) * target
      if stop {
        player.stop()
        players.removeValue(forKey: channel)
        pausedChannels.remove(channel)
      }
      return
    }
    fadeTasks[channel] = Task { [weak self, weak player] in
      var elapsed = 0.0
      var previous = CACurrentMediaTime()
      while !Task.isCancelled {
        try? await Task.sleep(nanoseconds: 20_000_000)
        guard !Task.isCancelled, let self, let player, self.players[channel] === player else {
          return
        }
        let now = CACurrentMediaTime()
        if self.active { elapsed += max(0, now - previous) * 1000 }
        previous = now
        let fraction = Float(min(1, elapsed / Double(milliseconds)))
        self.gains[channel] = start + (target - start) * fraction
        player.volume = self.volume(channel) * (self.gains[channel] ?? target)
        if fraction >= 1 {
          if stop {
            player.stop()
            self.players.removeValue(forKey: channel)
            self.pausedChannels.remove(channel)
          }
          return
        }
      }
    }
  }
  private func volume(_ channel: String) -> Float {
    muted ? 0 : Float(channel == "bgm" ? bgmVolume : (channel == "se" ? seVolume : voiceVolume))
  }
  private func applyVolume() {
    for (channel, player) in players { player.volume = volume(channel) * (gains[channel] ?? 1) }
    updateCurrentAudio()
  }
  func toggleMute() {
    muted.toggle()
    UserDefaults.standard.set(muted, forKey: "muted")
    if muted { stopAudio() }
    clearAudioOutputFailure()
    applyVolume()
  }
  private func failAudioClip(_ clip: Clip, channel: String, message: String) {
    silenceChannel(channel)
    audioFailures.record(clip.asset)
    failedAudioClip = (clip, channel)
    error = message
    stopFastForward()
  }
  private func failAudioOutput(_ message: String) {
    guard audioOutputError == nil else { return }
    audioOutputError = message
    stopAudio()
    error = message
    stopFastForward()
  }
  private func clearAudioOutputFailure() {
    guard let message = audioOutputError else { return }
    audioOutputError = nil
    if error == message { error = nil }
    reconcileMediaPlayback()
  }
  func showAudioOutputRecovery() {
    if error == nil { error = audioOutputError }
  }
  func continueMuted() {
    if !muted {
      toggleMute()
    } else {
      clearAudioOutputFailure()
      stopAudio()
      applyVolume()
    }
  }
  func retryAudioOutput() {
    guard audioOutputError != nil else { return }
    clearAudioOutputFailure()
    updateCurrentAudio(force: true)
  }
  func replayVoice() {
    stopFastForward()
    guard let clip = snapshot?.scene.voice, !clip.asset.isEmpty else { return }
    audioFailures.retry(clip.asset)
    updateAudio(clip, channel: "voice", force: true)
  }
  func skipFailedAudio() {
    if let failedAudioClip { audioFailures.skip(failedAudioClip.clip.asset) }
    failedAudioClip = nil
    error = nil
  }
  func retryFailedAudio() {
    guard let failed = failedAudioClip else { return }
    audioFailures.retry(failed.clip.asset)
    failedAudioClip = nil
    error = nil
    updateAudio(failed.clip, channel: failed.channel, force: true)
  }
  func sceneActivityChanged(_ foreground: Bool) {
    mediaPolicy.setForeground(foreground)
    reconcileMediaPlayback()
    if !foreground {
      stopFastForward()
      autoSave()
    }
    #if os(iOS)
      if foreground, handle == nil, error == nil { reloadDocumentResources() }
    #endif
  }
  func applicationActivityChanged(_ applicationActive: Bool) {
    mediaPolicy.setApplicationActive(applicationActive)
    reconcileMediaPlayback()
    if !applicationActive {
      stopFastForward()
      autoSave()
    }
  }
  func interruptionBegan() {
    mediaPolicy.beginInterruption()
    reconcileMediaPlayback()
    stopFastForward()
    autoSave()
  }
  func interruptionEnded(shouldResume: Bool) {
    mediaPolicy.endInterruption(shouldResume: shouldResume)
    reconcileMediaPlayback()
    if !mediaPlaybackAllowed { stopFastForward() }
  }
  func outputRouteLost() {
    mediaPolicy.loseOutputRoute()
    reconcileMediaPlayback()
    stopFastForward()
    autoSave()
  }
  func resumeMediaByUser() {
    // Refresh blockers synchronously; a rejected click does not bank consent
    // for a later foreground/modal transition.
    mediaPolicy.setBlocked(modalOpen || importing || error != nil)
    guard mediaPolicy.requestUserResume() else { return }
    reconcileMediaPlayback()
  }
  private func reconcileMediaPlayback() {
    mediaPolicy.setBlocked(modalOpen || importing || error != nil)
    guard mediaPolicy.permitsPlayback && audioOutputError == nil else {
      suspendAudio(deactivateSession: !mediaPolicy.foreground && !mediaPolicy.interrupted)
      return
    }
    guard !active else { return }
    #if os(iOS)
      do {
        if !muted { try AVAudioSession.sharedInstance().setActive(true) }
      } catch {
        failAudioOutput("音频会话恢复失败：\(error.localizedDescription)")
        return
      }
    #endif
    active = true
    lastFrame = nil
    for channel in pausedChannels {
      if wantsAudio(channel), let player = players[channel], !player.play() {
        failAudioOutput("音频输出恢复失败，请检查输出设备，或选择静音继续。")
        return
      }
    }
    pausedChannels.removeAll()
    resumePendingVideo()
  }
  private func suspendAudio(deactivateSession: Bool = true) {
    revealInterface()
    guard active else { return }
    active = false
    lastFrame = nil
    for (channel, player) in players where player.isPlaying {
      pausedChannels.insert(channel)
      player.pause()
    }
    #if os(iOS)
      if deactivateSession {
        do {
          try AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        } catch { self.error = "音频会话暂停失败：\(error.localizedDescription)" }
      }
    #endif
  }
  func stopAudio() {
    for task in fadeTasks.values { task.cancel() }
    fadeTasks.removeAll()
    clips.removeAll()
    gains.removeAll()
    for player in players.values { player.stop() }
    players.removeAll()
    sequences.removeAll()
    requests.removeAll()
    pausedChannels.removeAll()
    for queue in audioQueues.values { queue.cancelAllOperations() }
  }
  func render(_ view: MTKView) {
    guard mediaPolicy.foreground, let handle, !renderFailed, let layer = view.layer as? CAMetalLayer
    else { return }
    let now = CACurrentMediaTime()
    defer { lastFrame = now }
    if ReadingInputPolicy.permitsStoryTick(
      mediaPlaybackAllowed: mediaPlaybackAllowed, interfaceHidden: interfaceHidden),
      let previous = lastFrame
    {
      let elapsed = min(1000, max(0, (now - previous) * 1000) + clockRemainder)
      let milliseconds = UInt64(elapsed)
      clockRemainder = elapsed - Double(milliseconds)
      let result = kr_tick(handle, milliseconds)
      if result == 0 {
        let message = rustError()
        DispatchQueue.main.async {
          self.error = "剧情时钟失败：\(message)"
          self.stopFastForward()
        }
      } else if result == 2, !refreshPending {
        refreshPending = true
        DispatchQueue.main.async {
          self.refreshPending = false
          self.refresh(forceAudio: false)
          if self.fastForward
            && (!self.choices.isEmpty || self.snapshot?.scene.video != nil
              || self.snapshot?.finished == true || self.snapshot?.blocked != nil
              || self.error != nil)
          {
            self.stopFastForward()
          }
        }
      }
    } else {
      clockRemainder = 0
    }
    if kr_draw(
      handle, Unmanaged.passUnretained(layer).toOpaque(), Float(view.drawableSize.width),
      Float(view.drawableSize.height)) == 0
    {
      renderFailed = true
      let message = rustError()
      DispatchQueue.main.async {
        self.error = "Metal 绘制失败：\(message)"
        self.stopFastForward()
      }
    }
  }
}

@MainActor final class Drawing: NSObject, MTKViewDelegate {
  var game: Game
  init(_ game: Game) { self.game = game }
  func draw(in view: MTKView) { game.render(view) }
  func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}
  func configure() -> MTKView {
    let view = MTKView(frame: .zero, device: MTLCreateSystemDefaultDevice())
    view.colorPixelFormat = .bgra8Unorm
    view.framebufferOnly = true
    view.preferredFramesPerSecond = 30
    view.delegate = self
    return view
  }
}
#if os(macOS)
  struct MetalCanvas: NSViewRepresentable {
    @ObservedObject var game: Game
    func makeCoordinator() -> Drawing { Drawing(game) }
    func makeNSView(context: Context) -> MTKView { context.coordinator.configure() }
    func updateNSView(_ view: MTKView, context: Context) { context.coordinator.game = game }
  }
#else
  struct MetalCanvas: UIViewRepresentable {
    @ObservedObject var game: Game
    func makeCoordinator() -> Drawing { Drawing(game) }
    func makeUIView(context: Context) -> MTKView { context.coordinator.configure() }
    func updateUIView(_ view: MTKView, context: Context) { context.coordinator.game = game }
  }
#endif

struct Content: View {
  @ObservedObject var game: Game
  @State private var importing = false
  @State private var showHistory = false
  @State private var showDiagnostics = false
  @State private var returnToTitle = false
  @State private var showSaves = false
  @State private var saving = false
  @State private var showSettings = false
  @State private var newGame = false
  @State private var showCollection = false
  @State private var collectionIsReplay = false
  @State private var gameplayMenu = GameplayMenuPresentation()
  @Environment(\.scenePhase) private var phase
  @ScaledMetric(relativeTo: .body) private var readingTypeSize: CGFloat = 18
  private var title: Bool { game.snapshot?.title ?? true }
  var body: some View {
    VStack(spacing: 0) {
      if !title {
        readingToolbar
          .opacity(game.interfaceHidden ? 0 : 1)
          .disabled(game.interfaceHidden)
          .allowsHitTesting(!game.interfaceHidden)
          .accessibilityHidden(game.interfaceHidden)
      }
      GeometryReader { geometry in
        let width = min(geometry.size.width, geometry.size.height * 16 / 9)
        let height = width * 9 / 16
        let top = (geometry.size.height - height) / 2
        ZStack {
          Color.black
          MetalCanvas(game: game)
            .frame(width: geometry.size.width, height: geometry.size.height).clipped()
            .contentShape(Rectangle()).onTapGesture {
              if !title && !game.importing { game.next() }
            }
          if title {
            GameTitleScreen(
              game: game,
              startNew: {
                if game.hasSave { newGame = true } else { game.action("new") }
              },
              resume: { game.action("continue") },
              openSaves: { openSaves(saving: false) },
              openCollection: { openCollection(replay: $0) },
              openResources: {
                #if os(macOS)
                  importing = true
                #else
                  game.reloadDocumentResources()
                #endif
              },
              openSettings: { showSettings = true }
            )
            .disabled(game.importing || game.modalOpen)
          } else if let state = game.snapshot {
            if let credits = state.scene.credits {
              CreditsView(credits: credits, images: game.creditImages)
                .frame(width: width, height: height)
                .position(x: geometry.size.width / 2, y: geometry.size.height / 2)
            }
            if state.scene.messageVisible && state.scene.video == nil && !state.finished
              && !game.interfaceHidden
            {
              let portraitWidth =
                state.sprites.values.contains { $0.position == "fc" } ? width * 0.20 : 0
              let panelHeight = ReadingLayout.panelHeight(
                sceneHeight: height, availableHeight: geometry.size.height,
                compact: width < 700, textScale: game.textScale,
                typeScale: readingTypeSize / 18)
              GameReadingPanel(
                speaker: state.scene.speaker, text: state.scene.text, page: state.pages,
                textScale: game.textScale, backgroundOpacity: game.dialogueOpacity,
                lineSpacing: game.dialogueLineSpacing, showPageNumber: game.showPageNumber
              ) {
                if let glyph = game.messageGlyph, let sheet = game.messageGlyphSheet {
                  MessageGlyphView(
                    glyph: glyph, sheet: sheet, elapsed: state.scene.visual.elapsedMs
                  ).accessibilityHidden(true)
                }
                Button {
                  game.action("back")
                } label: {
                  Label("上一句", systemImage: "chevron.left").labelStyle(.iconOnly)
                }
                .buttonStyle(.bordered).controlSize(.large).disabled(game.timeline.isEmpty)
                .keyboardShortcut(.leftArrow, modifiers: [])
                .accessibilityIdentifier("back").help("上一句（左方向键）")
                Button {
                  game.next()
                } label: {
                  Label(
                    game.fastForward ? "停止快进" : "下一句",
                    systemImage: game.fastForward ? "stop.fill" : "chevron.right"
                  )
                  .labelStyle(.iconOnly)
                }
                .buttonStyle(.bordered).controlSize(.large).disabled(
                  !game.fastForward && !game.canAdvance
                )
                .keyboardShortcut(.space, modifiers: [])
                .accessibilityIdentifier("advance").help("下一句（空格 / 右方向键）")
              }
              .frame(height: panelHeight)
              .padding(.leading, portraitWidth)
              .frame(width: width * 0.94)
              .position(
                x: geometry.size.width / 2,
                y: min(
                  geometry.size.height - panelHeight / 2 - 8,
                  max(panelHeight / 2 + 8, top + height - panelHeight / 2 - height * 0.02))
              )
              .disabled(game.modalOpen || game.importing || game.error != nil)
            }
            if game.interfaceHidden {
              VStack {
                Spacer()
                HStack {
                  Spacer()
                  Button {
                    game.next()
                  } label: {
                    Label("显示界面", systemImage: "eye")
                  }
                  .buttonStyle(.bordered).controlSize(.large)
                  .keyboardShortcut(.space, modifiers: [])
                  .accessibilityIdentifier("reveal-interface")
                  .accessibilityHint("只恢复界面，不推进剧情；也可点击画面恢复")
                  .help("点击画面、空格或右方向键只恢复界面，不翻页")
                }
              }.padding(16)
            }
            if state.interlude != nil && game.choices.isEmpty && state.scene.video == nil {
              VStack {
                Spacer()
                Button("人物介绍 · 点击继续剧情  ›") { game.next() }
                  .disabled(!game.canAdvance)
                  .keyboardShortcut(.space, modifiers: []).accessibilityIdentifier(
                    "continue-interlude"
                  )
                  .buttonStyle(.borderedProminent).controlSize(.large).padding(16)
              }
            }
            if !game.choices.isEmpty {
              GameTheme.text.opacity(0.18).contentShape(Rectangle()).onTapGesture {}
              VStack(spacing: 14) {
                Label("做出你的选择", systemImage: "sparkles")
                  .font(.system(.headline, design: .rounded)).foregroundStyle(GameTheme.accent)
                ScrollView {
                  VStack(spacing: 12) {
                    ForEach(game.choices) { choice in
                      Button {
                        game.choose(choice)
                      } label: {
                        Text(choice.text).font(.system(size: 17 * game.textScale))
                          .multilineTextAlignment(.center).frame(maxWidth: .infinity).padding(10)
                      }.buttonStyle(.borderedProminent).controlSize(.large)
                        .disabled(!choice.enabled || !game.canChoose)
                        .accessibilityIdentifier("choice-\(choice.id)")
                    }
                  }
                }
              }.padding(24).frame(width: min(620, width * 0.88), height: min(450, height * 0.85))
                .modifier(GamePanelStyle())
            }
            if let video = state.scene.video {
              let playbackIdentity = game.videoIdentity
              Color.black.contentShape(Rectangle()).onTapGesture {}
              if let url = game.videoURL {
                GameVideoView(
                  url: url, sequence: video.sequence, looping: video.looping,
                  skippable: video.skippable, muted: game.muted, volume: game.bgmVolume,
                  onCompleted: {
                    if playbackIdentity == game.videoIdentity { game.videoCompleted($0) }
                  },
                  onFailure: {
                    if playbackIdentity == game.videoIdentity {
                      game.stopFastForward()
                      game.error = "视频播放失败：\($0)"
                    }
                  }
                )
                .id(game.videoIdentity).frame(width: width, height: height)
                .environment(\.scenePhase, game.mediaPlaybackAllowed ? phase : .inactive)
                .allowsHitTesting(game.mediaPlaybackAllowed)
              } else {
                ProgressView("正在准备视频…").foregroundStyle(.white).tint(.white)
              }
            }
            if state.finished {
              GameTheme.background.opacity(0.85).contentShape(Rectangle()).onTapGesture {}
              VStack(spacing: 18) {
                if let ending = state.flow?.ending {
                  Text("故事结束").font(.system(.largeTitle, design: .rounded, weight: .bold))
                  Text(ending).font(.headline).multilineTextAlignment(.center)
                    .accessibilityIdentifier("ending-id")
                  if state.flow?.replay == true { Text("回想播放结束").foregroundStyle(.secondary) }
                  Button("CG 鉴赏") { openCollection(replay: false) }
                  Button("场景回想") { openCollection(replay: true) }
                } else {
                  Text("剧情已停止").font(.title)
                  Text("运行时未返回结局标识，请查看兼容状态。").font(.callout)
                }
                Button("返回标题") { returnToTitle = true }
              }.buttonStyle(.bordered).controlSize(.large).padding(30).modifier(GamePanelStyle())
            }
            if state.blocked != nil {
              Text("遇到未支持指令，已停止执行。请查看兼容状态。").font(.caption).padding()
                .foregroundStyle(.white).background(GameTheme.accent)
                .frame(maxHeight: .infinity, alignment: .top)
            }
          }
          if game.importing {
            GameTheme.background.opacity(0.96)
            VStack(spacing: 12) {
              Text("正在导入资源，请保持应用打开")
              ProgressView(value: game.importProgress)
              Text(game.importDetail).font(.caption).foregroundStyle(.secondary)
              Button("取消导入") { game.cancelImport() }.accessibilityIdentifier("cancel-import")
            }.padding(24).frame(width: min(340, geometry.size.width * 0.8))
              .modifier(GamePanelStyle())
          }
          if game.audioOutputError != nil {
            Button("音频输出已暂停 · 恢复选项") { game.showAudioOutputRecovery() }
              .padding(12).modifier(GamePanelStyle())
              .frame(maxHeight: .infinity, alignment: .top).padding(.top, 12)
          } else if game.mediaPolicy.requiresUserResume {
            VStack(spacing: 8) {
              Text(game.mediaPauseMessage).font(.callout).multilineTextAlignment(.center)
              Button("恢复音视频") { game.resumeMediaByUser() }
                .disabled(!game.canResumeMedia).accessibilityIdentifier("resume-media")
            }.padding(16).modifier(GamePanelStyle())
              .frame(maxWidth: min(420, geometry.size.width * 0.9))
              .frame(maxHeight: .infinity, alignment: .top).padding(.top, 12)
          }
        }
      }.clipped()
    }
    .foregroundStyle(GameTheme.text).tint(GameTheme.accent)
    .background(GameTheme.background).preferredColorScheme(.light)
    #if os(iOS)
      .background {
        // Keep external-keyboard save available without a transient UIKit Menu.
        Button("存档") {
          openSaves(saving: true)
          synchronizeModalState()
        }
        .keyboardShortcut("s", modifiers: .command)
        .disabled(title || game.interfaceHidden || hasPresentedModal || game.importing)
        .hidden().allowsHitTesting(false).accessibilityHidden(true)
      }
    #endif
    .overlay(alignment: .top) {
      if let text = game.saveWarning ?? game.notice {
        Text(text).font(.caption).padding(10).background(.regularMaterial, in: Capsule())
          .padding(.top, 50).allowsHitTesting(false)
      }
    }
    #if os(macOS)
      .fileImporter(
        isPresented: $importing, allowedContentTypes: [.folder], allowsMultipleSelection: false
      ) { result in
        switch result {
        case .success(let urls): if let url = urls.first { game.importDirectory(url) }
        case .failure(let error): game.error = error.localizedDescription
        }
      }
    #endif
    .alert(
      "运行提示", isPresented: Binding(get: { game.error != nil }, set: { if !$0 { game.error = nil } })
    ) {
      if game.audioOutputError != nil {
        Button("静音并继续") { game.continueMuted() }
        Button("重试音频输出") { game.retryAudioOutput() }
      } else if game.hasAudioFailure {
        Button("跳过此音频并继续") { game.skipFailedAudio() }
        Button("重试此音频") { game.retryFailedAudio() }
      } else {
        if game.snapshot?.scene.video != nil {
          Button("重试视频") { game.retryVideo() }
          Button("返回标题") {
            game.error = nil
            game.action("title")
          }
        } else {
          Button("确定") { game.error = nil }
        }
      }
    } message: {
      Text(game.error ?? "")
    }
    .confirmationDialog(
      "返回标题？将保留最后成功的自动进度，手动槽位不变。", isPresented: $returnToTitle, titleVisibility: .visible
    ) {
      Button("返回标题") {
        if game.snapshot?.blocked != nil || game.autoSave() {
          game.action("title")
        } else {
          game.error = "自动保存失败，未返回标题。请先存入手动槽位，或明确选择放弃未保存进度。"
        }
      }
      if game.saveWarning != nil {
        Button("放弃未保存进度并返回", role: .destructive) { game.action("title") }
      }
      Button("取消", role: .cancel) {}
    }
    .confirmationDialog("开始新游戏会替换自动进度，手动存档不变。", isPresented: $newGame, titleVisibility: .visible) {
      Button("开始新游戏", role: .destructive) { game.action("new") }
      Button("取消", role: .cancel) {}
    }
    #if os(iOS)
      .sheet(
        isPresented: Binding(
          get: { gameplayMenu.isPresented },
          set: { if !$0 { gameplayMenu.dismiss() } }),
        onDismiss: finishGameplayMenu
      ) {
        GameplayMenuSheet(
          muted: game.muted,
          hasVoice: !(game.snapshot?.scene.voice.asset.isEmpty ?? true),
          hasLibrary: game.handle != nil,
          select: { _ = gameplayMenu.select($0) },
          close: { gameplayMenu.dismiss() })
      }
    #endif
    .sheet(isPresented: $showSaves) {
      SaveManager(game: game, saving: saving)
    }
    .sheet(isPresented: $showSettings) {
      Settings(game: game)
    }
    .sheet(isPresented: $showCollection) {
      CollectionMenu(game: game, replay: collectionIsReplay)
    }
    .sheet(isPresented: $showHistory) {
      Backlog(game: game)
    }
    .sheet(isPresented: $showDiagnostics) {
      document(
        "运行状态",
        text: ([
          "Windows DLL / Steam API：未加载",
          "已加载资源包（后者优先）：\(game.snapshot?.archives?.joined(separator: " → ") ?? "无")",
          "存档保存剧情变量、待选选项、计时器、动画、视频标识和音频设置；全局解锁进度独立保存。读档从头播放当前音频和视频，不恢复采样级媒体位置。",
          game.snapshot?.blocked ?? "",
        ] + (game.snapshot?.warnings ?? [])).filter { !$0.isEmpty }.joined(separator: "\n\n"),
        close: { showDiagnostics = false })
    }
    .onAppear {
      game.sceneActivityChanged(phase == .active)
      synchronizeModalState()
      #if os(macOS)
        game.applicationActivityChanged(NSApplication.shared.isActive)
      #endif
    }
    .onChange(of: phase) { game.sceneActivityChanged($0 == .active) }
    .onChange(of: hasPresentedModal) { _ in synchronizeModalState() }
    #if os(iOS)
      .onReceive(NotificationCenter.default.publisher(for: AVAudioSession.interruptionNotification))
      { notification in
        guard let raw = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
          let type = AVAudioSession.InterruptionType(rawValue: raw)
        else { return }
        if type == .began {
          game.interruptionBegan()
        } else {
          let rawOptions = notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0
          let shouldResume = AVAudioSession.InterruptionOptions(rawValue: rawOptions).contains(
            .shouldResume)
          game.interruptionEnded(shouldResume: shouldResume)
        }
      }
      .onReceive(NotificationCenter.default.publisher(for: AVAudioSession.routeChangeNotification))
      { notification in
        guard let raw = notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
          AVAudioSession.RouteChangeReason(rawValue: raw) == .oldDeviceUnavailable
        else { return }
        game.outputRouteLost()
      }
    #else
      .onReceive(
        NotificationCenter.default.publisher(for: NSApplication.willResignActiveNotification)
      ) { _ in game.applicationActivityChanged(false) }
      .onReceive(
        NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)
      ) { _ in game.applicationActivityChanged(true) }
      .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification))
      { _ in
        game.stopFastForward()
        game.autoSave()
        game.action("flush")
        game.stopAudio()
      }
    #endif
  }
  private var readingToolbar: some View {
    ViewThatFits(in: .horizontal) {
      toolbarContent(showBrand: true)
      toolbarContent(showBrand: false)
    }
    .padding(.horizontal, 12).padding(.vertical, 4)
    .background(GameTheme.background)
    .overlay(alignment: .bottom) { GameTheme.border.frame(height: 1) }
    .disabled(game.importing || game.modalOpen || game.error != nil)
    .zIndex(1)
  }

  private func toolbarContent(showBrand: Bool) -> some View {
    HStack(spacing: 8) {
      if showBrand {
        Text("Koikari").font(.caption).foregroundStyle(.secondary).accessibilityHidden(true)
      }
      Spacer(minLength: 8)
      Button {
        showHistory = true
      } label: {
        Label("回看", systemImage: "text.alignleft")
          .labelStyle(GameToolbarLabelStyle(compact: !showBrand))
      }.buttonStyle(.bordered).controlSize(.large).accessibilityIdentifier("backlog")
        .accessibilityLabel("回看")
      Button {
        game.toggleFastForward()
      } label: {
        Label(
          game.fastForward ? "快进中 · 停止" : "快进",
          systemImage: game.fastForward ? "stop.fill" : "forward.end"
        )
        .labelStyle(GameToolbarLabelStyle(compact: !showBrand))
      }
      .accessibilityLabel(game.fastForward ? "快进中，停止快进" : "快进")
      .buttonStyle(.bordered).controlSize(.large)
      .disabled(!game.fastForward && !game.canAdvance)
      .keyboardShortcut("f", modifiers: []).accessibilityIdentifier("fast-forward")
      Button {
        game.hideInterface()
      } label: {
        Label("隐藏界面", systemImage: "eye.slash").labelStyle(.iconOnly)
      }
      .buttonStyle(.bordered).controlSize(.large).disabled(!game.canHideInterface)
      .accessibilityIdentifier("hide-interface")
      .help("隐藏界面欣赏画面；再次点击只恢复界面，不翻页")
      #if os(iOS)
        Button(action: openGameplayMenu) {
          Label("菜单", systemImage: "line.3.horizontal")
            .labelStyle(GameToolbarLabelStyle(compact: !showBrand))
        }
        .buttonStyle(.bordered).controlSize(.large)
        .accessibilityIdentifier("compact-game-menu").accessibilityLabel("游戏菜单")
      #else
        Menu {
          Button {
            openSaves(saving: true)
          } label: {
            Label("存档", systemImage: "square.and.arrow.down")
          }
          .keyboardShortcut("s", modifiers: .command).accessibilityIdentifier("save")
          Button {
            openSaves(saving: false)
          } label: {
            Label("读档", systemImage: "tray.and.arrow.up")
          }
          .accessibilityIdentifier("load")
          Divider()
          Button {
            game.replayVoice()
          } label: {
            Label("重播语音", systemImage: "speaker.wave.2")
          }
          .disabled(game.snapshot?.scene.voice.asset.isEmpty ?? true)
          Button {
            game.toggleMute()
          } label: {
            Label(
              game.muted ? "取消静音" : "静音",
              systemImage: game.muted ? "speaker.wave.2" : "speaker.slash"
            )
          }
          Button {
            showSettings = true
          } label: {
            Label("设置", systemImage: "slider.horizontal.3")
          }
          .accessibilityIdentifier("settings")
          Divider()
          Button("CG 鉴赏") { openCollection(replay: false) }.disabled(game.handle == nil)
          Button("场景回想") { openCollection(replay: true) }.disabled(game.handle == nil)
          Button("兼容状态") { showDiagnostics = true }
          #if os(macOS)
            Button("切换全屏") { NSApp.keyWindow?.toggleFullScreen(nil) }
              .keyboardShortcut("f", modifiers: [.command, .control])
          #endif
          Divider()
          Button {
            returnToTitle = true
          } label: {
            Label("返回标题", systemImage: "house")
          }
        } label: {
          Label("菜单", systemImage: "line.3.horizontal")
            .labelStyle(GameToolbarLabelStyle(compact: !showBrand))
            .font(.callout.weight(.medium)).padding(.horizontal, 12).frame(minHeight: 44)
        }
        .menuStyle(.automatic)
        .fixedSize()
        .accessibilityIdentifier("compact-game-menu").accessibilityLabel("游戏菜单")
      #endif
    }
  }

  private var hasPresentedModal: Bool {
    importing || showHistory || showDiagnostics || returnToTitle || showSaves || showSettings
      || newGame || showCollection || game.error != nil || gameplayMenu.blocksGameplay
  }

  private func synchronizeModalState() {
    let blocked = hasPresentedModal
    if game.modalOpen != blocked { game.modalOpen = blocked }
    if blocked { game.stopFastForward() } else { game.resumePendingVideo() }
  }

  private func openGameplayMenu() {
    guard !hasPresentedModal, !game.importing, !game.modalOpen,
      gameplayMenu.present()
    else { return }
    // Synchronous, before UIKit presents: no tick/fast-forward may replace the
    // menu's anchor or advance the story underneath the player's selection.
    synchronizeModalState()
  }

  private func finishGameplayMenu() {
    let action = gameplayMenu.didDismiss()
    // An error/import/another presentation may arrive during the dismissal animation.
    // Never dispatch a stale selection over that new blocker.
    guard !hasPresentedModal, !game.importing else {
      synchronizeModalState()
      return
    }
    // Open the destination only after UIKit releases the menu presenter. Set
    // destination state first so there is no unpaused frame between modals.
    switch action {
    case .save: openSaves(saving: true)
    case .load: openSaves(saving: false)
    case .settings: showSettings = true
    case .gallery: openCollection(replay: false)
    case .replay: openCollection(replay: true)
    case .diagnostics: showDiagnostics = true
    case .returnToTitle: returnToTitle = true
    default: break
    }
    synchronizeModalState()
    // These non-modal actions must run after the modal playback gate is released.
    switch action {
    case .replayVoice: game.replayVoice()
    case .toggleMute: game.toggleMute()
    default: break
    }
  }

  private func openSaves(saving: Bool) {
    game.stopFastForward()
    guard !game.importing, game.error == nil else { return }
    game.reloadSaves()
    guard game.error == nil else { return }
    self.saving = saving
    showSaves = true
  }

  private func openCollection(replay: Bool) {
    collectionIsReplay = replay
    if game.reloadCollection(replay: replay) { showCollection = true }
  }
  private func document(_ title: String, text: String, close: @escaping () -> Void) -> some View {
    GameDocumentSheet(title: title, text: text, close: close)
  }
}

@main struct KoikariApp: App {
  @StateObject private var game = Game()
  var body: some Scene {
    WindowGroup {
      Content(game: game)
        #if os(macOS)
          .frame(minWidth: 800, minHeight: 488)
        #endif
    }
    #if os(macOS)
      .defaultSize(width: 1280, height: 758)
      .commands {
        CommandMenu("剧情") {
          Button("下一句") { game.next() }.keyboardShortcut(.rightArrow, modifiers: [])
          .disabled(
            game.modalOpen || game.importing || game.error != nil
              || (!game.interfaceHidden && !game.fastForward && !game.canAdvance))
          Button("停止快进或显示界面") { game.stopReadingMode() }.keyboardShortcut(.escape, modifiers: [])
          .disabled(!game.interfaceHidden && !game.fastForward)
        }
      }
    #endif
  }
}
