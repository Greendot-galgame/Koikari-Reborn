import SwiftUI

enum GameTheme {
  #if os(macOS)
    static let background = Color(nsColor: .windowBackgroundColor)
    static let surface = Color(nsColor: .controlBackgroundColor)
  #else
    static let background = Color(uiColor: .systemGroupedBackground)
    static let surface = Color(uiColor: .secondarySystemGroupedBackground)
  #endif
  // A single tint echoes the original logo; component shapes and feedback stay native.
  static let accent = Color(red: 0.47, green: 0.14, blue: 0.57)
  static let text = Color.primary
  static let secondaryText = Color.secondary
  static let border = Color.secondary.opacity(0.18)
  static let warning = Color(red: 0.60, green: 0.28, blue: 0.08)
  static let success = Color(red: 0.17, green: 0.42, blue: 0.31)
  static let cornerRadius: CGFloat = 10
}

struct GameToolbarLabelStyle: LabelStyle {
  let compact: Bool
  func makeBody(configuration: Configuration) -> some View {
    if compact {
      configuration.icon
    } else {
      HStack(spacing: 6) {
        configuration.icon
        configuration.title
      }
    }
  }
}

struct GamePanelStyle: ViewModifier {
  func body(content: Self.Content) -> some View { GroupBox { content } }
}

struct GameSheetHeader: View {
  let title: String
  let closeIdentifier: String
  let close: () -> Void

  init(title: String, closeIdentifier: String, close: @escaping () -> Void) {
    self.title = title
    self.closeIdentifier = closeIdentifier
    self.close = close
  }

  var body: some View {
    VStack(spacing: 0) {
      HStack(spacing: 16) {
        Text(title).font(.headline).accessibilityAddTraits(.isHeader)
        Spacer(minLength: 8)
        Button("完成", action: close)
          .buttonStyle(.bordered)
          .accessibilityIdentifier(closeIdentifier)
      }
      .padding(.horizontal, 20)
      .padding(.vertical, 12)
      Divider()
    }
  }
}

private struct DialogueContentHeight: PreferenceKey {
  static let defaultValue: CGFloat = 0
  static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
    value = max(value, nextValue())
  }
}

private struct DialogueViewportHeight: PreferenceKey {
  static let defaultValue: CGFloat = 0
  static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
    value = max(value, nextValue())
  }
}

// The same visual component is used for live dialogue and the settings preview.
// Only the caller supplies actions; the preview has no connection to the engine.
struct GameReadingPanel<Controls: View>: View {
  let speaker: String
  let text: String
  let page: UInt64
  let textScale: Double
  let backgroundOpacity: Double
  let lineSpacing: Double
  let showPageNumber: Bool
  let showsControls: Bool
  let controls: Controls
  @ScaledMetric(relativeTo: .body) private var bodySize: CGFloat = 18
  @State private var contentHeight: CGFloat = 0
  @State private var viewportHeight: CGFloat = 0

  init(
    speaker: String, text: String, page: UInt64, textScale: Double,
    backgroundOpacity: Double, lineSpacing: Double, showPageNumber: Bool,
    showsControls: Bool = true, @ViewBuilder controls: () -> Controls
  ) {
    self.speaker = speaker
    self.text = text
    self.page = page
    self.textScale = textScale
    self.backgroundOpacity = backgroundOpacity
    self.lineSpacing = lineSpacing
    self.showPageNumber = showPageNumber
    self.showsControls = showsControls
    self.controls = controls()
  }

  private var contentOverflows: Bool { contentHeight > viewportHeight + 1 }

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      ScrollView {
        VStack(alignment: .leading, spacing: 6) {
          HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(speaker.isEmpty ? "旁白" : speaker)
              .font(.subheadline.weight(.semibold)).foregroundStyle(GameTheme.text)
              .fixedSize(horizontal: false, vertical: true)
              .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 0)
            if showPageNumber {
              Text("第 \(page) 页").font(.caption.monospacedDigit())
                .foregroundStyle(GameTheme.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
            }
          }
          Text(text)
            .font(.system(size: bodySize * textScale))
            .lineSpacing(lineSpacing)
            .foregroundStyle(GameTheme.text)
            .frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
        }
        .background {
          GeometryReader { proxy in
            Color.clear.preference(key: DialogueContentHeight.self, value: proxy.size.height)
          }
        }
        .padding(.bottom, 2)
      }
      .scrollIndicators(.visible)
      .id("\(page)-\(text)")
      .background {
        GeometryReader { proxy in
          Color.clear.preference(key: DialogueViewportHeight.self, value: proxy.size.height)
        }
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
      .accessibilityHint(contentOverflows ? "长对白可上下滚动阅读" : "")
      if showsControls {
        HStack(spacing: 8) {
          if contentOverflows {
            ViewThatFits(in: .horizontal) {
              Label("上下滚动", systemImage: "arrow.up.arrow.down")
              Image(systemName: "arrow.up.arrow.down")
            }
            .font(.caption).foregroundStyle(GameTheme.secondaryText)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("对白可上下滚动")
            .help("长对白可上下滚动阅读")
          }
          Spacer(minLength: 0)
          controls
        }
        .fixedSize(horizontal: false, vertical: true)
        .layoutPriority(1)
      }
    }
    .padding(.horizontal, 14)
    .padding(.vertical, 10)
    .background(
      GameTheme.surface.opacity(backgroundOpacity),
      in: RoundedRectangle(cornerRadius: GameTheme.cornerRadius)
    )
    .overlay {
      RoundedRectangle(cornerRadius: GameTheme.cornerRadius)
        .strokeBorder(GameTheme.border, lineWidth: 1).allowsHitTesting(false)
    }
    .onPreferenceChange(DialogueContentHeight.self) { contentHeight = $0 }
    .onPreferenceChange(DialogueViewportHeight.self) { viewportHeight = $0 }
    .accessibilityElement(children: .contain)
  }
}
