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
  var id: Int { slot }
  var name: String { slot == 9 ? "自动进度" : "手动槽位 \(slot)" }
  enum CodingKeys: String, CodingKey {
    case slot, exists, compatible, pages, speaker, text, error
    case savedAt = "saved_at"
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
  @Published var error: String? { didSet { reconcileMediaPlayback() } }
  @Published var importing = false { didSet { reconcileMediaPlayback() } }
  @Published var hasSave = false
  @Published var slots: [SaveSlot] = []
  @Published var timeline: [PastPage] = []
  @Published var fastForward = false
  @Published var modalOpen = false { didSet { reconcileMediaPlayback() } }
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
  private(set) var creditImages: [String: Image] = [:]
  private(set) var messageGlyph: MessageGlyphState?
  private(set) var messageGlyphSheet: Image?
  var hasAudioFailure: Bool { failedAudioClip != nil }
  private var pendingVideoCompletion: UInt64?
  var choices: [StoryChoice] { snapshot?.flow?.choices ?? [] }
  var canAdvance: Bool {
    guard active, !modalOpen, !importing, error == nil, let state = snapshot else { return false }
    return !state.title && !state.finished && state.blocked == nil && choices.isEmpty
      && state.scene.video == nil
      && (state.flow?.acceptsAdvanceWait ?? true)
  }
  var canChoose: Bool {
    guard active, !modalOpen, !importing, error == nil, let state = snapshot else { return false }
    return !state.title && !state.finished && state.blocked == nil && state.scene.video == nil
      && (state.flow?.waitMs ?? 0) == 0
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
          guard event.keyCode == 53, let self, self.fastForward else { return false }
          self.stopFastForward()
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
  @discardableResult func autoSave() -> Bool {
    guard let handle, let state = snapshot, !state.title, state.blocked == nil else { return false }
    if "save:9".withCString({ kr_action(handle, $0) }) == 0 {
      saveWarning = "自动进度保存失败：\(rustError())"
      return false
    } else {
      saveWarning = nil
      hasSave = true
      return true
    }
  }
  @discardableResult func action(_ action: String) -> Bool {
    guard let handle, !importing else { return false }
    if action != "advance" { stopFastForward() }
    if action == "advance", !canAdvance { return false }
    if action.hasPrefix("choose:"), !canChoose { return false }
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
    guard !modalOpen, !importing, error == nil else { return }
    if fastForward { stopFastForward() } else { action("advance") }
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
      if replay { replays = entries } else { gallery = entries }
      return true
    } catch {
      self.error = "鉴赏列表读取失败：\(error.localizedDescription)"
      return false
    }
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
    if mediaPlaybackAllowed, let previous = lastFrame {
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
  @Environment(\.scenePhase) private var phase
  private var title: Bool { game.snapshot?.title ?? true }
  var body: some View {
    VStack(spacing: 0) {
      ViewThatFits(in: .horizontal) {
        HStack(spacing: 14) {
          Text("KOIKARI / REBORN").font(.system(size: 12, weight: .semibold, design: .monospaced))
          Spacer()
          if !title {
            Button("上一句") { game.action("back") }.disabled(game.timeline.isEmpty)
              .keyboardShortcut(.leftArrow, modifiers: []).accessibilityIdentifier("back")
            Button(game.fastForward ? "停止快进" : "快进") { game.toggleFastForward() }
              .disabled(!game.fastForward && !game.canAdvance)
              .keyboardShortcut("f", modifiers: []).accessibilityIdentifier("fast-forward")
            Button("存档") {
              saving = true
              game.reloadSaves()
              showSaves = true
            }
            .keyboardShortcut("s", modifiers: .command).accessibilityIdentifier("save")
            Button("读档") {
              saving = false
              game.reloadSaves()
              showSaves = true
            }
            .accessibilityIdentifier("load")
            Button("回看") { showHistory = true }
            Button("标题") { returnToTitle = true }
          }
          Button(game.muted ? "取消静音" : "静音") { game.toggleMute() }
          Button("设置") { showSettings = true }.accessibilityIdentifier("settings")
          Menu("更多") {
            Button("重播语音") { game.replayVoice() }.disabled(
              game.snapshot?.scene.voice.asset.isEmpty ?? true)
            Button("兼容状态") { showDiagnostics = true }
            Button("CG 鉴赏") { openCollection(replay: false) }.disabled(game.handle == nil)
            Button("场景回想") { openCollection(replay: true) }.disabled(game.handle == nil)
            #if os(macOS)
              Button("切换全屏") { NSApp.keyWindow?.toggleFullScreen(nil) }
                .keyboardShortcut("f", modifiers: [.command, .control])
            #endif
          }
        }
        HStack {
          Text("KOIKARI").font(.system(size: 12, weight: .semibold, design: .monospaced))
          Spacer()
          if !title {
            Button(game.fastForward ? "停止快进" : "下一句") { game.next() }
              .disabled(!game.fastForward && !game.canAdvance).accessibilityIdentifier(
                "advance-compact")
          }
          Menu("游戏") {
            if !title {
              Button("上一句") { game.action("back") }.disabled(game.timeline.isEmpty)
              Button(game.fastForward ? "停止快进" : "快进") { game.toggleFastForward() }
                .disabled(!game.fastForward && !game.canAdvance)
              Button("存档") {
                saving = true
                game.reloadSaves()
                showSaves = true
              }
              Button("读档") {
                saving = false
                game.reloadSaves()
                showSaves = true
              }
              Button("回看") { showHistory = true }
              Button("返回标题") { returnToTitle = true }
            }
            Button("CG 鉴赏") { openCollection(replay: false) }.disabled(game.handle == nil)
            Button("场景回想") { openCollection(replay: true) }.disabled(game.handle == nil)
            Button("重播语音") { game.replayVoice() }.disabled(
              game.snapshot?.scene.voice.asset.isEmpty ?? true)
            Button(game.muted ? "取消静音" : "静音") { game.toggleMute() }
            Button("设置") { showSettings = true }
            Button("兼容状态") { showDiagnostics = true }
          }.accessibilityIdentifier("compact-game-menu")
        }
      }.font(.system(size: 13)).padding(.horizontal, 16).frame(height: 44).background(
        Color(red: 0.06, green: 0.08, blue: 0.13)
      ).disabled(game.importing || game.modalOpen).zIndex(1)
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
            VStack(spacing: max(6, height * 0.025)) {
              Text("租借男友").font(.system(size: max(24, height * 0.085), weight: .light))
              Text("Rust + Metal · 原生运行时").font(.system(size: 12)).foregroundStyle(.secondary)
              if game.handle != nil {
                Button("新游戏") {
                  if game.hasSave { newGame = true } else { game.action("new") }
                }.accessibilityIdentifier("new-game")
                Button("继续游戏") { game.action("continue") }.disabled(!game.hasSave)
                  .accessibilityIdentifier("continue")
                Button("存档管理") {
                  saving = false
                  game.reloadSaves()
                  showSaves = true
                }
                HStack {
                  Button("CG 鉴赏") { openCollection(replay: false) }
                  Button("场景回想") { openCollection(replay: true) }
                }
              }
              #if os(macOS)
                Button(game.handle == nil ? "导入 XP3 所在文件夹" : "更换资源文件夹") { importing = true }
                  .accessibilityIdentifier("import-assets")
              #else
                Button("重新检查资源") { game.reloadDocumentResources() }
                  .accessibilityIdentifier("reload-resources")
                Text("文件 → 我的 iPad/iPhone → 本应用 → GameResources")
                  .font(.system(size: 11)).multilineTextAlignment(.center)
                Text(game.resourceStatus).font(.system(size: 11)).foregroundStyle(.secondary)
                  .multilineTextAlignment(.center)
              #endif
              Text("本地 data.xp3，可附加 adult.xp3 / patch*.xp3 · 不加载 Windows / Steam")
                .font(.system(size: 10)).foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            .buttonStyle(.borderedProminent).tint(Color(red: 0.21, green: 0.35, blue: 0.48))
            .padding(max(12, height * 0.04)).background(
              .ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18)
            )
            .frame(maxWidth: min(520, width * 0.9)).position(
              x: geometry.size.width / 2, y: geometry.size.height / 2)
          } else if let state = game.snapshot {
            if let credits = state.scene.credits {
              CreditsView(credits: credits, images: game.creditImages)
                .frame(width: width, height: height)
                .position(x: geometry.size.width / 2, y: geometry.size.height / 2)
            }
            if state.scene.messageVisible && state.scene.video == nil && !state.finished {
              let portraitWidth =
                state.sprites.values.contains { $0.position == "fc" } ? width * 0.20 : 0
              let panelHeight = min(height * 0.6, max(110, height * 0.32 * CGFloat(game.textScale)))
              VStack(alignment: .leading, spacing: 6) {
                HStack {
                  Text(state.scene.speaker).font(
                    .system(size: max(14, height * 0.031), weight: .semibold))
                  Spacer()
                  Text("\(state.pages)").font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(.secondary)
                }
                ScrollView {
                  Text(state.scene.text).font(
                    .system(size: max(14, height * 0.032) * CGFloat(game.textScale))
                  ).lineSpacing(3)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }.id("\(state.pages)-\(state.scene.text)").frame(maxHeight: .infinity)
                HStack {
                  Button("上一句") { game.action("back") }.disabled(game.timeline.isEmpty)
                  Spacer()
                  if let glyph = game.messageGlyph, let sheet = game.messageGlyphSheet {
                    MessageGlyphView(
                      glyph: glyph, sheet: sheet, elapsed: state.scene.visual.elapsedMs)
                  }
                  Button("下一句  ›") { game.next() }.disabled(!game.canAdvance)
                    .keyboardShortcut(.space, modifiers: []).accessibilityIdentifier("advance")
                }
              }
              .padding(.leading, portraitWidth)
              .padding(max(10, height * 0.02)).frame(
                width: width * 0.94, height: panelHeight, alignment: .leading
              )
              .background {
                RoundedRectangle(cornerRadius: 12)
                  .fill(Color(red: 0.04, green: 0.06, blue: 0.12).opacity(0.90))
                  .padding(.leading, portraitWidth)
              }
              .position(
                x: geometry.size.width / 2,
                y: top + height - panelHeight / 2 - height * 0.02
              )
              .accessibilityElement(children: .contain)
            }
            if state.interlude != nil && game.choices.isEmpty && state.scene.video == nil {
              VStack {
                Spacer()
                Button("人物介绍 · 点击继续剧情  ›") { game.next() }
                  .disabled(!game.canAdvance)
                  .keyboardShortcut(.space, modifiers: []).accessibilityIdentifier(
                    "continue-interlude"
                  )
                  .padding(12).background(.regularMaterial, in: Capsule()).padding(16)
              }
            }
            if !game.choices.isEmpty {
              Color.black.opacity(0.35).contentShape(Rectangle()).onTapGesture {}
              VStack(spacing: 14) {
                Text("请选择").font(.headline)
                ScrollView {
                  VStack(spacing: 12) {
                    ForEach(game.choices) { choice in
                      Button {
                        game.choose(choice)
                      } label: {
                        Text(choice.text).font(.system(size: 17 * game.textScale))
                          .multilineTextAlignment(.center).frame(maxWidth: .infinity).padding(10)
                      }.buttonStyle(.borderedProminent)
                        .disabled(!choice.enabled || !game.canChoose)
                        .accessibilityIdentifier("choice-\(choice.id)")
                    }
                  }
                }
              }.padding(24).frame(width: min(620, width * 0.88), height: min(450, height * 0.85))
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
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
                ProgressView("正在准备视频…")
              }
            }
            if state.finished {
              Color.black.opacity(0.65).contentShape(Rectangle()).onTapGesture {}
              VStack(spacing: 18) {
                if let ending = state.flow?.ending {
                  Text("故事结束").font(.largeTitle)
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
              }.padding(30).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
            }
            if state.blocked != nil {
              Text("遇到未支持指令，已停止执行。请查看兼容状态。").font(.caption).padding().background(.red.opacity(0.8))
                .frame(maxHeight: .infinity, alignment: .top)
            }
          }
          if game.importing {
            Color.black.opacity(0.8)
            VStack(spacing: 12) {
              Text("正在导入资源，请保持应用打开")
              ProgressView(value: game.importProgress)
              Text(game.importDetail).font(.caption).foregroundStyle(.secondary)
              Button("取消导入") { game.cancelImport() }.accessibilityIdentifier("cancel-import")
            }.padding(24).frame(width: min(340, geometry.size.width * 0.8))
              .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
          }
          if game.audioOutputError != nil {
            Button("音频输出已暂停 · 恢复选项") { game.showAudioOutputRecovery() }
              .padding(12).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
              .frame(maxHeight: .infinity, alignment: .top).padding(.top, 12)
          } else if game.mediaPolicy.requiresUserResume {
            VStack(spacing: 8) {
              Text(game.mediaPauseMessage).font(.callout).multilineTextAlignment(.center)
              Button("恢复音视频") { game.resumeMediaByUser() }
                .disabled(!game.canResumeMedia).accessibilityIdentifier("resume-media")
            }.padding(16).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
              .frame(maxWidth: min(420, geometry.size.width * 0.9))
              .frame(maxHeight: .infinity, alignment: .top).padding(.top, 12)
          }
        }
      }.clipped()
    }
    .foregroundStyle(.white).preferredColorScheme(.dark)
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
      #if os(macOS)
        game.applicationActivityChanged(NSApplication.shared.isActive)
      #endif
    }
    .onChange(of: phase) { game.sceneActivityChanged($0 == .active) }
    .onChange(
      of: importing || showHistory || showDiagnostics || returnToTitle || showSaves || showSettings
        || newGame || showCollection || game.error != nil
    ) {
      game.modalOpen = $0
      if $0 { game.stopFastForward() } else { game.resumePendingVideo() }
    }
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
  private func openCollection(replay: Bool) {
    collectionIsReplay = replay
    if game.reloadCollection(replay: replay) { showCollection = true }
  }
  private func document(_ title: String, text: String, close: @escaping () -> Void) -> some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack {
        Text(title).font(.headline)
        Spacer()
        Button("关闭", action: close)
      }
      ScrollView {
        Text(text).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
      }
    }.padding(24).frame(minWidth: 300, idealWidth: 650, minHeight: 250, idealHeight: 450)
      .preferredColorScheme(.dark)
  }
}

