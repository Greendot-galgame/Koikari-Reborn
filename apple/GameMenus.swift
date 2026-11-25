import SwiftUI

private struct LibrarySurface: ViewModifier {
  var width: CGFloat = 680
  func body(content: Self.Content) -> some View {
    content
      #if os(macOS)
        // macOS sheets size to the content's fitting size. A flexible frame
        // with only a small minimum collapsed to a ~470pt panel, so pin the
        // intended library size: the content is then laid out for exactly the
        // panel it is drawn in. An unbounded height laid the lists out for a
        // taller area than the sheet clips, so their last row sat below the
        // visible edge and the panel looked like it started from the corner.
        .frame(width: width, height: 620)
      #else
        .frame(minWidth: 300, idealWidth: width, minHeight: 280, idealHeight: 620)
      #endif
      .background(GameTheme.background).foregroundStyle(GameTheme.text)
      .tint(GameTheme.accent).preferredColorScheme(.light)
  }
}

private struct LibraryEmptyState: View {
  let title: String
  let symbol: String
  let detail: String
  var body: some View {
    VStack(spacing: 10) {
      Image(systemName: symbol).font(.title2).foregroundStyle(.secondary).accessibilityHidden(true)
      Text(title).font(.headline)
      Text(detail).font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
    }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity)
  }
}

struct GameDocumentSheet: View {
  let title: String
  let text: String
  let close: () -> Void
  var body: some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: title, closeIdentifier: "close-document", close: close)
      ScrollView {
        Text(text).font(.callout).lineSpacing(6).textSelection(.enabled)
          .frame(maxWidth: .infinity, alignment: .leading).padding(20)
      }
    }.modifier(LibrarySurface())
  }
}

/// A stable native sheet, independent of the rapidly refreshed story snapshot.
/// Do not inherit the background toolbar's modal-disabled state here.
struct GameplayMenuSheet: View {
  let muted: Bool
  let hasVoice: Bool
  let hasLibrary: Bool
  let select: (GameplayMenuAction) -> Void
  let close: () -> Void

  var body: some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: "游戏菜单", closeIdentifier: "close-game-menu", close: close)
      Form {
        Section("进度") {
          entry(.save, "存档", "square.and.arrow.down").keyboardShortcut("s", modifiers: .command)
          entry(.load, "读档", "tray.and.arrow.up")
        }
        Section("声音与显示") {
          entry(.autoPlay, "自动播放", "play.circle")
          entry(.replayVoice, "重播语音", "speaker.wave.2").disabled(!hasVoice)
          entry(.toggleMute, muted ? "取消静音" : "静音", muted ? "speaker.wave.2" : "speaker.slash")
          entry(.settings, "设置", "slider.horizontal.3")
        }
        Section("其他") {
          entry(.storyTree, "剧情树", "list.bullet.indent").disabled(!hasLibrary)
          entry(.gallery, "CG 鉴赏", "photo.on.rectangle").disabled(!hasLibrary)
          entry(.replay, "场景回想", "play.rectangle").disabled(!hasLibrary)
          entry(.diagnostics, "兼容状态", "info.circle")
          entry(.returnToTitle, "返回标题", "house")
        }
      }.formStyle(.grouped)
    }.modifier(LibrarySurface(width: 480))
  }

  private func entry(_ action: GameplayMenuAction, _ title: String, _ symbol: String) -> some View {
    Button {
      select(action)
    } label: {
      Label(title, systemImage: symbol)
        .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
        .contentShape(Rectangle())
    }.accessibilityIdentifier("game-menu-\(action.rawValue)")
  }
}

struct CollectionMenu: View {
  @ObservedObject var game: Game
  let replay: Bool
  @Environment(\.dismiss) private var dismiss
  @Environment(\.dynamicTypeSize) private var typeSize
  @State private var selected: CollectionEntry?
  @State private var confirmReplay = false
  @State private var unlockedOnly = false
  @State private var showsLarge = false
  @State private var fullImage: Image?
  @State private var fullImageLoading = false
  @State private var fullImageFailed = false
  private var entries: [CollectionEntry] { replay ? game.replays : game.gallery }
  private var visibleEntries: [CollectionEntry] { entries.filter { !unlockedOnly || $0.unlocked } }
  private var columns: [GridItem] {
    typeSize.isAccessibilitySize
      ? [GridItem(.flexible())] : [GridItem(.adaptive(minimum: 160), spacing: 16)]
  }

