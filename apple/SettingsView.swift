import SwiftUI

struct Settings: View {
  @ObservedObject var game: Game
  @Environment(\.dismiss) private var dismiss
  @ScaledMetric(relativeTo: .body) private var readingTypeSize: CGFloat = 18

  var body: some View {
    VStack(spacing: 0) {
      GameSheetHeader(title: "阅读与声音", closeIdentifier: "close-settings") { dismiss() }

      Form {
        Section("阅读") {
          preview
          slider(
            "对白字号", value: $game.textScale, range: 0.8...1.6, step: 0.1,
            display: "\(Int((game.textScale * 100).rounded()))%", identifier: "text-scale")
          slider(
            "背景不透明度", value: $game.dialogueOpacity, range: 0.65...1, step: 0.05,
            display: "\(Int((game.dialogueOpacity * 100).rounded()))%",
            identifier: "dialogue-opacity")
          slider(
            "行间距", value: $game.dialogueLineSpacing, range: 2...12, step: 1,
            display: "\(Int(game.dialogueLineSpacing))", identifier: "dialogue-line-spacing")
          Toggle("显示剧情页码", isOn: $game.showPageNumber)
            .accessibilityIdentifier("show-page-number")
        }

        Section {
          Toggle(
            "全部静音",
            isOn: Binding(get: { game.muted }, set: { if $0 != game.muted { game.toggleMute() } })
          )
          slider(
            "音乐", value: $game.bgmVolume, range: 0...1, step: 0.01,
            display: "\(Int((game.bgmVolume * 100).rounded()))%", identifier: "bgm-volume")
          slider(
            "语音", value: $game.voiceVolume, range: 0...1, step: 0.01,
            display: "\(Int((game.voiceVolume * 100).rounded()))%", identifier: "voice-volume")
          slider(
            "音效", value: $game.seVolume, range: 0...1, step: 0.01,
            display: "\(Int((game.seVolume * 100).rounded()))%", identifier: "se-volume")
        } header: {
          Text("声音")
        } footer: {
          if game.muted {
            Text("当前已静音；音量设置会保留，取消静音后生效。")
          }
        }

        Section {
          DisclosureGroup("操作说明") {
            VStack(alignment: .leading, spacing: 8) {
              Text("长对白可上下滚动。点击画面或下一句继续；快进中再次点击只会停止。")
              Text("隐藏界面后，点击画面或按空格只会恢复界面，不会翻页。选项、视频、结局和弹窗会停止快进。")
              #if os(macOS)
                Text("空格 / → 下一句　← 上一句　F 快进\n⌘S 存档　Esc 停止快进或显示界面")
              #endif
            }
            .font(.callout).foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
          }
        }

        Section {
          Button("恢复默认设置") {
            game.textScale = 1
            game.dialogueOpacity = 0.9
            game.dialogueLineSpacing = 5
            game.showPageNumber = false
            game.bgmVolume = 0.35
            game.voiceVolume = 1
            game.seVolume = 0.7
            if game.muted { game.toggleMute() }
          }
          .buttonStyle(.bordered)
          .accessibilityIdentifier("reset-settings")
        } footer: {
          Text("设置自动保存。")
        }
      }
      .formStyle(.grouped)
    }
    .frame(minWidth: 300, idealWidth: 620, minHeight: 250, idealHeight: 660)
    .background(GameTheme.background).foregroundStyle(GameTheme.text)
    .tint(GameTheme.accent).preferredColorScheme(.light)
  }

  private var preview: some View {
    GameReadingPanel(
      speaker: "阅读预览", text: "「今天，也想和你多待一会儿。」\n一起翻开故事的下一页吧。",
      page: 128, textScale: game.textScale, backgroundOpacity: game.dialogueOpacity,
      lineSpacing: game.dialogueLineSpacing, showPageNumber: game.showPageNumber,
      showsControls: false
    ) {
      EmptyView()
    }
    .frame(
      height: min(160, 112 * max(1, CGFloat(game.textScale) * readingTypeSize / 18))
    )
    .accessibilityIdentifier("reading-preview")
  }

  private func slider(
    _ title: String, value: Binding<Double>, range: ClosedRange<Double>, step: Double,
    display: String, identifier: String
  ) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack {
        Text(title)
        Spacer()
        Text(display).monospacedDigit().foregroundStyle(.secondary)
      }
      Slider(value: value, in: range, step: step)
        .accessibilityLabel(title).accessibilityValue(display).accessibilityIdentifier(identifier)
    }
  }
}