struct CollectionMenu: View {
  @ObservedObject var game: Game
  let replay: Bool
  @Environment(\.dismiss) private var dismiss
  @State private var selected: CollectionEntry?
  @State private var confirmReplay = false
  private var entries: [CollectionEntry] { replay ? game.replays : game.gallery }
  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      HStack {
        Text(replay ? "场景回想" : "CG 鉴赏").font(.headline)
        Spacer()
        Text("\(entries.filter { $0.unlocked }.count) / \(entries.count)").foregroundStyle(
          .secondary)
        Button("关闭") {
          game.hideCG()
          dismiss()
        }
      }
      if selected != nil && !replay {
        MetalCanvas(game: game).aspectRatio(16 / 9, contentMode: .fit)
          .accessibilityIdentifier("gallery-preview")
        Button("返回列表") {
          game.hideCG()
          selected = nil
        }
      } else if entries.isEmpty {
        Text("暂无可显示条目。解锁进度随剧情推进保存。").foregroundStyle(.secondary)
      } else {
        ScrollView {
          LazyVStack(spacing: 10) {
            ForEach(entries) { entry in
              Button {
                if replay {
                  selected = entry
                  confirmReplay = true
                } else if game.showCG(entry) {
                  selected = entry
                }
              } label: {
                HStack {
                  Image(systemName: entry.unlocked ? (replay ? "play.circle" : "photo") : "lock")
                  Text(entry.unlocked ? entry.title : "尚未解锁").multilineTextAlignment(.leading)
                  Spacer()
                }.padding(12).frame(maxWidth: .infinity)
              }.buttonStyle(.bordered).disabled(!entry.unlocked)
                .accessibilityIdentifier("\(replay ? "replay" : "cg")-\(entry.id)")
            }
          }
        }
      }
      Spacer(minLength: 0)
    }.padding(24).frame(minWidth: 300, idealWidth: 720, minHeight: 300, idealHeight: 560)
      .preferredColorScheme(.dark)
      .onDisappear { game.hideCG() }
      .confirmationDialog(
        "开始回想？当前自动进度会更新，手动存档保留。", isPresented: $confirmReplay, titleVisibility: .visible
      ) {
        Button("开始回想") {
          if let selected, game.action("replay:\(selected.id)") { dismiss() }
        }
        Button("取消", role: .cancel) { selected = nil }
      }
  }
}