  var body: some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: replay ? "场景回想" : "CG 鉴赏", closeIdentifier: "close-collection") {
        game.hideCG()
        dismiss()
      }
      if let selected, !replay {
        VStack(alignment: .leading, spacing: 14) {
          HStack {
            Button {
              game.hideCG()
              self.selected = nil
            } label: {
              Label("返回列表", systemImage: "chevron.left")
            }.buttonStyle(.bordered).accessibilityIdentifier("gallery-back")
            Spacer()
            Text(selected.title).font(.callout).foregroundStyle(.secondary).lineLimit(1)
            Button(action: openLargePreview) {
              Label("查看大图", systemImage: "arrow.up.left.and.arrow.down.right")
            }
            .buttonStyle(.borderedProminent).accessibilityIdentifier("cg-view-large")
          }
          MetalCanvas(game: game).aspectRatio(16 / 9, contentMode: .fit)
            .accessibilityIdentifier("gallery-preview")
            .contentShape(Rectangle())
            .onTapGesture(count: 2) { openLargePreview() }
            .accessibilityHint("双击可查看大图")
          Text("双击画面或点按「查看大图」可放大欣赏，并支持双指缩放。")
            .font(.caption).foregroundStyle(.secondary)
          Spacer(minLength: 0)
        }.padding(20)
      } else {
        HStack(spacing: 16) {
          Picker("显示条目", selection: $unlockedOnly) {
            Text("全部").tag(false)
            Text("已解锁").tag(true)
          }.pickerStyle(.segmented).frame(maxWidth: 240, alignment: .leading).labelsHidden()
            .accessibilityIdentifier("collection-filter")
          Spacer(minLength: 0)
          Text("\(entries.filter { $0.unlocked }.count) / \(entries.count)")
            .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            .accessibilityLabel("已解锁 \(entries.filter { $0.unlocked }.count)，共 \(entries.count) 项")
        }.padding(20)
        if visibleEntries.isEmpty {
          LibraryEmptyState(
            title: unlockedOnly ? "暂无已解锁内容" : "暂无内容",
            symbol: replay ? "play.rectangle" : "photo.on.rectangle",
            detail: "随着剧情推进，解锁的内容会出现在这里。")
        } else {
          ScrollView {
            LazyVGrid(columns: columns, spacing: 16) {
              ForEach(visibleEntries) { entry in
                CollectionTile(game: game, entry: entry, replay: replay) {
                  if replay {
                    selected = entry
                    confirmReplay = true
                  } else if game.showCG(entry) {
                    selected = entry
                    fullImage = nil
                    fullImageFailed = false
                    fullImageLoading = false
                  }
                }
              }
            }.padding(.horizontal, 20).padding(.bottom, 20)
          }
        }
      }
    }
    .modifier(LibrarySurface(width: 840))
    .onDisappear {
      game.hideCG()
      game.clearGalleryThumbnails()
      fullImage = nil
    }
    .onChange(of: showsLarge) { presented in
      if !presented {
        fullImage = nil
        fullImageLoading = false
        fullImageFailed = false
      }
    }
    #if os(iOS)
      .fullScreenCover(isPresented: $showsLarge) { largePreview }
    #else
      .sheet(isPresented: $showsLarge) {
        largePreview.frame(minWidth: 880, idealWidth: 1180, minHeight: 560, idealHeight: 760)
      }
    #endif
    .confirmationDialog(
      "开始回想「\(selected?.title ?? "")」？当前自动进度会更新，手动存档保留。", isPresented: $confirmReplay,
      titleVisibility: .visible
    ) {
      Button("开始回想") {
        if let selected, game.action("replay:\(selected.id)") { dismiss() }
      }
      Button("取消", role: .cancel) { selected = nil }
    }
  }

  private var largePreview: some View {
    CGViewer(
      title: selected?.title ?? "", image: fullImage, loading: fullImageLoading,
      failed: fullImageFailed, close: { showsLarge = false })
  }

  private func openLargePreview() {
    guard let selected else { return }
    fullImage = nil
    fullImageFailed = false
    fullImageLoading = true
    showsLarge = true
    Task { @MainActor in
      let image = await game.galleryFullImage(selected)
      guard !Task.isCancelled, showsLarge else { return }
      fullImage = image
      fullImageFailed = image == nil
      fullImageLoading = false
    }
  }
}

/// Full-window CG viewer. Pinch (or trackpad magnification) zooms, dragging pans,
/// and a double-click returns to the fitted size. The decoded image is bounded
/// by the same loader as thumbnails, just at a larger (not original) pixel size.
struct CGViewer: View {
  let title: String
  let image: Image?
  let loading: Bool
  let failed: Bool
  let close: () -> Void
  @State private var scale: CGFloat = 1
  @State private var settledScale: CGFloat = 1
  @State private var offset: CGSize = .zero
  @State private var settledOffset: CGSize = .zero

  var body: some View {
    ZStack {
      Color.black.ignoresSafeArea()
      if let image {
        GeometryReader { proxy in
          image.resizable().scaledToFit()
            .frame(width: proxy.size.width, height: proxy.size.height)
            .scaleEffect(scale, anchor: .center)
            .offset(offset)
            .contentShape(Rectangle())
            // Zoom and pan run simultaneously: a pinch already moves the finger
            // centroid, and a pan that starts mid-pinch must not restart the
            // magnification gesture or drop its accumulated scale — that
            // restart is what read as stutter on iOS.
            .simultaneousGesture(
              MagnificationGesture()
                .onChanged { value in
                  scale = min(4, max(1, settledScale * value))
                }
                .onEnded { _ in
                  settledScale = scale
                  if scale <= 1.001 {
                    scale = 1
                    settledScale = 1
                    offset = .zero
                    settledOffset = .zero
                  }
                }
            )
            .simultaneousGesture(
              DragGesture()
                .onChanged { value in
                  guard scale > 1 else { return }
                  offset = CGSize(
                    width: settledOffset.width + value.translation.width,
                    height: settledOffset.height + value.translation.height)
                }
                .onEnded { _ in
                  settledOffset = offset
                  if scale <= 1.001 {
                    offset = .zero
                    settledOffset = .zero
                  }
                }
            )
            .onTapGesture(count: 2) { resetZoom() }
        }
        .accessibilityHidden(true)
      } else if loading {
        ProgressView("正在载入大图…").tint(.white).foregroundStyle(.white)
      } else if failed {
        VStack(spacing: 10) {
          Image(systemName: "photo.badge.exclamationmark").font(.largeTitle)
          Text("大图暂不可用，仍可在预览中查看。").font(.callout)
        }.foregroundStyle(.white.opacity(0.8))
      }
      VStack(spacing: 0) {
        HStack(spacing: 12) {
          Text(title).font(.headline).foregroundStyle(.white).lineLimit(1)
          Spacer(minLength: 8)
          Button(action: resetZoom) {
            Label("适应窗口", systemImage: "arrow.down.right.and.arrow.up.left")
              .font(.callout.weight(.medium))
              .padding(.horizontal, 12).padding(.vertical, 7)
              .background(.white.opacity(0.14), in: Capsule())
              .foregroundStyle(.white)
          }
          .buttonStyle(.plain)
          .accessibilityIdentifier("cg-viewer-fit")
          Button(action: close) {
            Label("关闭", systemImage: "xmark")
              .font(.callout.weight(.semibold))
              .padding(.horizontal, 14).padding(.vertical, 7)
              .background(.white, in: Capsule())
              .foregroundStyle(.black)
          }
          .buttonStyle(.plain)
          .accessibilityIdentifier("cg-viewer-close")
        }
        .padding(.horizontal, 18).padding(.vertical, 12)
        .background(.black.opacity(0.35))
        Spacer()
      }
    }
    .preferredColorScheme(.dark)
  }

