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
  // Character introductions use a warmer, more theatrical palette than reading.
  static let interludeAccent = Color(red: 0.86, green: 0.72, blue: 0.46)
  static let interludeSurface = Color(red: 0.12, green: 0.10, blue: 0.12)
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
  let onAdvance: (() -> Void)?
  let controls: Controls
  @ScaledMetric(relativeTo: .body) private var bodySize: CGFloat = 22
  @State private var contentHeight: CGFloat = 0
  @State private var viewportHeight: CGFloat = 0

  init(
    speaker: String, text: String, page: UInt64, textScale: Double,
    backgroundOpacity: Double, lineSpacing: Double, showPageNumber: Bool,
    showsControls: Bool = true, onAdvance: (() -> Void)? = nil,
    @ViewBuilder controls: () -> Controls
  ) {
    self.speaker = speaker
    self.text = text
    self.page = page
    self.textScale = textScale
    self.backgroundOpacity = backgroundOpacity
    self.lineSpacing = lineSpacing
    self.showPageNumber = showPageNumber
    self.showsControls = showsControls
    self.onAdvance = onAdvance
    self.controls = controls()
  }

  private var contentOverflows: Bool { contentHeight > viewportHeight + 1 }
  private var hasHeader: Bool { !speaker.isEmpty || showPageNumber }
  private var scaledBodySize: CGFloat { bodySize * textScale }

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      if hasHeader {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
          if !speaker.isEmpty {
            Capsule().fill(GameTheme.accent).frame(width: 3, height: 15)
              .accessibilityHidden(true)
            Text(speaker)
              .font(.system(size: 15 * min(textScale, 1.25), weight: .semibold))
              .foregroundStyle(GameTheme.accent)
              .lineLimit(1)
              .accessibilityAddTraits(.isHeader)
          }
          Spacer(minLength: 8)
          if showPageNumber {
            Text("第 \(page) 页")
              .font(.caption2.monospacedDigit())
              .foregroundStyle(GameTheme.secondaryText)
              .lineLimit(1)
          }
        }
        .frame(maxWidth: .infinity)
        .contentShape(Rectangle())
        .onTapGesture {
          if let onAdvance { onAdvance() }
        }
        .padding(.bottom, 8)
        Divider().overlay(GameTheme.border).padding(.bottom, 10)
      }
      GeometryReader { viewport in
        ScrollViewReader { proxy in
          ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 0) {
              Color.clear.frame(height: 1).id("dialogue-top")
              Text(text)
                .font(.system(size: scaledBodySize, weight: .regular))
                .lineSpacing(lineSpacing)
                .tracking(0.15)
                .foregroundStyle(GameTheme.text)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.bottom, 2)
            }
            .background {
              GeometryReader { content in
                Color.clear.preference(
                  key: DialogueContentHeight.self, value: content.size.height)
              }
            }
            .frame(minHeight: max(0, viewport.size.height), alignment: .topLeading)
            .contentShape(Rectangle())
            .onTapGesture {
              if let onAdvance { onAdvance() }
            }
          }
          .scrollIndicators(.visible)
          .onAppear { viewportHeight = viewport.size.height }
          .onChange(of: viewport.size.height) { viewportHeight = $0 }
          .onChange(of: page) { _ in
            proxy.scrollTo("dialogue-top", anchor: .top)
          }
          .onChange(of: text) { _ in
            proxy.scrollTo("dialogue-top", anchor: .top)
          }
        }
      }
      .accessibilityHint(contentOverflows ? "长对白可上下滚动阅读，轻点可继续" : "")
      .accessibilityAction(named: "继续") {
        if let onAdvance { onAdvance() }
      }
      if showsControls {
        HStack(spacing: 8) {
          if contentOverflows {
            Label("上下滚动", systemImage: "arrow.up.arrow.down")
              .font(.caption2).foregroundStyle(GameTheme.secondaryText)
              .accessibilityElement(children: .ignore)
              .accessibilityLabel("对白可上下滚动")
          }
          Spacer(minLength: 0)
          controls
        }
        .frame(height: 34)
        .fixedSize(horizontal: false, vertical: true)
        .layoutPriority(1)
      }
    }
    .padding(.horizontal, 22)
    .padding(.vertical, 14)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .contentShape(RoundedRectangle(cornerRadius: 12))
    .background(
      RoundedRectangle(cornerRadius: 12)
        .fill(GameTheme.surface.opacity(min(1, max(0.35, backgroundOpacity))))
    )
    .overlay {
      RoundedRectangle(cornerRadius: 12)
        .strokeBorder(GameTheme.border, lineWidth: 1).allowsHitTesting(false)
    }
    .shadow(color: .black.opacity(0.10), radius: 14, y: 4)
    .onPreferenceChange(DialogueContentHeight.self) { contentHeight = $0 }
    .accessibilityElement(children: .contain)
  }
}