struct Backlog: View {
  @ObservedObject var game: Game
  @Environment(\.dismiss) private var dismiss
  @State private var pending: PastPage?
  @State private var confirm = false
  @State private var fullHistory = false
  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Text("回看与回溯（最近 100 页）").font(.headline)
        Spacer()
        Button("关闭") { dismiss() }
      }
      Text("点击记录可回到当时画面与对白；自动进度会更新，手动槽位不变。读档后也可回溯。")
        .font(.caption).foregroundStyle(.secondary)
      if game.timeline.isEmpty { Text("还没有可回溯的页面。") }
      ScrollViewReader { proxy in
        ScrollView {
          LazyVStack(alignment: .leading, spacing: 14) {
            ForEach(game.timeline) { page in
              Button {
                pending = page
                confirm = true
              } label: {
                VStack(alignment: .leading, spacing: 4) {
                  Text("第 \(page.page) 页 · \(page.interlude ? "人物介绍" : page.speaker)").font(
                    .caption
                  ).foregroundStyle(.secondary)
                  Text(page.interlude ? "回到人物介绍画面" : page.text).frame(
                    maxWidth: .infinity, alignment: .leading)
                  Text("回溯至此").font(.caption).foregroundStyle(.blue)
                }.padding(10).background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
              }.buttonStyle(.plain).id(page.id).accessibilityIdentifier("rewind-\(page.page)")
            }
          }
        }.onAppear { if let last = game.timeline.last { proxy.scrollTo(last.id, anchor: .bottom) } }
      }
      Button("完整文字回看（最近 300 页）") { fullHistory = true }
    }.padding(20).frame(minWidth: 300, idealWidth: 620, minHeight: 250, idealHeight: 520)
      .preferredColorScheme(.dark)
      .sheet(isPresented: $fullHistory) {
        VStack(alignment: .leading, spacing: 12) {
          HStack {
            Text("完整文字回看").font(.headline)
            Spacer()
            Button("关闭") { fullHistory = false }
          }
          ScrollView {
            Text(game.snapshot?.history.joined(separator: "\n\n") ?? "").frame(
              maxWidth: .infinity, alignment: .leading
            ).textSelection(.enabled)
          }
        }.padding(20).frame(minWidth: 300, idealWidth: 620, minHeight: 250, idealHeight: 520)
          .preferredColorScheme(.dark)
      }
      .confirmationDialog(
        "回到第 \(pending?.page ?? 0) 页？自动进度将更新，手动存档不变。", isPresented: $confirm,
        titleVisibility: .visible
      ) {
        Button("确认回溯") { if let pending, game.action("rewind:\(pending.page)") { dismiss() } }
        Button("取消", role: .cancel) {}
      }
  }
}

