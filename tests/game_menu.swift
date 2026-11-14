import Foundation

// The native check appends the actual Content routing methods as an extension
// of this harness. No reimplemented dispatcher, engine, UI, or user data is used.
@MainActor
final class GameplayMenuHarness {
  let game = GameplayMenuFakeGame()
  var gameplayMenu = GameplayMenuPresentation()
  var importing = false
  var showHistory = false
  var showDiagnostics = false
  var returnToTitle = false
  var showSaves = false
  var showSettings = false
  var newGame = false
  var showCollection = false
  var collectionIsReplay = false
  var saving = false

  var destinations: [Bool] {
    [showSaves, showSettings, showCollection, showDiagnostics, returnToTitle]
  }
}

@MainActor
final class GameplayMenuFakeGame {
  struct Call: Equatable {
    let name: String
    let blocked: Bool
  }
  var modalOpen = false {
    didSet {
      gateWrites.append(modalOpen)
      calls.append(Call(name: "gate", blocked: modalOpen))
    }
  }
  var importing = false
  var error: String?
  var fastForward = true
  var foreground = true
  var requiresUserResume = false
  var muted = false
  var savedListError: String?
  var collectionResult = true
  var collectionError: String?
  var calls: [Call] = []
  var gateWrites: [Bool] = []
  var stoppedFastForward = 0
  var collectionRequests: [Bool] = []
  var voicePlaybackStarts = 0

  var permitsPlayback: Bool {
    !modalOpen && !importing && error == nil && foreground && !requiresUserResume
  }
  func stopFastForward() {
    calls.append(Call(name: "stop", blocked: modalOpen))
    if fastForward { stoppedFastForward += 1 }
    fastForward = false
  }
  func resumePendingVideo() { calls.append(Call(name: "resume", blocked: modalOpen)) }
  func replayVoice() {
    calls.append(Call(name: "voice", blocked: modalOpen))
    if permitsPlayback { voicePlaybackStarts += 1 }
  }
  func toggleMute() {
    calls.append(Call(name: "mute", blocked: modalOpen))
    muted.toggle()
  }
  func reloadSaves() {
    calls.append(Call(name: "saves", blocked: modalOpen))
    if let savedListError { error = savedListError }
  }
  func reloadCollection(replay: Bool) -> Bool {
    calls.append(Call(name: "collection", blocked: modalOpen))
    collectionRequests.append(replay)
    if let collectionError { error = collectionError }
    return collectionResult
  }
  func count(_ name: String) -> Int { calls.filter { $0.name == name }.count }
}

