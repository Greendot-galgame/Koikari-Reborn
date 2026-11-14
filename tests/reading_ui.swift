// Compiles the actual UI input/layout policies, without game resources or SwiftUI.
import Foundation

@main struct ReadingUITests {
  static func main() {
    var assertions = 0
    func check(_ result: Bool, _ message: String) {
      precondition(result, message)
      assertions += 1
    }

    let ready = ReadingEligibility(
      active: true, hasScene: true, title: false, messageVisible: true)
    check(ready.canHide, "A settled reading page may hide its interface")
    check(!ReadingEligibility().canHide, "No scene must never enable hiding")
    let blockers: [(WritableKeyPath<ReadingEligibility, Bool>, Bool)] = [
      (\.active, false), (\.hasScene, false), (\.title, true), (\.finished, true),
      (\.blocked, true), (\.messageVisible, false), (\.hasChoices, true), (\.hasVideo, true),
      (\.hasCredits, true), (\.hasInterlude, true), (\.waiting, true), (\.modalOpen, true),
      (\.importing, true), (\.hasError, true),
    ]
    for (key, value) in blockers {
      var state = ready
      state[keyPath: key] = value
      check(!state.canHide, "Each blocking state independently denies hiding")
    }

    for hidden in [false, true] {
      for fast in [false, true] {
        for blocked in [false, true] {
          for advances in [false, true] {
            let intent = ReadingInputPolicy.nextIntent(
              interfaceHidden: hidden, fastForward: fast, blocked: blocked, canAdvance: advances)
            let expected: ReadingInputPolicy.Intent =
              blocked
              ? .none : hidden ? .reveal : fast ? .stopFastForward : advances ? .advance : .none
            check(intent == expected, "Inputs must consume reveal/stop before any advance")
          }
        }
      }
    }
    let first = ReadingInputPolicy.nextIntent(
      interfaceHidden: true, fastForward: false, blocked: false, canAdvance: false)
    check(first == .reveal, "Hidden pages reveal even though advancement is gated")
    let second = ReadingInputPolicy.nextIntent(
      interfaceHidden: false, fastForward: false, blocked: false, canAdvance: true)
    check(second == .advance, "Only a separate eligible input advances after reveal")

    for playback in [false, true] {
      for hidden in [false, true] {
        check(
          ReadingInputPolicy.permitsStoryTick(
            mediaPlaybackAllowed: playback, interfaceHidden: hidden)
            == (playback && !hidden), "Hidden UI and suspended media both block story ticking")
      }
    }

    for height in [0.0, 120, 247, 302, 720] {
      for compact in [false, true] {
        for textScale in [0.8, 1, 1.6] {
          for typeScale in [1.0, 1.5, 2.8] {
            let panel = ReadingLayout.panelHeight(
              sceneHeight: height, compact: compact, textScale: textScale, typeScale: typeScale)
            check(
              panel.isFinite && panel >= 0 && panel <= max(0, height - 16),
              "Reading panel stays inside the available safe viewport")
            if height >= 240 {
              check(
                panel - ReadingLayout.chromeHeight
                  >= ReadingLayout.minimumTextViewport(
                    textScale: textScale, typeScale: typeScale),
                "Supported landscape sizes retain at least one full large-text line")
            }
          }
        }
      }
    }
    let portraitPanel = ReadingLayout.panelHeight(
      sceneHeight: 180, availableHeight: 600, compact: true, textScale: 1.6, typeScale: 2.8)
    check(
      portraitPanel - ReadingLayout.chromeHeight
        >= ReadingLayout.minimumTextViewport(
          textScale: 1.6, typeScale: 2.8),
      "Narrow layouts can use letterbox space for accessible text")
    check(
      ReadingLayout.panelHeight(sceneHeight: 720, compact: false, textScale: 1, typeScale: 1)
        == ReadingLayout.panelHeight(sceneHeight: 1440, compact: false, textScale: 1, typeScale: 1),
      "Reading size must not grow simply because the canvas grows")
    print("\(assertions) reading UI policy/layout assertions passed (no rendered UI)")
  }
}