  private func resetZoom() {
    withAnimation(.easeOut(duration: 0.18)) {
      scale = 1
      settledScale = 1
      offset = .zero
      settledOffset = .zero
    }
  }
}

private struct CollectionTile: View {
  @ObservedObject var game: Game
  let entry: CollectionEntry
  @Environment(\.dynamicTypeSize) private var typeSize
  let replay: Bool
  let open: () -> Void
  @State private var thumbnail: Image?
  @State private var unavailable = false
  @State private var thumbnailGeneration: UUID?
  private var identity: String { "\(game.collectionGeneration):\(entry.id):\(entry.unlocked)" }

  var body: some View {
    Button(action: open) {
      VStack(alignment: .leading, spacing: 10) {
        ZStack(alignment: .bottomTrailing) {
          Rectangle().fill(GameTheme.surface)
          if !entry.unlocked {
            Image(systemName: "lock").font(.title2).foregroundStyle(GameTheme.text)
          } else if let thumbnail, thumbnailGeneration == game.collectionGeneration {
            thumbnail.resizable().scaledToFit()
          } else if replay && entry.preview == nil {
            VStack(spacing: 6) {
              Image(systemName: "play.rectangle").font(.largeTitle)
              Text("暂无预览").font(.caption)
            }.foregroundStyle(GameTheme.accent)
          } else if unavailable, thumbnailGeneration == game.collectionGeneration {
            VStack(spacing: 6) {
              Image(systemName: "photo").font(.title2)
              Text("预览暂不可用").font(.caption)
            }.foregroundStyle(.secondary)
          } else {
            ProgressView().controlSize(.small)
          }
          if replay && entry.unlocked {
            Image(systemName: "play.fill")
              .font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
              .padding(7).background(.black.opacity(0.45), in: Circle())
              .padding(8).accessibilityHidden(true)
          }
        }
        .aspectRatio(16 / 9, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .accessibilityHidden(true)
        Text(entry.unlocked ? entry.title : "尚未解锁")
          .font(.callout.weight(.medium)).foregroundStyle(
            entry.unlocked ? GameTheme.text : .secondary
          )
          .multilineTextAlignment(.leading).lineLimit(typeSize.isAccessibilitySize ? nil : 2)
          .frame(maxWidth: .infinity, minHeight: 36, alignment: .topLeading)
      }
      .padding(10).contentShape(Rectangle())
    }
    .buttonStyle(.bordered).buttonBorderShape(.roundedRectangle).disabled(!entry.unlocked)
    .accessibilityIdentifier("\(replay ? "replay" : "cg")-\(entry.id)")
    .accessibilityLabel(entry.unlocked ? entry.title : "尚未解锁")
    .task(id: identity) {
      thumbnail = nil
      unavailable = false
      thumbnailGeneration = nil
      let generation = game.collectionGeneration
      guard entry.unlocked else { return }
      if replay && entry.preview == nil {
        unavailable = true
        thumbnailGeneration = generation
        return
      }
      let result =
        replay ? await game.replayThumbnail(entry) : await game.galleryThumbnail(entry)
      guard !Task.isCancelled, generation == game.collectionGeneration else { return }
      thumbnailGeneration = generation
      thumbnail = result
      unavailable = result == nil
    }
    .onDisappear {
      thumbnail = nil
      thumbnailGeneration = nil
      unavailable = false
    }
  }
}

/// Opens the history on the newest page. macOS 14 / iOS 17 can anchor the
/// scroll view itself, which works on the first layout pass; older releases
/// keep the best-effort scroll issued from onAppear below.
private struct BacklogScrollAnchor: ViewModifier {
  let anchoredToBottom: Bool
  @ViewBuilder func body(content: Self.Content) -> some View {
    if #available(iOS 17.0, macOS 14.0, *) {
      content.defaultScrollAnchor(anchoredToBottom ? .bottom : .top)
    } else {
      content
    }
  }
}

struct Backlog: View {
  @ObservedObject var game: Game
  @Environment(\.dismiss) private var dismiss
  @State private var selected: PastPage?
  @State private var confirmRollback = false
  @State private var fullHistory = false

  init(game: Game, initialFullHistory: Bool = false) {
    self.game = game
    _fullHistory = State(initialValue: initialFullHistory)
  }
  private var historyText: String {
    let text = game.snapshot?.history.joined(separator: "\n\n") ?? ""
    return text.isEmpty ? "暂无对白记录。" : text
  }

