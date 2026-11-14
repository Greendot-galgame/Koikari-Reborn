import Foundation
import SwiftUI

/// Native controls own all actions. Original artwork is optional, read-only decoration.
@MainActor
struct GameTitleScreen: View {
  @ObservedObject var game: Game
  let startNew: () -> Void
  let resume: () -> Void
  let openSaves: () -> Void
  let openCollection: (Bool) -> Void
  let openResources: () -> Void
  let openSettings: () -> Void

  @Environment(\.dynamicTypeSize) private var dynamicTypeSize
  @State private var requestedResourceCheck = false
  private var isReady: Bool { game.handle != nil }
  private var applicationName: String {
    Bundle.main.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String ?? "租借男友 Reborn"
  }
  private var showsResourceStatus: Bool {
    // Initial setup already explains what's needed. Show a check result only
    // after an explicit retry, without hiding actual errors or compatibility notices.
    #if os(iOS)
      !isReady && requestedResourceCheck && !game.resourceStatus.isEmpty && game.error == nil
    #else
      false
    #endif
  }
  private var hasNotices: Bool {
    showsResourceStatus || game.originalUI.hasUnavailableArtwork
      || !(game.error ?? "").isEmpty || !(game.snapshot?.blocked ?? "").isEmpty
      || !(game.snapshot?.warnings ?? []).isEmpty
  }

  var body: some View {
    GeometryReader { geometry in
      let compact = geometry.size.height < 500
      let inset: CGFloat = geometry.size.width < 700 ? 16 : 32
      let width = min(960, max(0, geometry.size.width - inset * 2))
      VStack(spacing: 0) {
        header.frame(width: width)
          .padding(.horizontal, inset).padding(.vertical, compact ? 8 : 16)
          .frame(maxWidth: .infinity)
        GeometryReader { content in
          ScrollView {
            Group {
              if isReady {
                readyContent(width: width, compact: compact)
                  .frame(maxWidth: 960)
              } else {
                onboarding(compact: compact).frame(maxWidth: 560)
              }
            }
            .padding(.horizontal, inset).padding(.vertical, compact ? 12 : 24)
            // Center the complete content, including notices. A short window or
            // large type grows the scroll content instead of clipping the action.
            .frame(maxWidth: .infinity, minHeight: content.size.height)
          }
        }
      }
      .background {
        GameTheme.background
        if isReady, let background = game.originalUI.titleBackground {
          background.resizable().scaledToFill()
            .frame(width: geometry.size.width, height: geometry.size.height).clipped()
            .opacity(0.12).accessibilityHidden(true).allowsHitTesting(false)
        } else if !isReady {
          GameTheme.accent.opacity(0.025)
        }
      }
    }
    .foregroundStyle(GameTheme.text).tint(GameTheme.accent).preferredColorScheme(.light)
    .disabled(game.importing || game.modalOpen)
  }

  private var header: some View {
    HStack {
      Spacer()
      Button(action: openSettings) { Label("设置", systemImage: "gearshape") }
        .buttonStyle(.bordered).buttonBorderShape(.roundedRectangle)
        .controlSize(.regular).accessibilityIdentifier("settings")
    }
    .frame(minHeight: 32)
  }

