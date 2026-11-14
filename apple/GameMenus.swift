import SwiftUI

private struct LibrarySurface: ViewModifier {
  var width: CGFloat = 680
  func body(content: Self.Content) -> some View {
    content
      .frame(minWidth: 300, idealWidth: width, minHeight: 280, idealHeight: 620)
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
          entry(.replayVoice, "重播语音", "speaker.wave.2").disabled(!hasVoice)
          entry(.toggleMute, muted ? "取消静音" : "静音", muted ? "speaker.wave.2" : "speaker.slash")
          entry(.settings, "设置", "slider.horizontal.3")
        }
        Section("其他") {
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
        VStack(alignment: .leading, spacing: 16) {
          HStack {
            Button {
              game.hideCG()
              self.selected = nil
            } label: {
              Label("返回列表", systemImage: "chevron.left")
            }.buttonStyle(.bordered).accessibilityIdentifier("gallery-back")
            Spacer()
            Text(selected.title).font(.callout).foregroundStyle(.secondary)
          }
          MetalCanvas(game: game).aspectRatio(16 / 9, contentMode: .fit)
            .accessibilityIdentifier("gallery-preview")
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
    }
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
        ZStack {
          Rectangle().fill(GameTheme.surface)
          if !entry.unlocked {
            Image(systemName: "lock").font(.title2).foregroundStyle(GameTheme.text)
          } else if replay {
            Image(systemName: "play.rectangle").font(.largeTitle).foregroundStyle(GameTheme.accent)
          } else if let thumbnail, thumbnailGeneration == game.collectionGeneration {
            thumbnail.resizable().scaledToFit()
          } else if unavailable, thumbnailGeneration == game.collectionGeneration {
            VStack(spacing: 6) {
              Image(systemName: "photo").font(.title2)
              Text("预览暂不可用").font(.caption)
            }.foregroundStyle(.secondary)
          } else {
            ProgressView().controlSize(.small)
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
      guard entry.unlocked, !replay else { return }
      let result = await game.galleryThumbnail(entry)
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
            LazyVStack(spacing: 12) {
              ForEach(game.timeline) { page in
                Button {
                  selected = page
                  confirmRollback = true
                } label: {
                  GroupBox {
                    VStack(alignment: .leading, spacing: 8) {
                      HStack(alignment: .firstTextBaseline) {
                        Text(page.interlude ? "人物介绍" : (page.speaker.isEmpty ? "旁白" : page.speaker))
                          .font(.callout.weight(.semibold))
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
            }.padding(.horizontal, 20).padding(.bottom, 20)
          }.onAppear {
            if let last = game.timeline.last { proxy.scrollTo(last.page, anchor: .bottom) }
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
  private var automatic: [SaveSlot] { game.slots.filter { $0.slot == 9 } }
  private var manual: [SaveSlot] { game.slots.filter { $0.slot != 9 }.sorted { $0.slot < $1.slot } }
  private var canSave: Bool { !(game.snapshot?.title ?? true) && game.snapshot?.blocked == nil }

  var body: some View {
    GeometryReader { proxy in page(width: proxy.size.width) }
      .modifier(LibrarySurface(width: 720))
      .onAppear { game.beginSavePreviews() }
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
  }

  private func page(width: CGFloat) -> some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: saving ? "保存进度" : "读取存档", closeIdentifier: "close-saves") { dismiss() }
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
            if !manual.isEmpty {
              VStack(alignment: .leading, spacing: 10) {
                Text("手动存档").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                LazyVGrid(
                  columns: [GridItem(.adaptive(minimum: 260), spacing: 12, alignment: .top)],
                  spacing: 12
                ) {
                  ForEach(manual) { slot in card(slot, width: width) }
                }
              }
            }
          }.padding(20)
        }
      }
    }
  }

  private func card(_ slot: SaveSlot, width: CGFloat) -> some View {
    GroupBox {
      if slot.slot == 9 && width >= 360 && !typeSize.isAccessibilitySize {
        HStack(alignment: .top, spacing: width >= 600 ? 16 : 12) {
          SaveSlotThumbnail(game: game, slot: slot).frame(width: width >= 600 ? 192 : 104)
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
        if saving && slot.slot != 9 {
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
