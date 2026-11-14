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
          HStack(spacing: 6) {
            Image(systemName: "sparkle")
              .font(.system(size: 10, weight: .semibold))
              .foregroundStyle(GameTheme.accent)
              .accessibilityHidden(true)
            Text("人物介绍")
              .font(.system(size: 11, weight: .semibold))
              .tracking(2.2)
              .foregroundStyle(GameTheme.accent)
            Spacer(minLength: 0)
          }
          if !lines.description.isEmpty {
            Text(lines.description)
              .font(.system(size: descriptionSize * min(textScale, 1.3)))
              .foregroundStyle(GameTheme.secondaryText)
              .lineSpacing(max(2, lineSpacing * 0.5))
              .fixedSize(horizontal: false, vertical: true)
          }
          if !lines.quote.isEmpty {
            Text(lines.quote)
              .font(.system(size: quoteSize * textScale, weight: .medium, design: .serif))
              .foregroundStyle(GameTheme.text)
              .lineSpacing(lineSpacing + 1)
              .fixedSize(horizontal: false, vertical: true)
          }
          HStack(spacing: 4) {
            Spacer(minLength: 0)
            Text("轻触继续")
              .font(.system(size: 11.5, weight: .medium))
              .foregroundStyle(GameTheme.secondaryText)
            Image(systemName: "chevron.right")
              .font(.system(size: 9, weight: .semibold))
              .foregroundStyle(GameTheme.accent)
          }
          .padding(.top, 2)
          .accessibilityHidden(true)
        }
        .padding(.leading, 18)
        // The rail is an overlay so it spans the authored lines without making
        // the otherwise intrinsic card grow to fill the scene.
        .overlay(alignment: .leading) {
          RoundedRectangle(cornerRadius: 1.5)
            .fill(GameTheme.accent)
            .frame(width: 3)
            .accessibilityHidden(true)
        }
        .padding(.horizontal, 22)
        .padding(.vertical, 18)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
          RoundedRectangle(cornerRadius: 16)
            .fill(GameTheme.surface.opacity(min(1, max(0.65, backgroundOpacity))))
            .overlay {
              LinearGradient(
                colors: [GameTheme.accent.opacity(0.055), .clear],
                startPoint: .topLeading, endPoint: .center
              )
              .clipShape(RoundedRectangle(cornerRadius: 16))
            }
        }
        .overlay {
          RoundedRectangle(cornerRadius: 16)
            .strokeBorder(GameTheme.accent.opacity(0.20), lineWidth: 1)
            .allowsHitTesting(false)
        }
        .shadow(color: .black.opacity(0.12), radius: 16, y: 5)
        .contentShape(RoundedRectangle(cornerRadius: 16))
      }
      .buttonStyle(.plain)
      .accessibilityIdentifier("interlude-continue")
      .accessibilityLabel("人物介绍，轻触继续")
      .accessibilityHint("继续剧情")
    } else {
      // One-image eye-catches embed their own typography; add only a quiet,
      // light hint that belongs to the same native reading surface.
      Button(action: onContinue) {
        Label("轻触继续", systemImage: "chevron.right")
          .font(.system(size: 14, weight: .medium))
          .foregroundStyle(GameTheme.accent)
          .padding(.horizontal, 18)
          .padding(.vertical, 10)
          .background(GameTheme.surface.opacity(0.94), in: Capsule())
          .overlay {
            Capsule().strokeBorder(GameTheme.accent.opacity(0.22), lineWidth: 1)
          }
          .shadow(color: .black.opacity(0.10), radius: 8, y: 2)
          .contentShape(Capsule())
      }
      .buttonStyle(.plain)
      .accessibilityIdentifier("interlude-continue")
      .accessibilityLabel("人物介绍，轻触继续")
    }
  }
}