@main
struct GameplayMenuChecks {
  @MainActor
  static func main() {
    var assertions = 0
    func expect(_ value: Bool, _ message: String) {
      precondition(value, message)
      assertions += 1
    }
    let actions = GameplayMenuAction.allCases
    expect(actions.count == 9, "Every existing gameplay menu action is represented")
    expect(Set(actions.map(\.rawValue)).count == 9, "Stable action identifiers are unique")

    var idle = GameplayMenuPresentation()
    expect(!idle.isPresented && !idle.blocksGameplay, "Initial phase is closed")
    idle.dismiss()
    expect(idle.didDismiss() == nil && !idle.blocksGameplay, "Idle callbacks are inert")
    for action in actions {
      var state = GameplayMenuPresentation()
      expect(!state.select(action), "Cannot select from a closed menu")
      expect(state.present(), "First open succeeds")
      expect(state.isPresented && state.blocksGameplay, "Opening immediately owns the blocker")
      expect(!state.present(), "Cannot present twice")
      expect(state.select(action), "First selection queues an action")
      expect(!state.isPresented && state.blocksGameplay, "Dismissing still owns the blocker")
      expect(!state.select(.settings), "Second tap does not replace the selected action")
      expect(!state.present(), "Cannot reopen while dismissal is in flight")
      state.dismiss()
      state.dismiss()
      expect(state.blocksGameplay, "Binding false and repeated dismissal do not drop the gate")
      expect(state.didDismiss() == action, "Actual dismissal consumes the original action")
      expect(!state.blocksGameplay && !state.isPresented, "Consumed menu becomes closed")
      expect(state.didDismiss() == nil, "Dismissal callback does not dispatch twice")
      expect(state.present(), "A subsequent menu can open")
      state.dismiss()
      expect(!state.select(action), "Cannot select after a cancel/swipe dismissal starts")
      expect(state.blocksGameplay && !state.isPresented, "Cancel blocks until onDismiss")
      expect(state.didDismiss() == nil && !state.blocksGameplay, "Cancelled menu has no action")
    }

    for action in actions {
      let h = GameplayMenuHarness()
      h.openGameplayMenu()
      expect(h.gameplayMenu.isPresented && h.game.modalOpen, "Production open pauses synchronously")
      expect(!h.game.fastForward && h.game.stoppedFastForward == 1, "Opening stops fast-forward")
      expect(
        h.game.calls.first == GameplayMenuFakeGame.Call(name: "gate", blocked: true),
        "Gate precedes stop/refresh work")
      expect(!h.game.permitsPlayback, "Opening suppresses media playback")
      expect(h.gameplayMenu.select(action), "Production selection queues")
      h.synchronizeModalState()
      // The real sheet binding writes false when selection initiates dismissal.
      h.gameplayMenu.dismiss()
      h.synchronizeModalState()
      h.openGameplayMenu()
      expect(!h.destinations.contains(true), "No destination opens before UIKit onDismiss")
      expect(
        h.game.count("saves") == 0 && h.game.count("collection") == 0, "No early catalog reads")
      expect(h.game.count("voice") == 0 && h.game.count("mute") == 0, "No early non-modal actions")
      expect(h.game.gateWrites == [true], "No pre-dismissal unblock or duplicate gate write")
      h.finishGameplayMenu()
      expect(!h.gameplayMenu.blocksGameplay, "Menu ownership ends after actual dismissal")
      let isPlaybackAction = action == .replayVoice || action == .toggleMute
      if isPlaybackAction {
        expect(!h.destinations.contains(true), "Playback actions do not present a sheet")
        expect(h.game.gateWrites == [true, false], "Non-modal action releases the gate")
        let name = action == .replayVoice ? "voice" : "mute"
        expect(
          h.game.calls.last == GameplayMenuFakeGame.Call(name: name, blocked: false),
          "Playback action runs after unblocking")
        expect(h.game.count(name) == 1, "Non-modal action runs once")
        expect(
          h.game.voicePlaybackStarts == (action == .replayVoice ? 1 : 0), "Voice can actually start"
        )
        expect(h.game.muted == (action == .toggleMute), "Mute toggles exactly once")
      } else {
        expect(h.destinations.filter { $0 }.count == 1, "Exactly one destination is presented")
        expect(h.game.modalOpen && h.game.gateWrites == [true], "No unpaused handoff frame")
        expect(h.game.count("resume") == 0, "Pending video cannot advance during handoff")
        expect(h.showSaves == (action == .save || action == .load), "Save/load routing")
        expect(h.showSettings == (action == .settings), "Settings routing")
        expect(h.showCollection == (action == .gallery || action == .replay), "Library routing")
        expect(h.showDiagnostics == (action == .diagnostics), "Diagnostics routing")
        expect(h.returnToTitle == (action == .returnToTitle), "Title confirmation routing")
        if h.showSaves {
          expect(h.saving == (action == .save), "Save and load modes remain distinct")
          expect(
            h.game.calls.contains(GameplayMenuFakeGame.Call(name: "saves", blocked: true)),
            "Save catalog loads while paused")
          expect(h.game.count("saves") == 1, "Save catalog loaded once")
        }
        if h.showCollection {
          expect(
            h.collectionIsReplay == (action == .replay), "Gallery and replay modes remain distinct")
          expect(h.game.collectionRequests == [action == .replay], "Correct library loaded once")
          expect(
            h.game.calls.contains(GameplayMenuFakeGame.Call(name: "collection", blocked: true)),
            "Library loads while paused")
        }
      }
      let before = ["saves", "collection", "voice", "mute"].map { h.game.count($0) }
      h.finishGameplayMenu()
      expect(
        before == ["saves", "collection", "voice", "mute"].map { h.game.count($0) },
        "Repeated onDismiss never repeats an action")
      expect(h.game.stoppedFastForward == 1, "Only one running fast-forward session was stopped")
    }

    for blocker in 0..<11 {
      let h = GameplayMenuHarness()
      switch blocker {
      case 0: h.importing = true
      case 1: h.showHistory = true
      case 2: h.showDiagnostics = true
      case 3: h.returnToTitle = true
      case 4: h.showSaves = true
      case 5: h.showSettings = true
      case 6: h.newGame = true
      case 7: h.showCollection = true
      case 8: h.game.error = "synthetic error"
      case 9: h.game.importing = true
      default: h.game.modalOpen = true
      }
      let before = h.game.calls
      h.openGameplayMenu()
      expect(!h.gameplayMenu.blocksGameplay, "Existing blockers reject menu opening")
      expect(h.game.calls == before, "Rejected open does not mutate gameplay")
    }

    let cancel = GameplayMenuHarness()
    cancel.openGameplayMenu()
    cancel.gameplayMenu.dismiss()
    cancel.synchronizeModalState()
    expect(cancel.game.modalOpen, "Swipe dismissal remains blocked until callback")
    cancel.finishGameplayMenu()
    expect(
      !cancel.game.modalOpen && !cancel.destinations.contains(true),
      "Cancel releases without destination")
    expect(
      cancel.game.count("saves") == 0 && cancel.game.count("voice") == 0,
      "Cancel has no selected action")

    for error in [nil, "synthetic collection failure"] as [String?] {
      let h = GameplayMenuHarness()
      h.game.collectionResult = false
      h.game.collectionError = error
      h.openGameplayMenu()
      _ = h.gameplayMenu.select(.gallery)
      h.finishGameplayMenu()
      expect(!h.showCollection, "Failed collection read never presents its sheet")
      expect(h.game.error == error, "Collection error is preserved")
      expect(h.game.modalOpen == (error != nil), "Failure releases or transfers to error blocker")
      expect(h.game.count("resume") == (error == nil ? 1 : 0), "Error path cannot resume media")
    }

    for action in actions {
      let h = GameplayMenuHarness()
      h.openGameplayMenu()
      _ = h.gameplayMenu.select(action)
      h.game.error = "late synthetic error"
      h.finishGameplayMenu()
      expect(!h.destinations.contains(true), "Late error cancels queued destination")
      expect(h.game.modalOpen && h.game.gateWrites == [true], "Late error retains the blocker")
      expect(
        ["saves", "collection", "voice", "mute"].allSatisfy { h.game.count($0) == 0 },
        "Late error does not execute stale action")
    }
    for importInProgress in [true, false] {
      let h = GameplayMenuHarness()
      h.openGameplayMenu()
      _ = h.gameplayMenu.select(.settings)
      if importInProgress { h.game.importing = true } else { h.showHistory = true }
      h.finishGameplayMenu()
      expect(!h.showSettings, "New importer or modal cancels queued sheet")
      expect(!h.game.permitsPlayback, "Independent blocker still suppresses playback")
    }
    for action in [GameplayMenuAction.save, .load] {
      let h = GameplayMenuHarness()
      h.game.savedListError = "synthetic save-list failure"
      h.openGameplayMenu()
      _ = h.gameplayMenu.select(action)
      h.finishGameplayMenu()
      expect(!h.showSaves, "Save-list error does not compete with root error alert")
      expect(h.game.modalOpen && h.game.error != nil, "Save-list failure stays blocked")
    }
    for background in [true, false] {
      let h = GameplayMenuHarness()
      h.openGameplayMenu()
      _ = h.gameplayMenu.select(.replayVoice)
      h.game.foreground = !background
      h.game.requiresUserResume = !background
      h.finishGameplayMenu()
      expect(
        h.game.voicePlaybackStarts == 0, "Dismissal cannot override background/interruption gate")
    }
    print(
      "\(assertions) gameplay menu state/routing assertions passed (production routing, synthetic Game; no rendered UI)"
    )
  }
}
