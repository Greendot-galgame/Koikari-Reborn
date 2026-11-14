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
  private var isReady: Bool { game.handle != nil }
  private var applicationName: String {
    Bundle.main.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String ?? "租借男友 Reborn"
  }
  private var hasNotices: Bool {
    (!isReady && !game.resourceStatus.isEmpty)
      || game.originalUI.hasUnavailableArtwork
      || !(game.error ?? "").isEmpty
      || !(game.snapshot?.blocked ?? "").isEmpty
      || !(game.snapshot?.warnings ?? []).isEmpty
  }

  var body: some View {
    GeometryReader { geometry in
      let compact = geometry.size.height < 500
      let inset: CGFloat = geometry.size.width < 700 ? 16 : 32
      let width = min(960, max(0, geometry.size.width - inset * 2))
      let columns = geometry.size.width >= 620 && !dynamicTypeSize.isAccessibilitySize
      let menuWidth = min(dynamicTypeSize.isAccessibilitySize ? 440 : (compact ? 300 : 320), width)
      let gap: CGFloat = compact ? 24 : 72
      let logoWidth = max(0, min(460, width - menuWidth - gap))
      let mainHeight = hasNotices ? 0 : max(0, geometry.size.height - (compact ? 80 : 100))
      ScrollView {
        VStack(spacing: compact ? 12 : 20) {
          header
          if hasNotices { resourceNotices }
          Group {
            if columns {
              HStack(spacing: gap) {
                identity(height: compact ? 128 : 244).frame(width: logoWidth)
                menu(compact: compact).frame(width: menuWidth)
              }
            } else {
              VStack(spacing: 24) {
                identity(height: compact ? 104 : 160)
                menu(compact: compact).frame(maxWidth: menuWidth)
              }
            }
          }
          .frame(maxWidth: .infinity, minHeight: mainHeight)
        }
        .frame(width: width)
        .padding(.horizontal, inset).padding(.vertical, compact ? 12 : 24)
        .frame(maxWidth: .infinity, minHeight: geometry.size.height, alignment: .top)
      }
      .background {
        GameTheme.background
        if let background = game.originalUI.titleBackground {
          background.resizable().scaledToFill()
            .frame(width: geometry.size.width, height: geometry.size.height).clipped()
            .opacity(0.12).accessibilityHidden(true).allowsHitTesting(false)
        }
      }
    }
    .foregroundStyle(GameTheme.text).tint(GameTheme.accent).preferredColorScheme(.light)
    .disabled(game.importing || game.modalOpen)
  }

  private var header: some View {
    HStack {
      Text("Koikari Reborn").font(.callout.weight(.medium)).foregroundStyle(.secondary)
      Spacer()
      Button(action: openSettings) { Label("设置", systemImage: "gearshape") }
        .buttonStyle(.bordered).controlSize(.regular)
        .accessibilityIdentifier("settings")
    }
    .frame(minHeight: 32)
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

  private func menu(compact: Bool) -> some View {
    VStack(alignment: .leading, spacing: compact ? 10 : 16) {
      if isReady {
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
      } else {
        Text("准备游戏资源").font(.title3.weight(.semibold))
        Text("请选择自己持有的游戏资源，载入后即可开始。")
          .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        resourceButton
        #if os(macOS)
          Text("所选文件夹应直接包含 data.xp3。")
            .font(.caption).foregroundStyle(.secondary)
        #else
          Text("请将 data.xp3 放入「文件」App →「\(applicationName)」→ GameResources 文件夹，再重新检查。")
            .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        #endif
      }
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
    Button(action: openResources) {
      #if os(macOS)
        actionLabel("选择资源文件夹", symbol: "folder")
      #else
        actionLabel("重新检查资源", symbol: "arrow.clockwise")
      #endif
    }
    .buttonStyle(.borderedProminent).controlSize(.large)
    #if os(macOS)
      .accessibilityIdentifier("import-assets")
    #else
      .accessibilityIdentifier("reload-resources")
    #endif
  }

  @ViewBuilder
  private var resourceNotices: some View {
    if !isReady && !game.resourceStatus.isEmpty {
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