  var body: some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: "剧情回看", closeIdentifier: "close-backlog") { dismiss() }
      Picker("回看模式", selection: $fullHistory) {
        Text("可回溯记录").tag(false)
        Text("全部文字").tag(true)
      }.pickerStyle(.segmented).labelsHidden().padding(20)
        .accessibilityIdentifier("backlog-mode")
      if fullHistory {
        ScrollView {
          Text(historyText)
            .font(.callout).lineSpacing(6).textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 20).padding(
              .bottom, 20)
        }
      } else if game.timeline.isEmpty {
        LibraryEmptyState(title: "暂无对白记录", symbol: "text.bubble", detail: "继续阅读后，可在这里回看和回溯对白。")
      } else {
        ScrollViewReader { proxy in
          ScrollView {
            // A plain stack, not a lazy one: the timeline is capped at the 100
            // stored pages, and a lazy stack has no rows laid out when onAppear
            // fires, so scrolling to the newest page landed on a guessed offset
            // and left the panel blank until the user dragged it.
            VStack(spacing: 12) {
              ForEach(game.timeline) { page in
                Button {
                  selected = page
                  confirmRollback = true
                } label: {
                  GroupBox {
                    VStack(alignment: .leading, spacing: 8) {
                      HStack(alignment: .firstTextBaseline) {
                        if page.interlude || !page.speaker.isEmpty {
                          Text(page.interlude ? "人物介绍" : page.speaker)
                            .font(.callout.weight(.semibold))
                        }
                        Spacer()
                        Text("第 \(page.page) 页").font(.caption.monospacedDigit()).foregroundStyle(
                          .secondary)
                        Image(systemName: "arrow.uturn.backward").font(.caption).foregroundStyle(
                          .secondary)
                      }
                      if page.interlude || !page.text.isEmpty {
                        Text(page.interlude ? "回到人物介绍画面" : page.text).font(.body).lineSpacing(4)
                          .multilineTextAlignment(.leading)
                          .fixedSize(horizontal: false, vertical: true)
                      }
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(4)
                  }.contentShape(Rectangle())
                }.buttonStyle(.plain).id(page.page)
                  .accessibilityIdentifier("rewind-\(page.page)")
                  .accessibilityHint("回溯到这一页；操作前会再次确认")
              }
            }
            // Extra bottom room: the sheet clips slightly more than it lays
            // out, so without it the newest row ended up half hidden under the
            // panel's bottom edge even with the list scrolled all the way down.
            .padding(.horizontal, 20).padding(.bottom, 52)
          }
          .modifier(BacklogScrollAnchor(anchoredToBottom: game.timeline.count > 3))
          .background {
            // A sheet settles on its final size only after the first layout
            // pass, and a jump issued before that is dropped — which is why the
            // panel kept opening on the oldest page. Watch the viewport instead
            // and scroll once it has a real height.
            GeometryReader { viewport in
              Color.clear.onChange(of: viewport.size) { size in
                guard size.height > 1, let last = game.timeline.last else { return }
                proxy.scrollTo(last.page, anchor: .bottom)
              }
            }
          }
        }
      }
    }
    .modifier(LibrarySurface())
    .confirmationDialog(
      "回到第 \(selected?.page ?? 0) 页？自动进度将更新，手动存档不变。",
      isPresented: $confirmRollback, titleVisibility: .visible
    ) {
      Button("回到这一页") {
        if let selected, game.action("rewind:\(selected.page)") { dismiss() }
      }
      Button("取消", role: .cancel) { selected = nil }
    }
  }
}

struct SaveManager: View {
  @ObservedObject var game: Game
  @Environment(\.scenePhase) private var phase
  @Environment(\.dynamicTypeSize) private var typeSize
  let saving: Bool
  @Environment(\.dismiss) private var dismiss
  @State private var pending: SaveSlot?
  @State private var confirm = false
  @State private var exportingBackup = false
  @State private var exportingDocument = SaveBackupDocument()
  @State private var importingBackup = false
  @State private var importingReport: String?
  @State private var confirmImport = false
  @State private var pendingImportFile: URL?
  /// Save-page tab: 0 is the common route (共通线), each further index a heroine
  /// route. Slots are grouped by the route recorded inside each save file.
  @State private var selectedPage = 0
  private var automatic: [SaveSlot] { game.slots.filter { $0.slot == 9 } }
  private var manual: [SaveSlot] { game.slots.filter { $0.slot != 9 }.sorted { $0.slot < $1.slot } }
  private var canSave: Bool {
    !(game.snapshot?.title ?? true) && game.snapshot?.blocked == nil
      && game.snapshot?.flow?.replay != true
  }
  /// Route the story is currently inside: nil on the common route reads as
  /// page 0, heroine routes carry their own index. A new manual save may only
  /// ever land on this page, so saving never scatters onto another line.
  private var currentRoute: Int { max(0, game.snapshot?.flow?.route ?? 0) }
  /// One page per route: index 0 is always the common route. Beyond the
  /// declared routes, any slot that recorded an out-of-range route still gets a
  /// page so its save stays reachable instead of disappearing.
  private var pageCount: Int {
    let declared = max(1, game.routeNames.count)
    let highest = manual.map(\.page).max() ?? 0
    return max(declared, highest + 1)
  }
  private var pagedManual: [SaveSlot] {
    manual.filter { $0.page == selectedPage }
  }

  var body: some View {
    GeometryReader { proxy in page(width: proxy.size.width) }
      .modifier(LibrarySurface(width: 720))
      .onAppear {
        game.beginSavePreviews()
        // When saving, open on the page that matches the route being played so
        // a heroine-route save lands on its own page by default.
        if saving { selectedPage = currentRoute }
      }
      .onChange(of: currentRoute) { route in
        if saving { selectedPage = route }
      }
      .onDisappear { game.endSavePreviews() }
      .onChange(of: phase) { if $0 == .active { game.reloadSaves() } }
      .confirmationDialog(
        saving ? "覆盖 \(pending?.name ?? "存档")？" : "读取 \(pending?.name ?? "存档")，将替换当前进度？",
        isPresented: $confirm, titleVisibility: .visible
      ) {
        Button(saving ? "覆盖保存" : "读取", role: saving ? .destructive : nil) {
          if let pending {
            let command = "\(saving ? "save" : "load"):\(pending.slot)"
            if game.action(command) && !saving { dismiss() }
          }
        }
        Button("取消", role: .cancel) {}
      }
      .confirmationDialog(
        "导入将写入存档目录，同名槽位会被覆盖；不兼容资源的档仍会保留原样。确定继续？",
        isPresented: $confirmImport, titleVisibility: .visible
      ) {
        Button("导入", role: .destructive) {
          if let url = pendingImportFile { runImport(url) }
        }
        Button("取消", role: .cancel) { pendingImportFile = nil }
      }
      .fileExporter(
        isPresented: $exportingBackup, document: exportingDocument,
        contentType: SaveBackup.utType,
        defaultFilename: "koikari-backup"
      ) { result in
        switch result {
        case .success(let url):
          game.inform("存档备份已导出：\(url.lastPathComponent)")
        case .failure(let error):
          game.inform("导出失败：\(error.localizedDescription)")
        }
      }
      .fileImporter(
        isPresented: $importingBackup, allowedContentTypes: [SaveBackup.utType]
      ) { result in
        switch result {
        case .success(let url):
          pendingImportFile = url
          confirmImport = true
        case .failure(let error):
          game.inform("打开文件失败：\(error.localizedDescription)")
        }
      }
      .alert("导入结果", isPresented: .constant(importingReport != nil)) {
        Button("好") { importingReport = nil }
      } message: {
        Text(importingReport ?? "")
      }
  }

