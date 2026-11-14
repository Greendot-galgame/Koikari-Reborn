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
    check(ready.canOfferHide, "A settled reading page offers hiding")
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
    for (key, value) in blockers where key != \.messageVisible && key != \.waiting {
      var state = ready
      state[keyPath: key] = value
      check(!state.canOfferHide, "The toolbar stays stable only for settled, eligible pages")
    }
    var waiting = ready
    waiting.waiting = true
    check(!waiting.canHide, "A waiting page cannot pause the clock by hiding")
    check(
      waiting.canOfferHide,
      "A transient wait must not disable the hide control and make it blink")
    var emptyMessage = ready
    emptyMessage.messageVisible = false
    check(!emptyMessage.canHide, "No visible message layer means nothing to hide")
    check(
      emptyMessage.canOfferHide,
      "A momentarily empty message layer must not flicker the hide control")

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

    for speed in [1.0, 1.5, 2, 3, 4, 5] {
      let interval = FastForwardPolicy.intervalNanoseconds(speed: speed)
      check(
        interval <= FastForwardPolicy.baseInterval,
        "Fast-forward never waits longer than the original cadence")
      check(
        abs(Double(interval) - Double(FastForwardPolicy.baseInterval) / speed) <= 1,
        "The configured multiplier sets the interval")
    }
    check(
      FastForwardPolicy.intervalNanoseconds(speed: 0) == FastForwardPolicy.baseInterval,
      "Invalid low speed clamps to the original cadence")
    check(
      FastForwardPolicy.intervalNanoseconds(speed: 9) == 50_000_000,
      "Invalid high speed clamps to the fastest cadence")
    check(
      FastForwardPolicy.intervalNanoseconds(speed: .infinity) == FastForwardPolicy.baseInterval,
      "Non-finite speed falls back to the original cadence")

    for height in [0.0, 120, 247, 302, 720] {
      for textScale in [0.8, 1, 1.6] {
        for typeScale in [1.0, 1.5, 2.8] {
          for sizeScale in [0.8, 1, 1.25] {
            let panel = ReadingLayout.panelHeight(
              sceneHeight: height, textScale: textScale, typeScale: typeScale,
              lineSpacing: 5, dialogueScale: sizeScale)
            check(
              panel.isFinite && panel >= 0 && panel <= max(0, height - 16),
              "Reading panel stays inside the available viewport")
            if height >= 300 {
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
      sceneHeight: 180, availableHeight: 600, textScale: 1.6, typeScale: 2.8,
      lineSpacing: 5, dialogueScale: 1.25)
    check(
      portraitPanel - ReadingLayout.chromeHeight
        >= ReadingLayout.minimumTextViewport(textScale: 1.6, typeScale: 2.8),
      "Narrow layouts can use letterbox space for accessible text")
    for width in [440.0, 667, 1280, 1920] {
      for sizeScale in [0.8, 1, 1.25] {
        let panelWidth = ReadingLayout.panelWidth(
          sceneWidth: width, dialogueScale: sizeScale)
        check(
          panelWidth.isFinite && panelWidth > 0 && panelWidth <= width - 24,
          "Dialogue width stays inside the canvas")
      }
      if width >= 700 {
        check(
          ReadingLayout.panelWidth(sceneWidth: width, dialogueScale: 1) < width * 0.80,
          "The default dialogue box is narrower than the old near-full-width box")
      }
    }
    check(
      ReadingLayout.panelWidth(sceneWidth: 1280, dialogueScale: 1)
        == ReadingLayout.panelWidth(sceneWidth: 1280, dialogueScale: 1),
      "Dialogue geometry is a pure function of canvas and user preference")
    print("\(assertions) reading UI policy/layout assertions passed (no rendered UI)")
  }
}
