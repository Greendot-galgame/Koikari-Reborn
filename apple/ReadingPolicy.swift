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

  var canHide: Bool {
    active && hasScene && !title && !finished && !blocked && messageVisible
      && !hasChoices && !hasVideo && !hasCredits && !hasInterlude && !waiting
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

enum ReadingLayout {
  // Reserve space for the speaker/actions and let long or large text scroll.
  // Unlike the scene, reading typography does not scale with the 16:9 canvas.
  static let chromeHeight = 80.0

  static func minimumTextViewport(textScale: Double, typeScale: Double) -> Double {
    18 * textScale * typeScale * 1.5
  }

  static func panelHeight(
    sceneHeight: Double, availableHeight: Double? = nil, compact: Bool,
    textScale: Double, typeScale: Double
  ) -> Double {
    let base = compact ? 154.0 : 190.0
    let minimum = chromeHeight + minimumTextViewport(textScale: textScale, typeScale: typeScale)
    let preferred = max(minimum, 88 + (base - 88) * max(1, textScale) * max(1, typeScale))
    // Large text may use letterbox space; never squeeze its viewport to a fraction
    // of the scene. Speaker metadata scrolls with the text, actions stay reachable.
    let ceiling = min(
      max(0, (availableHeight ?? sceneHeight) - 16), max(sceneHeight * 0.72, minimum))
    return max(0, min(ceiling, preferred))
  }
}