  private func runImport(_ url: URL) {
    let accessed = url.startAccessingSecurityScopedResource()
    defer { if accessed { url.stopAccessingSecurityScopedResource() } }
    do {
      let written = try SaveBackup.import(from: url, to: game.savesDirectory)
      game.reloadSaves()
      importingReport = "已导入 \(written.count) 个存档文件：\(written.joined(separator: "、"))。"
    } catch {
      importingReport = "导入失败：\(error.localizedDescription)"
    }
    pendingImportFile = nil
    confirmImport = false
  }

  private func page(width: CGFloat) -> some View {
    let wide = width >= 600 && !typeSize.isAccessibilitySize
    return VStack(spacing: 0) {
      GameSheetHeader(title: saving ? "保存进度" : "读取存档", closeIdentifier: "close-saves") { dismiss() }
      HStack(spacing: 10) {
        Button {
          exportingDocument = SaveBackupDocument(source: game.savesDirectory)
          exportingBackup = true
        } label: {
          Label("导出存档备份", systemImage: "square.and.arrow.up")
        }
        .buttonStyle(.bordered).controlSize(.regular)
        .disabled(!game.slots.contains(where: { $0.exists }))
        .accessibilityIdentifier("save-export")

        Button {
          importingBackup = true
        } label: {
          Label("导入存档备份", systemImage: "square.and.arrow.down")
        }
        .buttonStyle(.bordered).controlSize(.regular)
        .accessibilityIdentifier("save-import")

        Spacer()
      }
      .padding(.horizontal, 20).padding(.top, 14).padding(.bottom, 4)
      if game.slots.isEmpty {
        LibraryEmptyState(title: "暂无存档", symbol: "tray", detail: "游戏中的进度可以保存到手动槽位。")
      } else {
        ScrollView {
          VStack(alignment: .leading, spacing: 20) {
            if let notice = game.notice {
              Text(notice).font(.callout).foregroundStyle(GameTheme.success)
                .accessibilityIdentifier("save-notice")
            }
            if !automatic.isEmpty {
              VStack(alignment: .leading, spacing: 10) {
                Text("自动进度").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                ForEach(automatic) { slot in card(slot, width: width) }
              }
            }
            if saving || !manual.isEmpty {
              VStack(alignment: .leading, spacing: 10) {
                Text("手动存档").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                if pageCount > 1 { routeStrip }
                if pagedManual.isEmpty {
                  Text("这一页还没有存档。").font(.callout).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center).padding(.vertical, 24)
                } else if wide {
                  VStack(spacing: 12) {
                    ForEach(pagedManual) { slot in card(slot, width: width) }
                  }
                } else {
                  LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 260), spacing: 12, alignment: .top)],
                    spacing: 12
                  ) {
                    ForEach(pagedManual) { slot in card(slot, width: width) }
                  }
                }
              }
            }
          }.padding(20)
        }
      }
    }
  }

  /// Route tabs scroll sideways instead of squeezing every route into one
  /// segmented control, which clipped labels once a game has many routes.
  private var routeStrip: some View {
    VStack(alignment: .leading, spacing: 4) {
      ScrollView(.horizontal, showsIndicators: false) {
        HStack(spacing: 8) {
          ForEach(0..<pageCount, id: \.self) { index in
            let selected = index == selectedPage
            Button {
              selectedPage = index
            } label: {
              Text(game.routeTitle(index))
                .font(.callout.weight(selected ? .semibold : .regular))
                .lineLimit(1)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(selected ? GameTheme.accent : GameTheme.surface, in: Capsule())
                .overlay {
                  Capsule().strokeBorder(
                    selected ? Color.clear : GameTheme.border, lineWidth: 1
                  )
                  .allowsHitTesting(false)
                }
                .foregroundStyle(selected ? Color.white : GameTheme.text)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("save-page-\(index)")
            .accessibilityAddTraits(selected ? .isSelected : [])
          }
        }
        .padding(.vertical, 2)
      }
      .accessibilityIdentifier("save-page-picker")
      // While saving, only the current route's page may receive the save, so
      // retarget the selection instead of letting it drift onto another line.
      .onChange(of: selectedPage) { page in
        if saving, page != currentRoute { selectedPage = currentRoute }
      }
      if saving {
        Text("当前正在\(game.routeTitle(currentRoute))，新存档只能保存在这一页。")
          .font(.caption).foregroundStyle(.secondary)
      }
    }
  }

  private func card(_ slot: SaveSlot, width: CGFloat) -> some View {
    GroupBox {
      if width >= 600 && !typeSize.isAccessibilitySize {
        HStack(alignment: .top, spacing: 16) {
          SaveSlotThumbnail(game: game, slot: slot).frame(width: 224)
          information(slot).frame(maxWidth: .infinity)
        }.padding(4)
      } else {
        verticalCard(slot).padding(4)
      }
    }.frame(maxWidth: .infinity, alignment: .topLeading)
  }

  private func verticalCard(_ slot: SaveSlot) -> some View {
    VStack(alignment: .leading, spacing: 12) {
      SaveSlotThumbnail(game: game, slot: slot)
      information(slot)
    }
  }

  private func information(_ slot: SaveSlot) -> some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack {
        Label(
          slot.slot == 9 ? "自动保存" : "槽位 \(slot.slot)",
          systemImage: slot.slot == 9 ? "clock.arrow.circlepath" : "doc"
        )
        .font(.callout.weight(.semibold))
        Spacer(minLength: 4)
        if slot.exists && !slot.compatible {
          Image(systemName: "exclamationmark.triangle").foregroundStyle(GameTheme.warning)
        }
      }
      VStack(alignment: .leading, spacing: 8) {
        if slot.compatible {
          Text(
            slot.text.isEmpty
              ? "已保存的游戏进度" : (slot.speaker.isEmpty ? "" : "\(slot.speaker)：") + slot.text
          )
          .font(.callout).lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
          if slot.savedAt > 0 {
            Text(
              Date(timeIntervalSince1970: Double(slot.savedAt) / 1000)
                .formatted(date: .abbreviated, time: .shortened)
            )
            .font(.caption).foregroundStyle(.secondary)
          }
        } else {
          Text(slot.exists ? "无法读取此存档" : "空槽位").font(.callout).foregroundStyle(.secondary)
          if let error = slot.error {
            Text(error).font(.caption).foregroundStyle(GameTheme.warning)
              .fixedSize(horizontal: false, vertical: true)
          }
        }
      }.frame(minHeight: slot.slot == 9 ? 0 : 54, alignment: .topLeading)
      HStack {
        if slot.compatible {
          Text("第 \(slot.pages) 页").font(.caption).foregroundStyle(.secondary)
        }
        Spacer()
        if saving && slot.slot != 9 && slot.page == currentRoute {
          Button("保存") {
            if slot.exists {
              pending = slot
              confirm = true
            } else {
              game.action("save:\(slot.slot)")
            }
          }.disabled(!canSave).accessibilityLabel("保存至\(slot.name)").accessibilityIdentifier(
            "save-slot-\(slot.slot)")
        } else if !saving {
          Button("读取") {
            pending = slot
            confirm = true
          }
          .disabled(!slot.compatible).accessibilityLabel("读取\(slot.name)")
          .accessibilityIdentifier("load-slot-\(slot.slot)")
        }
      }.buttonStyle(.bordered).controlSize(.regular)
    }.frame(maxWidth: .infinity, alignment: .leading)
  }
}