struct SaveManager: View {
  @ObservedObject var game: Game
  let saving: Bool
  @Environment(\.dismiss) private var dismiss
  @State private var pending: SaveSlot?
  @State private var confirm = false

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Text(saving ? "保存进度" : "读取进度").font(.headline)
        Spacer()
        Button("关闭") { dismiss() }
      }
      Text("6 个手动槽位独立保存；自动进度随剧情更新，不覆盖手动槽位。")
        .font(.caption).foregroundStyle(.secondary)
      if let notice = game.notice { Text(notice).font(.caption).foregroundStyle(.green) }
      ScrollView {
        VStack(spacing: 12) {
          ForEach(game.slots) { slot in
            VStack(alignment: .leading, spacing: 6) {
              HStack {
                Text(slot.name).font(.headline)
                Spacer()
                if saving && slot.slot != 9 {
                  Button("存入此槽") {
                    if slot.exists {
                      pending = slot
                      confirm = true
                    } else {
                      game.action("save:\(slot.slot)")
                    }
                  }.disabled((game.snapshot?.title ?? true) || game.snapshot?.blocked != nil)
                    .accessibilityIdentifier("save-slot-\(slot.slot)")
                } else if !saving {
                  Button("读取") {
                    pending = slot
                    confirm = true
                  }
                  .disabled(!slot.compatible).accessibilityIdentifier("load-slot-\(slot.slot)")
                }
              }
              if slot.compatible {
                Text(
                  "第 \(slot.pages) 页 · \(Date(timeIntervalSince1970: Double(slot.savedAt) / 1000).formatted(date: .abbreviated, time: .shortened))"
                )
                .font(.caption).foregroundStyle(.secondary)
                Text([slot.speaker, slot.text].filter { !$0.isEmpty }.joined(separator: "："))
                  .font(.callout).lineLimit(3)
              } else if slot.exists {
                Text("存档不可用：\(slot.error ?? "校验失败")").font(.caption).foregroundStyle(.orange)
              } else {
                Text("空槽位").font(.caption).foregroundStyle(.secondary)
              }
            }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
              .background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
          }
        }
      }
    }.padding(20).frame(minWidth: 300, idealWidth: 620, minHeight: 250, idealHeight: 520)
      .preferredColorScheme(.dark)
      .confirmationDialog(
        saving ? "覆盖\(pending?.name ?? "此槽位")？" : "读取\(pending?.name ?? "此进度")？当前自动进度将随之更新。",
        isPresented: $confirm, titleVisibility: .visible
      ) {
        Button(saving ? "确认覆盖" : "确认读取", role: saving ? .destructive : nil) {
          if let pending, game.action("\(saving ? "save" : "load"):\(pending.slot)"), !saving {
            dismiss()
          }
        }
        Button("取消", role: .cancel) {}
      }
  }
}

