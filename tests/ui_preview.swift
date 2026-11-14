// Visual fixtures only: compile with scripts/render-ui-previews.sh.
// This isolated model never opens the engine, user preferences or saves.
// Original image files are read only with the explicit private-artwork preview option.
// These renders test the actual presentation views, not gameplay or native input.
#if KOIKARI_UI_PREVIEW
  import AppKit
  import SwiftUI

  struct PreviewSnapshot {
    var title = false
    var history: [String] = []
    var blocked: String?
    var warnings: [String] = []
  }

  @MainActor final class Game: ObservableObject {
    @Published var handle: Int?
    @Published var hasSave: Bool
    @Published var slots: [SaveSlot] = []
    @Published var savePreviewGeneration = UUID()
    private let saveThumbnails = GameGalleryThumbnails()
    var saveThumbnailUnavailable = false
    @Published var gallery: [CollectionEntry] = []
    @Published var replays: [CollectionEntry] = []
    @Published var timeline: [PastPage] = []
    @Published var notice: String?
    @Published var collectionGeneration = UUID()
    private let thumbnails = GameGalleryThumbnails()
    var thumbnailUnavailable = false
    @Published var snapshot: PreviewSnapshot? = PreviewSnapshot()
    @Published var error: String?
    @Published var originalUI = OriginalUIArtwork()
    @Published var importing = false
    @Published var modalOpen = false
    @Published var resourceStatus: String
    @Published var textScale = 1.0
    @Published var dialogueOpacity = 0.9
    @Published var dialogueLineSpacing = 5.0
    @Published var showPageNumber = false
    @Published var bgmVolume = 0.35
    @Published var voiceVolume = 1.0
    @Published var seVolume = 0.7
    @Published var muted = false

    init(ready: Bool, saved: Bool) {
      handle = ready ? 1 : nil
      hasSave = saved
      resourceStatus =
        ready
        ? "已载入所选文件夹中的资源；直接只读访问，不创建额外副本。"
        : "尚未找到 data.xp3，请先放入资源文件。"
      if saved {
        slots = [
          SaveSlot(
            slot: 9, exists: true, compatible: true, savedAt: 1_790_000_000_000,
            pages: 128, speaker: "阅读预览", text: "明天见。故事会在这里，等待你回来。", error: nil,
            previewRevision: String(repeating: "a", count: 64))
        ]
      }
    }
    func toggleMute() { muted.toggle() }
    @discardableResult func action(_ command: String) -> Bool { false }
    func hideCG() {}
    func showCG(_ entry: CollectionEntry) -> Bool { false }
    func clearGalleryThumbnails() {
      thumbnails.clear()
      collectionGeneration = UUID()
    }
    func galleryThumbnail(_ entry: CollectionEntry) async -> Image? {
      guard entry.unlocked, !thumbnailUnavailable else { return nil }
      return await thumbnails.image(for: entry.id) { Self.syntheticImage(entry.id) }
    }
    func beginSavePreviews() {}
    func endSavePreviews() { clearSaveThumbnails() }
    func clearSaveThumbnails() {
      saveThumbnails.clear()
      savePreviewGeneration = UUID()
    }
    func reloadSaves() { clearSaveThumbnails() }
    func saveThumbnail(_ slot: SaveSlot) async -> Image? {
      guard slot.exists, slot.compatible, let revision = slot.previewRevision,
        !saveThumbnailUnavailable
      else { return nil }
      return await saveThumbnails.image(
        for: "\(slot.slot):\(revision)",
        read: {
          Self.syntheticSaveImage(slot.slot)
        })
    }
    private static func syntheticSaveImage(_ slot: Int) -> Data? {
      let image = NSImage(size: NSSize(width: 384, height: 216), flipped: false) { rect in
        NSColor(calibratedRed: 0.85, green: 0.92, blue: 0.97, alpha: 1).setFill()
        rect.fill()
        NSColor(calibratedRed: 0.70, green: 0.80, blue: 0.73, alpha: 1).setFill()
        NSRect(x: 0, y: 0, width: 384, height: 80).fill()
        NSColor(calibratedRed: 0.93, green: 0.89, blue: 0.82, alpha: 1).setFill()
        NSRect(x: 30, y: 60, width: 150, height: 120).fill()
        NSColor(calibratedRed: 0.60, green: 0.68, blue: 0.76, alpha: 1).setFill()
        NSRect(x: 44, y: 112, width: 120, height: 52).fill()
        let x = 235 + CGFloat(slot % 3) * 10
        NSColor(calibratedRed: 0.64, green: 0.57, blue: 0.72, alpha: 1).setFill()
        NSBezierPath(
          roundedRect: NSRect(x: x, y: 36, width: 54, height: 83), xRadius: 14, yRadius: 14
        ).fill()
        NSColor(calibratedRed: 0.94, green: 0.82, blue: 0.73, alpha: 1).setFill()
        NSBezierPath(ovalIn: NSRect(x: x + 8, y: 110, width: 38, height: 38)).fill()
        NSColor.white.withAlphaComponent(0.88).setFill()
        NSRect(x: 0, y: 0, width: 384, height: 30).fill()
        ("存档画面 · 合成夹具" as NSString).draw(
          at: NSPoint(x: 12, y: 7),
          withAttributes: [
            .font: NSFont.systemFont(ofSize: 14), .foregroundColor: NSColor.darkGray,
          ])
        return true
      }
      guard let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff) else {
        return nil
      }
      return bitmap.representation(using: .png, properties: [:])
    }
    func populateLibrary() {
      slots += [
        SaveSlot(
          slot: 1, exists: true, compatible: true, savedAt: 1_789_800_000_000,
          pages: 64, speaker: "示例角色", text: "阳光从窗边落进来，新的故事正要开始。", error: nil,
          previewRevision: String(repeating: "b", count: 64)),
        SaveSlot(
          slot: 2, exists: true, compatible: false, savedAt: 0, pages: 0,
          speaker: "", text: "", error: "存档与当前资源不匹配。"),
        SaveSlot(
          slot: 3, exists: false, compatible: false, savedAt: 0, pages: 0,
          speaker: "", text: "", error: nil),
        SaveSlot(
          slot: 4, exists: true, compatible: true, savedAt: 1_789_900_000_000,
          pages: 92, speaker: "旁白", text: "很长的合成存档摘要，用来检查卡片中的文字换行。更多的文字只会折叠在摘要中，不会挤走操作按钮。",
          error: nil, previewRevision: String(repeating: "c", count: 64)),
        SaveSlot(
          slot: 5, exists: false, compatible: false, savedAt: 0, pages: 0,
          speaker: "", text: "", error: nil),
        SaveSlot(
          slot: 6, exists: false, compatible: false, savedAt: 0, pages: 0,
          speaker: "", text: "", error: nil),
      ]
      gallery = (1...8).map { number in
        CollectionEntry(
          id: "synthetic-cg-\(number)",
          title: number == 2 ? "很长的示例场景名称，用来检查标题换行与卡片高度" : "示例场景 \(number)",
          asset: "", unlocked: [1, 2, 4].contains(number))
      }
      replays = (1...6).map { number in
        CollectionEntry(
          id: "synthetic-replay-\(number)", title: "示例回想 \(number)",
          asset: "", unlocked: number < 3)
      }
      timeline = [
        PastPage(page: 125, speaker: "", text: "阳光越过窗沿，轻轻落在书页上。", interlude: false),
        PastPage(page: 126, speaker: "示例角色", text: "「今天，也想和你多待一会儿。」", interlude: false),
        PastPage(page: 127, speaker: "", text: "", interlude: true),
        PastPage(
          page: 128, speaker: "示例角色", text: "「明天见。」\n把今天的故事暂时放在这里，下一次再从这一页继续。", interlude: false),
      ]
      snapshot?.history = timeline.map { $0.interlude ? "人物介绍" : $0.speaker + "：" + $0.text }
    }
    private static func syntheticImage(_ id: String) -> Data? {
      let colors: [NSColor] = [.systemPurple, .systemTeal, .systemPink, .systemIndigo]
      let color = colors[id.utf8.reduce(0) { ($0 + Int($1)) % colors.count }]
      let image = NSImage(size: NSSize(width: 384, height: 216), flipped: false) { rect in
        NSColor.controlBackgroundColor.setFill()
        rect.fill()
        color.withAlphaComponent(0.12).setFill()
        rect.fill()
        color.withAlphaComponent(0.22).setFill()
        NSRect(x: 0, y: 0, width: rect.width, height: 56).fill()
        ("合成预览" as NSString).draw(
          at: NSPoint(x: 24, y: 98),
          withAttributes: [
            .font: NSFont.systemFont(ofSize: 24, weight: .medium), .foregroundColor: color,
          ])
        return true
      }
      guard let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff) else {
        return nil
      }
      return bitmap.representation(using: .png, properties: [:])
    }
  }

  // Full CG presentation still uses Metal in production; never fake an actual gameplay render here.
  struct MetalCanvas: View {
    let game: Game
    var body: some View { Text("真实 CG 渲染不在此夹具中运行") }
  }

  @main struct UIPreviewRenderer {
    @MainActor private static var renderCount = 0
    @MainActor static func main() throws {
      let app = NSApplication.shared
      app.setActivationPolicy(.prohibited)
      guard (2...3).contains(CommandLine.arguments.count) else {
        fatalError("Expected output directory and optional private artwork directory")
      }
      let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
      var artwork = OriginalUIArtwork()
      if CommandLine.arguments.count == 3 {
        let directory = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
        artwork = OriginalUIArtwork.load { name in
          let filename =
            name == OriginalUIArtwork.Asset.titleLogo.rawValue
            ? "title-logo.webp" : "title-background.webp"
          return try? Data(contentsOf: directory.appendingPathComponent(filename))
        }
        guard !artwork.hasUnavailableArtwork else { fatalError("Private artwork did not decode") }
        for filename in ["title-logo.webp", "title-background.webp"] {
          let data = try Data(contentsOf: directory.appendingPathComponent(filename))
          guard GameGalleryThumbnails.decode(data) != nil else {
            fatalError("Private title WebP failed the bounded thumbnail codec smoke check")
          }
        }
        print("Private title WebP codec smoke passed; this does not validate real unlocked CGs.")
        print("Explicit private artwork enabled; save and dialogue data remain synthetic.")
      }
      for (name, ready, saved, width, height) in [
        ("title-onboarding-mac", false, false, 1280.0, 758.0),
        ("title-ready-mac", true, true, 1280.0, 758.0),
        ("title-new-mac", true, false, 1280.0, 758.0),
        ("title-ready-compact", true, true, 667.0, 320.0),
        ("title-ready-portrait", true, true, 390.0, 760.0),
        ("title-ready-accessibility", true, true, 667.0, 720.0),
        ("title-onboarding-compact", false, false, 667.0, 320.0),
      ] {
        let game = Game(ready: ready, saved: saved)
        if ready { game.originalUI = artwork }
        let view = GameTitleScreen(
          game: game, startNew: {}, resume: {}, openSaves: {}, openCollection: { _ in },
          openResources: {}, openSettings: {}
        )
        .dynamicTypeSize(name.hasSuffix("accessibility") ? .accessibility2 : .large)
        try render(view, name: name, width: width, height: height, output: output)
      }
      let settings = Game(ready: true, saved: true)
      try render(
        Settings(game: settings), name: "settings-mac", width: 620, height: 660, output: output)
      try render(
        Settings(game: settings), name: "settings-compact", width: 390, height: 660, output: output)
      for (suffix, width, height) in [("mac", 720.0, 620.0), ("compact", 390.0, 640.0)] {
        let library = Game(ready: true, saved: true)
        library.populateLibrary()
        try render(
          SaveManager(game: library, saving: false), name: "saves-read-\(suffix)", width: width,
          height: height, output: output)
        try render(
          SaveManager(game: library, saving: true), name: "saves-write-\(suffix)", width: width,
          height: height, output: output)
        try render(
          Backlog(game: library), name: "backlog-\(suffix)", width: width, height: height,
          output: output)
        let galleryWidth = suffix == "mac" ? 840.0 : width
        try render(
          CollectionMenu(game: library, replay: false), name: "gallery-\(suffix)",
          width: galleryWidth, height: height, output: output)
        try render(
          CollectionMenu(game: library, replay: true), name: "replays-\(suffix)",
          width: galleryWidth, height: height, output: output)
      }
      let empty = Game(ready: true, saved: false)
      try render(
        SaveManager(game: empty, saving: false), name: "saves-empty", width: 390, height: 520,
        output: output)
      try render(
        Backlog(game: empty), name: "backlog-empty", width: 390, height: 520, output: output)
      try render(
        CollectionMenu(game: empty, replay: false), name: "gallery-empty", width: 390, height: 520,
        output: output)
      let locked = Game(ready: true, saved: false)
      locked.gallery = (1...4).map {
        CollectionEntry(id: "locked-\($0)", title: "不得显示的标题", asset: "", unlocked: false)
      }
      try render(
        CollectionMenu(game: locked, replay: false), name: "gallery-locked", width: 390,
        height: 520, output: output)
      let fullHistory = Game(ready: true, saved: true)
      fullHistory.populateLibrary()
      try render(
        Backlog(game: fullHistory, initialFullHistory: true), name: "backlog-text", width: 680,
        height: 620, output: output)
      try render(
        Backlog(game: empty, initialFullHistory: true), name: "backlog-text-empty", width: 390,
        height: 520, output: output)
      fullHistory.thumbnailUnavailable = true
      try render(
        CollectionMenu(game: fullHistory, replay: false), name: "gallery-unavailable", width: 840,
        height: 620, output: output)
      fullHistory.thumbnailUnavailable = false
      try render(
        CollectionMenu(game: fullHistory, replay: false).dynamicTypeSize(.accessibility2),
        name: "gallery-accessibility", width: 390, height: 640, output: output)
      fullHistory.snapshot?.title = true
      try render(
        SaveManager(game: fullHistory, saving: true), name: "saves-disabled", width: 720,
        height: 620, output: output)
      let unavailableSave = Game(ready: true, saved: true)
      unavailableSave.populateLibrary()
      unavailableSave.saveThumbnailUnavailable = true
      try render(
        SaveManager(game: unavailableSave, saving: false), name: "saves-preview-unavailable",
        width: 720, height: 620, output: output)
      unavailableSave.saveThumbnailUnavailable = false
      unavailableSave.slots[0].previewRevision = nil
      try render(
        SaveManager(game: unavailableSave, saving: false), name: "saves-preview-missing-revision",
        width: 390, height: 640, output: output)
      try render(
        SaveManager(game: unavailableSave, saving: false).dynamicTypeSize(.accessibility2),
        name: "saves-accessibility", width: 390, height: 640, output: output)
      for (name, width) in [("game-menu-ipad", 480.0), ("game-menu-compact", 390.0)] {
        try render(
          GameplayMenuSheet(
            muted: false, hasVoice: true, hasLibrary: true, select: { _ in }, close: {}),
          name: name, width: width, height: 620, output: output)
      }
      try render(
        GameplayMenuSheet(
          muted: true, hasVoice: false, hasLibrary: false, select: { _ in }, close: {}),
        name: "game-menu-unavailable", width: 390, height: 620, output: output)
      try render(
        GameplayMenuSheet(
          muted: false, hasVoice: true, hasLibrary: true, select: { _ in }, close: {}
        )
        .dynamicTypeSize(.accessibility2),
        name: "game-menu-accessibility", width: 390, height: 640, output: output)
      for (name, width, height, scale) in [
        ("dialogue-compact", 440.0, 154.0, 1.0),
        ("dialogue-large-text", 440.0, 210.0, 1.6),
      ] {
        let panel = GameReadingPanel(
          speaker: "阅读预览",
          text: "「今天，也想和你多待一会儿。」\n把对白调到最舒服的样子，再一起翻开故事的下一页吧。较长的文字可以上下滚动阅读。",
          page: 128, textScale: scale, backgroundOpacity: 0.9, lineSpacing: 5, showPageNumber: true
        ) {
          Image(systemName: "chevron.down").frame(width: 32, height: 32)
          Button {
          } label: {
            Label("上一句", systemImage: "chevron.left").labelStyle(.iconOnly)
          }.buttonStyle(.bordered).controlSize(.large)
          Button {
          } label: {
            Label("下一句", systemImage: "chevron.right").labelStyle(.iconOnly)
          }.buttonStyle(.bordered).controlSize(.large)
        }
        .frame(width: width, height: height).padding(20)
        .background(GameTheme.background)
        try render(panel, name: name, width: width + 40, height: height + 40, output: output)
      }
      print(
        "Rendered \(renderCount) macOS SwiftUI visual fixtures; library images/data are synthetic; no engine, actual gameplay or iOS UI was exercised."
      )
    }

    @MainActor private static func render<V: View>(
      _ view: V, name: String, width: Double, height: Double, output: URL
    ) throws {
      // ImageRenderer omits platform-backed scroll views. An isolated offscreen
      // NSHostingView exercises the real scroll/layout hierarchy instead.
      let host = NSHostingView(
        rootView: view.frame(width: width, height: height).preferredColorScheme(.light)
          .environment(\.controlActiveState, .active))
      host.frame = NSRect(x: 0, y: 0, width: width, height: height)
      let window = NSWindow(
        contentRect: host.frame, styleMask: .borderless, backing: .buffered, defer: false)
      window.isReleasedWhenClosed = false
      window.contentView = host
      window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
      window.orderFront(nil)
      defer { window.close() }
      host.layoutSubtreeIfNeeded()
      RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.15))
      host.layoutSubtreeIfNeeded()
      host.displayIfNeeded()
      guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
        fatalError("Failed to allocate preview for \(name)")
      }
      host.cacheDisplay(in: host.bounds, to: bitmap)
      guard let data = bitmap.representation(using: .png, properties: [:]) else {
        fatalError("Failed to render \(name)")
      }
      try data.write(to: output.appendingPathComponent("\(name).png"))
      renderCount += 1
    }
  }
#endif