/// Opens a scrollable canvas horizontally centred and at its top, so the story
/// tree is in view the moment the panel appears instead of leaving the viewport
/// parked on an empty corner of a wide canvas.
private struct CenteredCanvasAnchor: ViewModifier {
  @ViewBuilder func body(content: Self.Content) -> some View {
    if #available(iOS 17.0, macOS 14.0, *) {
      content.defaultScrollAnchor(UnitPoint(x: 0.5, y: 0))
    } else {
      content
    }
  }
}

private struct SaveSlotThumbnail: View {
  @ObservedObject var game: Game
  let slot: SaveSlot
  @State private var image: Image?
  @State private var loadedIdentity: String?
  @State private var unavailable = false
  private var identity: String {
    "\(game.savePreviewGeneration):\(slot.slot):\(slot.exists):\(slot.compatible):\(slot.previewRevision ?? "none")"
  }

  var body: some View {
    ZStack {
      GameTheme.surface
      if slot.exists && slot.compatible, let image, loadedIdentity == identity {
        image.resizable().scaledToFit()
      } else if !slot.exists {
        Image(systemName: "photo").font(.title2).foregroundStyle(.tertiary)
      } else if !slot.compatible || slot.previewRevision == nil
        || (unavailable && loadedIdentity == identity)
      {
        VStack(spacing: 6) {
          Image(systemName: "photo").font(.title2)
          Text("暂无预览").font(.caption)
        }.foregroundStyle(GameTheme.secondaryText)
      } else {
        ProgressView().controlSize(.small)
      }
    }
    .aspectRatio(16 / 9, contentMode: .fit)
    .clipShape(RoundedRectangle(cornerRadius: 8))
    .overlay {
      RoundedRectangle(cornerRadius: 8).strokeBorder(GameTheme.border, lineWidth: 1)
        .allowsHitTesting(false)
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel("\(slot.name)的画面预览")
    .accessibilityValue(image != nil && loadedIdentity == identity ? "已显示" : "暂无预览")
    .accessibilityIdentifier("save-preview-\(slot.slot)")
    .task(id: identity) {
      image = nil
      loadedIdentity = nil
      unavailable = false
      guard slot.exists, slot.compatible, slot.previewRevision != nil else { return }
      let requestedIdentity = identity
      let result = await game.saveThumbnail(slot)
      guard !Task.isCancelled, requestedIdentity == identity else { return }
      loadedIdentity = requestedIdentity
      image = result
      unavailable = result == nil
    }
    .onDisappear {
      image = nil
      loadedIdentity = nil
      unavailable = false
    }
  }
}

/// Chapter/branch map backed by the engine's flow-tree projection. Nodes are
/// laid out as a real branching tree on a canvas that scrolls both axes and
/// pinch-zooms; tapping an unlocked node or a walked branch jumps to it.
/// Jumping autosaves (slot 9) then restarts a clean non-replay run of the target.
struct StoryTreeSheet: View {
  @ObservedObject var game: Game
  @Environment(\.dismiss) private var dismiss
  @State private var pending: PendingJump?
  @State private var confirm = false
  @State private var zoom: CGFloat = 1
  @GestureState private var pinch: CGFloat = 1

  private enum PendingJump {
    case event(Int)
    case choice(selection: Int, choice: Int)
    var title: String {
      switch self {
      case .event: return "这一章节"
      case .choice: return "这个分支"
      }
    }
  }

