import AVFoundation
@preconcurrency import MetalKit
import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
  import AppKit
#endif

struct Clip: Decodable, Sendable {
  var asset: String
  var sequence: UInt64
  var looping: Bool
}
struct Dialogue: Decodable {
  var speaker: String
  var text: String
  var messageVisible: Bool
  var bgm: Clip
  var voice: Clip
  var se: Clip
  enum CodingKeys: String, CodingKey {
    case speaker, text, bgm, voice, se
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

@MainActor final class Game: ObservableObject {
  @Published var snapshot: Snapshot?
  @Published var error: String?
  @Published var importing = false
  @Published var hasSave = false
  @Published var slots: [SaveSlot] = []
  @Published var timeline: [PastPage] = []
  @Published var fastForward = false
  @Published var modalOpen = false
  private var fastTask: Task<Void, Never>?
  @Published var notice: String?
  @Published var saveWarning: String?
  @Published var importProgress = 0.0
  @Published var importDetail = "正在检查资源…"
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
      escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
        MainActor.assumeIsolated {
          guard event.keyCode == 53, let self, self.fastForward else { return event }
          self.stopFastForward()
          return nil
        }
      }
    #endif
    #if os(iOS)
      AssetImport.cleanAbandoned(in: support)
      do {
        try AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
        try AVAudioSession.sharedInstance().setActive(true)
      } catch { self.error = error.localizedDescription }
    #endif
    let args = CommandLine.arguments
    if let index = args.firstIndex(of: "--assets"), index + 1 < args.count {
      connect(URL(fileURLWithPath: args[index + 1]))
    } else if let path = UserDefaults.standard.string(forKey: "assets") {
      #if os(iOS)
        if let directory = AssetLocation.resolve(path, relativeTo: support) {
          connect(directory)
        } else {
          error = "无法恢复资源位置，请重新导入 XP3 所在文件夹。"
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
      guard let location = AssetLocation.identifier(for: directory, relativeTo: support) else {
        error = "请先将 XP3 导入应用资源目录。"
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
    if let old = handle { kr_close(old) }
    stopAudio()
    cache.removeAll()
    cacheOrder.removeAll()
    handle = next
    renderFailed = false
    error = nil
    saveWarning = nil
    UserDefaults.standard.set(location, forKey: "assets")
    refresh(forceAudio: true)
    reloadSaves()
    return true
  }
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
    guard let handle, let json = kr_saves(handle) else { return }
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
    if action == "advance", snapshot?.finished == true || snapshot?.blocked != nil { return false }
    let ok = action.withCString { kr_action(handle, $0) } != 0
    if action == "title" || action == "new" { renderFailed = false }
    if !ok { error = rustError() }
    let loading = action == "continue" || action == "load" || action.hasPrefix("load:")
    let rewinding = action == "back" || action.hasPrefix("rewind:")
    if loading || rewinding { renderFailed = false }
    refresh(forceAudio: loading || rewinding || action == "new" || action == "title")
    if ok && !fastForward && (loading || rewinding || action == "new" || action == "advance") {
      autoSave()
    }
    if action != "advance" { reloadSaves() }
    if ok && action.hasPrefix("save:") { inform("已保存到手动槽位 \(action.dropFirst(5))") }
    if ok && loading { inform("已恢复第 \(snapshot?.pages ?? 0) 页") }
    return ok
  }
  func next() {
    guard !modalOpen else { return }
    if fastForward { stopFastForward() } else { action("advance") }
  }
  func toggleFastForward() {
    if fastForward {
      stopFastForward()
      return
    }
    guard !modalOpen, let state = snapshot, !state.title, !state.finished, state.blocked == nil
    else { return }
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
      if let json = kr_timeline(handle) {
        defer { kr_string_free(json) }
        timeline = try JSONDecoder().decode([PastPage].self, from: Data(String(cString: json).utf8))
      }
      if !fastForward, let scene = snapshot?.scene {
        updateAudio(scene.bgm, channel: "bgm", force: forceAudio)
        updateAudio(scene.voice, channel: "voice", force: forceAudio)
        updateAudio(scene.se, channel: "se", force: forceAudio)
      }
    } catch { self.error = error.localizedDescription }
  }
  private func updateAudio(_ clip: Clip, channel: String, force: Bool) {
    if !force && sequences[channel] == clip.sequence { return }
    sequences[channel] = clip.sequence
    let request = UUID()
    requests[channel] = request
    audioQueues[channel]?.cancelAllOperations()
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
      error = "音频读取失败：\(rustError())"
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
    queue.addOperation { [weak self] in
      var size = 0
      let wave = compressed.withUnsafeBytes { buffer in
        kr_decode_audio(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &size)
      }
      let decoded = wave.map { Data(bytes: $0, count: size) }
      let failure = decoded == nil ? self?.rustError() : nil
      if let wave { kr_bytes_free(wave, size) }
      DispatchQueue.main.async {
        guard let self, self.requests[channel] == request else { return }
        guard let decoded else {
          self.error = "音频解码失败：\(failure ?? "未知错误")"
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
        self.play(decoded, clip: clip, channel: channel)
      }
    }
  }
  private func play(_ data: Data, clip: Clip, channel: String) {
    do {
      let player = try AVAudioPlayer(data: data)
      player.numberOfLoops = clip.looping ? -1 : 0
      player.volume = volume(channel)
      player.prepareToPlay()
      if active { player.play() } else { pausedChannels.insert(channel) }
      players[channel] = player
    } catch { self.error = "音频播放失败：\(error.localizedDescription)" }
  }
  private func volume(_ channel: String) -> Float {
    muted ? 0 : Float(channel == "bgm" ? bgmVolume : (channel == "se" ? seVolume : voiceVolume))
  }
  private func applyVolume() {
    for (channel, player) in players { player.volume = volume(channel) }
  }
  func toggleMute() {
    muted.toggle()
    UserDefaults.standard.set(muted, forKey: "muted")
    applyVolume()
  }
  func replayVoice() {
    stopFastForward()
    guard let clip = snapshot?.scene.voice, !clip.asset.isEmpty else { return }
    updateAudio(clip, channel: "voice", force: true)
  }
  func pauseAudio() {
    guard active else { return }
    active = false
    for (channel, player) in players where player.isPlaying {
      pausedChannels.insert(channel)
      player.pause()
    }
  }
  func resumeAudio() {
    guard !active else { return }
    active = true
    for channel in pausedChannels { players[channel]?.play() }
    pausedChannels.removeAll()
  }
  func stopAudio() {
    for player in players.values { player.stop() }
    players.removeAll()
    sequences.removeAll()
    requests.removeAll()
    pausedChannels.removeAll()
    for queue in audioQueues.values { queue.cancelAllOperations() }
  }
  func render(_ view: MTKView) {
    guard active, let handle, !renderFailed, let layer = view.layer as? CAMetalLayer else { return }
    if kr_draw(
      handle, Unmanaged.passUnretained(layer).toOpaque(), Float(view.drawableSize.width),
      Float(view.drawableSize.height)) == 0
    {
      renderFailed = true
      let message = rustError()
      DispatchQueue.main.async { self.error = "Metal 绘制失败：\(message)" }
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
  @Environment(\.scenePhase) private var phase
  private var title: Bool { game.snapshot?.title ?? true }
  var body: some View {
    VStack(spacing: 0) {
      HStack(spacing: 14) {
        Text("KOIKARI / REBORN").font(.system(size: 12, weight: .semibold, design: .monospaced))
        Spacer()
        if !title {
          Button("上一句") { game.action("back") }.disabled(game.timeline.isEmpty)
            .keyboardShortcut(.leftArrow, modifiers: []).accessibilityIdentifier("back")
          Button(game.fastForward ? "停止快进" : "快进") { game.toggleFastForward() }
            .disabled(game.snapshot?.finished == true || game.snapshot?.blocked != nil)
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
              Text("Rust + Metal · 开场章节预览").font(.system(size: 12)).foregroundStyle(.secondary)
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
              }
              Button(game.handle == nil ? "导入 XP3 所在文件夹" : "更换资源文件夹") { importing = true }
                .accessibilityIdentifier("import-assets")
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
            if state.scene.messageVisible {
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
                  Button(state.finished ? "本章预览结束" : "下一句  ›") { game.next() }.disabled(
                    state.finished || state.blocked != nil
                  ).keyboardShortcut(.space, modifiers: []).accessibilityIdentifier("advance")
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
            if state.interlude != nil {
              VStack {
                Spacer()
                Button("人物介绍 · 点击继续剧情  ›") { game.next() }
                  .keyboardShortcut(.space, modifiers: []).accessibilityIdentifier(
                    "continue-interlude"
                  )
                  .padding(12).background(.regularMaterial, in: Capsule()).padding(16)
              }
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
    .fileImporter(
      isPresented: $importing, allowedContentTypes: [.folder], allowsMultipleSelection: false
    ) { result in
      switch result {
      case .success(let urls): if let url = urls.first { game.importDirectory(url) }
      case .failure(let error): game.error = error.localizedDescription
      }
    }
    .alert(
      "运行提示", isPresented: Binding(get: { game.error != nil }, set: { if !$0 { game.error = nil } })
    ) {
      Button("确定") { game.error = nil }
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
    .sheet(isPresented: $showHistory) {
      Backlog(game: game)
    }
    .sheet(isPresented: $showDiagnostics) {
      document(
        "兼容状态 · 非完整重制",
        text: ([
          "本预览仅实现首个主剧情章节。全 TJS、分支选项、动画、转场、视频和后续章节尚未支持。", "Windows DLL / Steam API：未加载",
          "已加载资源包（后者优先）：\(game.snapshot?.archives?.joined(separator: " → ") ?? "无")",
          "存档保存剧情位置、画面、回看和音频资源标识；读档重播当前语音，不恢复采样级播放位置。", game.snapshot?.blocked ?? "",
        ] + (game.snapshot?.warnings ?? [])).filter { !$0.isEmpty }.joined(separator: "\n\n"),
        close: { showDiagnostics = false })
    }
    .onChange(of: phase) { value in
      if value == .active {
        game.resumeAudio()
      } else {
        game.stopFastForward()
        game.autoSave()
        game.pauseAudio()
      }
    }
    .onChange(
      of: importing || showHistory || showDiagnostics || returnToTitle || showSaves || showSettings
        || newGame || game.error != nil
    ) {
      game.modalOpen = $0
      if $0 { game.stopFastForward() }
    }
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
          Text("设置自动保存。首章预览仍不支持完整 TJS、路线、视频及动画。")
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