  private func onboarding(compact: Bool) -> some View {
    VStack(alignment: .leading, spacing: compact ? 16 : 24) {
      HStack(spacing: 16) {
        Image(systemName: "folder.badge.plus")
          .font(.system(size: compact ? 24 : 30, weight: .regular))
          .foregroundStyle(GameTheme.accent)
          .frame(width: compact ? 48 : 64, height: compact ? 48 : 64)
          .background(GameTheme.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
          .accessibilityHidden(true)
        Text("租借男友").font(.title.weight(.semibold)).accessibilityAddTraits(.isHeader)
      }
      Divider()
      VStack(alignment: .leading, spacing: 8) {
        Text("准备游戏资源").font(.headline)
        #if os(macOS)
          Text("选择你持有的原作资源文件夹，载入后即可开始。")
            .font(.callout).foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        #else
          Text("将原作资源放入本应用文稿，再检查并载入。")
            .font(.callout).foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        #endif
      }
      resourceGuide
      resourceButton
        .fixedSize(horizontal: !dynamicTypeSize.isAccessibilitySize, vertical: true)
        .frame(maxWidth: .infinity, alignment: .trailing)
      if hasNotices { resourceNotices }
    }
    .padding(compact ? 20 : 28)
    .background(GameTheme.surface, in: RoundedRectangle(cornerRadius: 16))
    .overlay {
      RoundedRectangle(cornerRadius: 16).strokeBorder(GameTheme.border.opacity(0.55), lineWidth: 1)
        .allowsHitTesting(false)
    }
    .shadow(color: .black.opacity(0.035), radius: 16, y: 6)
    .accessibilityIdentifier("resource-onboarding")
  }

  private var resourceGuide: some View {
    HStack(alignment: .top, spacing: 8) {
      Image(systemName: "doc.zipper").accessibilityHidden(true)
      #if os(macOS)
        (Text("文件夹内需包含 ") + Text("data.xp3").font(.system(.caption, design: .monospaced))
          + Text("。"))
          .fixedSize(horizontal: false, vertical: true)
      #else
        VStack(alignment: .leading, spacing: 6) {
          Text("「文件」App →「\(applicationName)」→ GameResources")
          Text("此文件夹内需包含 data.xp3。")
        }.fixedSize(horizontal: false, vertical: true)
      #endif
    }
    .font(.caption).foregroundStyle(.secondary)
    .accessibilityElement(children: .combine)
  }

  private func readyContent(width: CGFloat, compact: Bool) -> some View {
    let columns = width >= 580 && !dynamicTypeSize.isAccessibilitySize
    let menuWidth = min(dynamicTypeSize.isAccessibilitySize ? 440 : (compact ? 300 : 320), width)
    let gap: CGFloat = compact ? 24 : 72
    let logoWidth = max(0, min(460, width - menuWidth - gap))
    return VStack(spacing: compact ? 16 : 28) {
      Group {
        if columns {
          HStack(spacing: gap) {
            identity(height: compact ? 128 : 244).frame(width: logoWidth)
            menu.frame(width: menuWidth)
          }
        } else {
          VStack(spacing: 24) {
            identity(height: compact ? 104 : 160)
            menu.frame(maxWidth: menuWidth)
          }
        }
      }.frame(maxWidth: .infinity)
      if hasNotices { resourceNotices }
    }
  }

  @ViewBuilder private func identity(height: CGFloat) -> some View {
    if let logo = game.originalUI.titleLogo {
      logo.resizable().scaledToFit().frame(height: height)
        .accessibilityLabel("恋爱，我就借走咯")
    } else {
      Text("租借男友").font(.largeTitle.weight(.semibold))
        .frame(height: min(height, 128)).accessibilityAddTraits(.isHeader)
    }
  }

  private var menu: some View {
    VStack(alignment: .leading, spacing: 16) {
      if game.hasSave {
        Button(action: resume) { actionLabel("继续游戏", symbol: "play.fill") }
          .buttonStyle(.borderedProminent).controlSize(.large)
          .accessibilityIdentifier("continue")
      } else {
        newGameButton
      }
      VStack(spacing: 10) {
        if game.hasSave {
          actionRow {
            newGameButton
          } trailing: {
            savesButton
          }
        } else {
          savesButton
        }
        actionRow {
          Button {
            openCollection(false)
          } label: {
            secondaryLabel("CG 鉴赏", symbol: "photo")
          }
          .accessibilityIdentifier("title-gallery")
        } trailing: {
          Button {
            openCollection(true)
          } label: {
            secondaryLabel("场景回想", symbol: "play.rectangle")
          }
          .accessibilityIdentifier("title-replays")
        }
      }
      .buttonStyle(.bordered).controlSize(.large)
    }
  }

  @ViewBuilder private var newGameButton: some View {
    let button = Button(action: startNew) { actionLabel("新游戏", symbol: "plus") }
      .controlSize(.large).accessibilityIdentifier("new-game")
    if game.hasSave {
      button.buttonStyle(.bordered)
    } else {
      button.buttonStyle(.borderedProminent)
    }
  }

  private var savesButton: some View {
    Button(action: openSaves) { secondaryLabel("存档管理", symbol: "tray") }
      .accessibilityIdentifier("title-saves")
  }

  @ViewBuilder private func actionRow<Leading: View, Trailing: View>(
    @ViewBuilder leading: () -> Leading, @ViewBuilder trailing: () -> Trailing
  ) -> some View {
    if dynamicTypeSize.isAccessibilitySize {
      VStack(spacing: 10) {
        leading()
        trailing()
      }
    } else {
      HStack(spacing: 10) {
        leading()
        trailing()
      }
    }
  }

  private func secondaryLabel(_ title: String, symbol: String) -> some View {
    Label(title, systemImage: symbol).frame(maxWidth: .infinity, minHeight: 30)
  }

  private var resourceButton: some View {
    Button {
      requestedResourceCheck = true
      openResources()
    } label: {
      #if os(macOS)
        actionLabel("选择资源文件夹", symbol: "folder")
      #else
        actionLabel("重新检查资源", symbol: "arrow.clockwise")
      #endif
    }
    .buttonStyle(.borderedProminent).buttonBorderShape(.roundedRectangle)
    .controlSize(.large)
    #if os(macOS)
      .accessibilityIdentifier("import-assets")
    #else
      .accessibilityIdentifier("reload-resources")
    #endif
  }

  @ViewBuilder
  private var resourceNotices: some View {
    if showsResourceStatus {
      notice(title: "资源状态", text: game.resourceStatus, symbol: "info.circle")
    }
    if game.originalUI.hasUnavailableArtwork {
      notice(title: "界面素材", text: "部分原作标题素材不可用，已使用原生样式补足。", symbol: "photo")
    }
    if let error = game.error, !error.isEmpty {
      notice(title: "错误提示", text: error, symbol: "exclamationmark.circle", warning: true)
    }
    if let blocked = game.snapshot?.blocked, !blocked.isEmpty {
      notice(title: "运行提示", text: blocked, symbol: "exclamationmark.triangle", warning: true)
    }
    ForEach(Array((game.snapshot?.warnings ?? []).enumerated()), id: \.offset) { _, warning in
      notice(title: "兼容提示", text: warning, symbol: "exclamationmark.triangle", warning: true)
    }
  }

  private func notice(title: String, text: String, symbol: String, warning: Bool = false)
    -> some View
  {
    HStack(alignment: .top, spacing: 8) {
      Image(systemName: symbol)
        .foregroundStyle(warning ? GameTheme.warning : GameTheme.accent)
        .accessibilityHidden(true)
      (warning ? Text("\(title) · ").fontWeight(.semibold) + Text(text) : Text(text))
        .foregroundStyle(warning ? GameTheme.warning : GameTheme.secondaryText)
        .lineSpacing(2)
        .fixedSize(horizontal: false, vertical: true)
        .textSelection(.enabled)
    }
    .font(.caption)
    .frame(maxWidth: .infinity, alignment: .leading)
    .accessibilityElement(children: .combine)
  }

  private func actionLabel(_ title: String, symbol: String) -> some View {
    Label(title, systemImage: symbol)
      .frame(maxWidth: .infinity, minHeight: 30)
  }
}