  /// Card metrics in points; the canvas size derives from column/depth counts.
  private let nodeWidth: CGFloat = 170
  private let nodeHeight: CGFloat = 44
  private let columnWidth: CGFloat = 210
  private let rowHeight: CGFloat = 82
  private let headerHeight: CGFloat = 40
  private let canvasInset: CGFloat = 24

  private struct TreePlan {
    struct Link: Identifiable {
      var from: CGPoint
      var to: CGPoint
      var edge: FlowEdge
      var id: String { edge.id + "@" + String(Int(from.y)) }
    }
    struct Header: Identifiable {
      var name: String
      var at: CGPoint
      var id: String { name }
    }
    var positions: [Int: CGPoint] = [:]
    var links: [Link] = []
    var headers: [Header] = []
    var size: CGSize = .zero
  }

  private var tree: FlowTree? { game.flowTree }

  var body: some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: "剧情树", closeIdentifier: "close-story-tree") { dismiss() }
      if let tree, !tree.nodes.isEmpty {
        let plan = layout(tree)
        let scale = zoom * pinch
        ZStack(alignment: .bottomTrailing) {
          ScrollView([.horizontal, .vertical], showsIndicators: true) {
            ZStack {
              edgeCanvas(plan)
              ForEach(plan.headers) { header in
                Text(header.name)
                  .font(.callout.weight(.semibold))
                  .foregroundStyle(GameTheme.accent)
                  .padding(.horizontal, 12).padding(.vertical, 5)
                  .background(GameTheme.surface, in: Capsule())
                  .overlay {
                    Capsule().strokeBorder(GameTheme.accent.opacity(0.4))
                      .allowsHitTesting(false)
                  }
                  .position(header.at)
              }
              ForEach(tree.nodes) { node in
                if let point = plan.positions[node.event] {
                  nodeCard(node).position(point)
                }
              }
              ForEach(plan.links) { link in
                if let label = link.edge.label {
                  branchChip(link, label: label)
                    .position(chipPoint(link))
                }
              }
            }
            .frame(width: plan.size.width, height: plan.size.height)
            .scaleEffect(scale, anchor: .topLeading)
            .frame(
              width: plan.size.width * scale,
              height: plan.size.height * scale,
              alignment: .topLeading
            )
            .gesture(
              MagnificationGesture()
                .updating($pinch) { value, state, _ in state = value }
                .onEnded { zoom = min(2.5, max(0.4, zoom * $0)) })
          }
          .modifier(CenteredCanvasAnchor())
          .background(GameTheme.background.opacity(0.4))
          zoomBar.padding(12)
        }
      } else {
        LibraryEmptyState(
          title: "暂无剧情节点", symbol: "list.bullet.indent",
          detail: "开始游玩后，章节与分支会按进度解锁显示在这里。")
      }
    }
    .modifier(LibrarySurface(width: 780))
    .confirmationDialog(
      "跳转到\(pending?.title ?? "")？当前自动进度会更新，手动存档保留。",
      isPresented: $confirm, titleVisibility: .visible
    ) {
      Button("跳转") {
        guard let pending else { return }
        switch pending {
        case .event(let event): game.jumpToEvent(event)
        case .choice(let selection, let choice):
          game.jumpToChoice(selection: selection, choice: choice)
        }
        dismiss()
      }
      Button("取消", role: .cancel) { pending = nil }
    }
  }

  /// Depth-first leaf ordering: each leaf takes the next column, parents sit
  /// centered over their children. Unreachable nodes become detached columns
  /// rather than silently disappearing.
  private func layout(_ tree: FlowTree) -> TreePlan {
    var plan = TreePlan()
    let byEvent = Dictionary(
      tree.nodes.map { ($0.event, $0) }, uniquingKeysWith: { first, _ in first })
    var depthOf: [Int: Int] = [:]
    var slotOf: [Int: Int] = [:]
    var nextSlot = 0
    var maxDepth = 0

    func children(of event: Int) -> [Int] {
      byEvent[event]?.edges.compactMap { $0.to }.filter { byEvent[$0] != nil } ?? []
    }

    func assign(_ root: Int) {
      var stack: [(event: Int, depth: Int, resolved: Bool)] = [(root, 0, false)]
      while let (event, depth, resolved) = stack.popLast() {
        if resolved {
          let kids = children(of: event).compactMap { slotOf[$0] }
          // Centre a parent over its subtree: that is what makes the canvas
          // read as a tree.  Opening on empty space is handled by centring the
          // scroll view on the canvas instead of flattening the layout.
          let slot = kids.isEmpty ? nextSlot : (kids.first! + kids.last!) / 2
          if kids.isEmpty { nextSlot += 1 }
          slotOf[event] = slot
          plan.positions[event] = CGPoint(
            x: canvasInset + CGFloat(slot) * columnWidth + nodeWidth / 2,
            y: headerHeight + CGFloat(depth) * rowHeight + nodeHeight / 2)
          continue
        }
        guard depthOf[event] == nil else { continue }
        depthOf[event] = depth
        maxDepth = max(maxDepth, depth)
        stack.append((event, depth, true))
        for child in children(of: event) where depthOf[child] == nil {
          stack.append((child, depth + 1, false))
        }
      }
    }

    let roots = tree.roots.isEmpty ? tree.nodes.map { $0.event } : tree.roots
    for root in roots {
      assign(root)
      if let point = plan.positions[root], let node = byEvent[root] {
        let name =
          node.route == 0
          ? "共通线"
          : (tree.routes.indices.contains(node.route)
            ? tree.routes[node.route] : "路线 \(node.route)")
        // Sit the route label over the first chapter it introduces, so it stays
        // in view now that the canvas opens horizontally centred instead of on
        // the column the subtree happens to start in.
        plan.headers.append(
          TreePlan.Header(name: name, at: CGPoint(x: point.x, y: 20)))
      }
    }
    for node in tree.nodes where plan.positions[node.event] == nil {
      assign(node.event)
    }

    for node in tree.nodes {
      guard let from = plan.positions[node.event] else { continue }
      for edge in node.edges {
        guard let target = edge.to, let to = plan.positions[target] else { continue }
        plan.links.append(
          TreePlan.Link(
            from: CGPoint(x: from.x, y: from.y + nodeHeight / 2),
            to: CGPoint(x: to.x, y: to.y - nodeHeight / 2),
            edge: edge))
      }
    }
    plan.size = CGSize(
      width: canvasInset * 2 + CGFloat(max(nextSlot, 1)) * columnWidth,
      height: headerHeight + CGFloat(maxDepth + 1) * rowHeight + canvasInset)
    return plan
  }

  /// Cubic control points share the vertical midpoint, so t = 0.5 lands on
  /// ((from.x + to.x) / 2, midY): that is where the label chip sits.
  private func chipPoint(_ link: TreePlan.Link) -> CGPoint {
    CGPoint(x: (link.from.x + link.to.x) / 2, y: (link.from.y + link.to.y) / 2)
  }

  private func edgeCanvas(_ plan: TreePlan) -> some View {
    Canvas { context, _ in
      for link in plan.links {
        var path = Path()
        path.move(to: link.from)
        let midY = (link.from.y + link.to.y) / 2
        path.addCurve(
          to: link.to,
          control1: CGPoint(x: link.from.x, y: midY),
          control2: CGPoint(x: link.to.x, y: midY))
        context.stroke(
          path,
          with: .color(link.edge.walked ? GameTheme.accent : GameTheme.border),
          style: StrokeStyle(
            lineWidth: link.edge.walked ? 2.5 : 1.5,
            dash: link.edge.walked ? [] : [5, 4]))
      }
    }
  }

  private func nodeCard(_ node: FlowNode) -> some View {
    Button {
      if node.unlocked { ask(.event(node.event)) }
    } label: {
      HStack(spacing: 7) {
        Image(systemName: symbol(node))
          .font(.subheadline)
          .foregroundStyle(node.unlocked ? GameTheme.accent : .secondary)
          .accessibilityHidden(true)
        Text(node.title.isEmpty ? "事件 \(node.event)" : node.title)
          .font(.subheadline.weight(.medium))
          .foregroundStyle(node.unlocked ? .primary : .secondary)
          .lineLimit(2)
          .multilineTextAlignment(.leading)
          .frame(maxWidth: .infinity, alignment: .leading)
        if node.visited {
          Image(systemName: "checkmark.circle.fill")
            .font(.caption)
            .foregroundStyle(GameTheme.success)
            .accessibilityLabel("已读")
        }
      }
      .padding(.horizontal, 9)
      .padding(.vertical, 6)
      .frame(width: nodeWidth, height: nodeHeight)
      .background(
        node.unlocked ? GameTheme.surface : GameTheme.surface.opacity(0.45),
        in: RoundedRectangle(cornerRadius: 10)
      )
      .overlay {
        RoundedRectangle(cornerRadius: 10)
          .strokeBorder(
            node.visited ? GameTheme.accent.opacity(0.55) : GameTheme.border,
            lineWidth: 1
          )
          .allowsHitTesting(false)
      }
    }
    .buttonStyle(.plain)
    .disabled(!node.unlocked)
    .accessibilityIdentifier("tree-node-\(node.event)")
  }

  private func branchChip(_ link: TreePlan.Link, label: String) -> some View {
    let edge = link.edge
    return Button {
      if edge.jumpable, let selection = edge.selection, let choice = edge.choice {
        ask(.choice(selection: selection, choice: choice))
      }
    } label: {
      Text(label)
        .font(.caption.weight(.medium))
        .lineLimit(1)
        .padding(.horizontal, 10).padding(.vertical, 4)
        .background(
          edge.walked ? GameTheme.accent : GameTheme.surface, in: Capsule()
        )
        .overlay {
          Capsule()
            .strokeBorder(edge.walked ? Color.clear : GameTheme.border, lineWidth: 1)
            .allowsHitTesting(false)
        }
        .foregroundStyle(edge.walked ? .white : .secondary)
    }
    .buttonStyle(.plain)
    .disabled(!edge.jumpable)
    .accessibilityIdentifier(
      "tree-branch-\(edge.selection ?? -1)-\(edge.choice ?? -1)")
  }

  private var zoomBar: some View {
    HStack(spacing: 0) {
      Button {
        zoom = max(0.4, zoom - 0.2)
      } label: {
        Image(systemName: "minus").frame(width: 28, height: 28)
      }
      Text("\(Int(round(zoom * pinch * 100)))%")
        .font(.caption.monospacedDigit())
        .frame(minWidth: 42)
      Button {
        zoom = min(2.5, zoom + 0.2)
      } label: {
        Image(systemName: "plus").frame(width: 28, height: 28)
      }
      Button {
        zoom = 1
      } label: {
        Text("1x").font(.caption.weight(.semibold)).frame(width: 30, height: 28)
      }
    }
    .buttonStyle(.plain)
    .foregroundStyle(GameTheme.text)
    .padding(.horizontal, 6).padding(.vertical, 3)
    .background(.regularMaterial, in: Capsule())
    .overlay {
      Capsule().strokeBorder(GameTheme.border, lineWidth: 1)
        .allowsHitTesting(false)
    }
    .accessibilityIdentifier("story-tree-zoom")
  }

  private func symbol(_ node: FlowNode) -> String {
    switch node.kind {
    case "choice": return "arrow.triangle.branch"
    case "ending": return "flag.checkered"
    default: return "book.closed"
    }
  }

  private func ask(_ jump: PendingJump) {
    pending = jump
    confirm = true
  }
}
