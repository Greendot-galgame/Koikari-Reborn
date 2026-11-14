import Foundation

// Pure policies shared by the UI and the no-resource native regression tests.
struct ReadingEligibility {
  var active = false
  var hasScene = false
  var title = true
  var finished = false
  var blocked = false
  var messageVisible = false
  var hasChoices = false
  var hasVideo = false
  var hasCredits = false
  var hasInterlude = false
  var waiting = false
  var modalOpen = false
  var importing = false
  var hasError = false

  /// Hiding also pauses the story clock, so a waiting page is never hidden.
  var canHide: Bool {
    canOfferHide && messageVisible && !waiting
  }

  /// Stable toolbar affordance: transient page states (waits, a momentarily
  /// empty message layer) must not make the control blink during fast forward.
  var canOfferHide: Bool {
    active && hasScene && !title && !finished && !blocked
      && !hasChoices && !hasVideo && !hasCredits && !hasInterlude
      && !modalOpen && !importing && !hasError
  }
}

enum ReadingInputPolicy {
  enum Intent: Equatable {
    case none, reveal, stopFastForward, advance
  }

  static func nextIntent(
    interfaceHidden: Bool, fastForward: Bool, blocked: Bool, canAdvance: Bool
  ) -> Intent {
    guard !blocked else { return .none }
    if interfaceHidden { return .reveal }
    if fastForward { return .stopFastForward }
    return canAdvance ? .advance : .none
  }

  static func permitsStoryTick(mediaPlaybackAllowed: Bool, interfaceHidden: Bool) -> Bool {
    mediaPlaybackAllowed && !interfaceHidden
  }
}

enum GameplayMenuAction: String, CaseIterable, Sendable {
  case save, load, replayVoice, toggleMute, settings, gallery, replay, diagnostics, returnToTitle
}

/// Keep the game paused until UIKit has actually dismissed the menu. A selected
/// action is consumed once, after dismissal, never while another presenter owns it.
struct GameplayMenuPresentation: Equatable {
  private enum Phase: Equatable {
    case closed, presented
    case dismissing(GameplayMenuAction?)
  }
  private var phase: Phase = .closed
  var isPresented: Bool { phase == .presented }
  var blocksGameplay: Bool { phase != .closed }

  @discardableResult mutating func present() -> Bool {
    guard phase == .closed else { return false }
    phase = .presented
    return true
  }
  @discardableResult mutating func select(_ action: GameplayMenuAction) -> Bool {
    guard phase == .presented else { return false }
    phase = .dismissing(action)
    return true
  }
  mutating func dismiss() {
    // SwiftUI also writes false after selecting an action. Preserve that action.
    if phase == .presented { phase = .dismissing(nil) }
  }
  mutating func didDismiss() -> GameplayMenuAction? {
    let action: GameplayMenuAction?
    if case .dismissing(let pending) = phase { action = pending } else { action = nil }
    phase = .closed
    return action
  }
}

/// Page cadence while fast-forwarding. One is the original 250 ms page step;
/// higher values shorten the interval without changing any engine semantics.
enum FastForwardPolicy {
  static let baseInterval: UInt64 = 250_000_000
  static let range: ClosedRange<Double> = 1...5

  static func intervalNanoseconds(speed: Double) -> UInt64 {
    guard speed.isFinite else { return baseInterval }
    let clamped = min(range.upperBound, max(range.lowerBound, speed))
    return UInt64((Double(baseInterval) / clamped).rounded())
  }
}

/// The reading box is deliberately independent of the current page and of any
/// sprite placement: its geometry is derived only from the canvas, the user's
/// size preference and Dynamic Type. Text length changes what scrolls, not the
/// box that holds it.
enum ReadingLayout {
  static let baseFontSize = 22.0
  static let chromeHeight = 96.0

  static func textSize(textScale: Double, typeScale: Double) -> Double {
    baseFontSize * max(0.5, textScale) * max(0.5, typeScale)
  }

  static func minimumTextViewport(textScale: Double, typeScale: Double) -> Double {
    textSize(textScale: textScale, typeScale: typeScale) * 1.45
  }

  static func panelWidth(
    sceneWidth: Double, availableWidth: Double? = nil, dialogueScale: Double
  ) -> Double {
    let fraction = sceneWidth < 700 ? 0.86 : (sceneWidth < 1100 ? 0.75 : 0.70)
    let preferred = min(900, max(260, sceneWidth * fraction)) * clampScale(dialogueScale)
    let ceiling = min(sceneWidth - 24, (availableWidth ?? sceneWidth) - 24)
    return max(0, min(preferred, max(240, ceiling)))
  }

  static func panelHeight(
    sceneHeight: Double, availableHeight: Double? = nil, textScale: Double,
    typeScale: Double, lineSpacing: Double, dialogueScale: Double
  ) -> Double {
    let fontSize = textSize(textScale: textScale, typeScale: typeScale)
    let contentMinimum =
      chromeHeight + fontSize * 2.65 + max(0, lineSpacing) * 2
    let preferred = max(contentMinimum, sceneHeight * 0.26 * clampScale(dialogueScale))
    // Large text may use letterbox space; never hide the scene entirely.
    let ceiling = min(
      max(0, (availableHeight ?? sceneHeight) - 16),
      max(sceneHeight * 0.72, contentMinimum))
    return max(0, min(ceiling, preferred))
  }

  private static func clampScale(_ scale: Double) -> Double {
    guard scale.isFinite else { return 1 }
    return min(1.35, max(0.75, scale))
  }
}