/// A deliberately separate presentation for character-introduction eye-catches.
/// The artwork already carries the character's name, so this card carries only
/// the two authored lines: a short description and the spoken quote.
struct GameInterludePanel: View {
  let text: String
  let textScale: Double
  let backgroundOpacity: Double
  let lineSpacing: Double
  let onContinue: () -> Void
  @ScaledMetric(relativeTo: .body) private var quoteSize: CGFloat = 23
  @ScaledMetric(relativeTo: .caption) private var descriptionSize: CGFloat = 15

  private var lines: (description: String, quote: String) {
    let pieces = text.split(separator: "\n", omittingEmptySubsequences: false)
      .map { String($0).trimmingCharacters(in: .whitespaces) }
      .filter { !$0.isEmpty }
    guard !pieces.isEmpty else { return ("", "") }
    if pieces.count == 1 { return ("", pieces[0]) }
    return (pieces[0], pieces.dropFirst().joined(separator: "\n"))
  }

  private var hasContent: Bool {
    !lines.description.isEmpty || !lines.quote.isEmpty
  }

  var body: some View {
    if hasContent {
      Button(action: onContinue) {
        VStack(alignment: .leading, spacing: 10) {
          HStack(spacing: 8) {
            Rectangle().fill(GameTheme.interludeAccent).frame(width: 22, height: 1)
            Text("人物介绍")
              .font(.system(size: 11, weight: .semibold))
              .tracking(3)
              .foregroundStyle(GameTheme.interludeAccent)
            Rectangle().fill(GameTheme.interludeAccent.opacity(0.45))
              .frame(width: 44, height: 1)
            Spacer(minLength: 0)
          }
          .accessibilityHidden(true)
          if !lines.description.isEmpty {
            Text(lines.description)
              .font(.system(size: descriptionSize * min(textScale, 1.3)))
              .foregroundStyle(.white.opacity(0.72))
              .lineSpacing(max(2, lineSpacing * 0.5))
              .fixedSize(horizontal: false, vertical: true)
          }
          if !lines.quote.isEmpty {
            Text(lines.quote)
              .font(.system(size: quoteSize * textScale, weight: .medium, design: .serif))
              .foregroundStyle(.white.opacity(0.96))
              .lineSpacing(lineSpacing + 1)
              .fixedSize(horizontal: false, vertical: true)
          }
          HStack(spacing: 4) {
            Spacer(minLength: 0)
            Text("轻触继续")
              .font(.system(size: 11.5, weight: .medium))
              .foregroundStyle(.white.opacity(0.58))
            Image(systemName: "chevron.right")
              .font(.system(size: 9, weight: .semibold))
              .foregroundStyle(.white.opacity(0.58))
          }
          .padding(.top, 2)
          .accessibilityHidden(true)
        }
        .padding(.horizontal, 26)
        .padding(.vertical, 20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
          RoundedRectangle(cornerRadius: 16)
            .fill(GameTheme.interludeSurface.opacity(min(1, max(0.55, backgroundOpacity))))
        }
        .overlay {
          RoundedRectangle(cornerRadius: 16)
            .strokeBorder(GameTheme.interludeAccent.opacity(0.28), lineWidth: 1)
            .allowsHitTesting(false)
        }
        .shadow(color: .black.opacity(0.22), radius: 20, y: 8)
        .contentShape(RoundedRectangle(cornerRadius: 16))
      }
      .buttonStyle(.plain)
      .accessibilityIdentifier("interlude-continue")
      .accessibilityLabel("人物介绍，轻触继续")
      .accessibilityHint("继续剧情")
    } else {
      // One-image eye-catches embed their own typography; only a quiet hint is added.
      Button(action: onContinue) {
        Label("轻触继续", systemImage: "chevron.right")
          .font(.system(size: 14, weight: .medium))
          .foregroundStyle(.white.opacity(0.92))
          .padding(.horizontal, 18)
          .padding(.vertical, 10)
          .background(.black.opacity(0.32), in: Capsule())
          .overlay { Capsule().strokeBorder(.white.opacity(0.22), lineWidth: 1) }
          .contentShape(Capsule())
      }
      .buttonStyle(.plain)
      .accessibilityIdentifier("interlude-continue")
      .accessibilityLabel("人物介绍，轻触继续")
    }
  }
}