struct Settings: View {
  @ObservedObject var game: Game
  @Environment(\.dismiss) private var dismiss
  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      HStack {
        Text("阅读与声音").font(.headline)
        Spacer()
        Button("关闭") { dismiss() }
      }
      ScrollView {
        VStack(alignment: .leading, spacing: 14) {
          Text("对白字号：\(Int(game.textScale * 100))%")
          Slider(value: $game.textScale, in: 0.8...1.6, step: 0.1).accessibilityIdentifier(
            "text-scale")
          Text("示例：这是一段中文对白。").font(.system(size: 16 * game.textScale))
          Text("音乐音量：\(Int(game.bgmVolume * 100))%")
          Slider(value: $game.bgmVolume, in: 0...1).accessibilityIdentifier("bgm-volume")
          Text("语音音量：\(Int(game.voiceVolume * 100))%")
          Slider(value: $game.voiceVolume, in: 0...1).accessibilityIdentifier("voice-volume")
          Text("音效音量：\(Int(game.seVolume * 100))%")
          Slider(value: $game.seVolume, in: 0...1).accessibilityIdentifier("se-volume")
          Toggle(
            "全部静音",
            isOn: Binding(get: { game.muted }, set: { if $0 != game.muted { game.toggleMute() } }))
          Button("恢复默认设置") {
            game.textScale = 1
            game.bgmVolume = 0.35
            game.voiceVolume = 1
            game.seVolume = 0.7
            if game.muted { game.toggleMute() }
          }
          Text("设置自动保存。快进遇到选项、视频或结局会停止。")
            .font(.caption).foregroundStyle(.secondary)
        }
      }
    }.padding(24).frame(minWidth: 300, idealWidth: 560, minHeight: 250, idealHeight: 480)
      .preferredColorScheme(.dark)
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
          Button("停止快进") { game.stopFastForward() }.keyboardShortcut(.escape, modifiers: [])
        }
      }
    #endif
  }
}
