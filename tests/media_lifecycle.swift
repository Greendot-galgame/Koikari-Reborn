// No UI, AVAudioSession, player, filesystem-save or resource access.
// Compile with the marked MEDIA LIFECYCLE POLICY region extracted verbatim from
// apple/KoikariApp.swift. See docs/TESTING.md for the exact commands.
@main @MainActor struct MediaLifecycleTests {
  private static var checks = 0
  private static func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    precondition(condition(), message)
    checks += 1
  }

  static func main() {
    var policy = MediaLifecyclePolicy()
    expect(policy.permitsPlayback, "Initial active scene may play")
    policy.setForeground(false)
    expect(!policy.permitsPlayback, "Background pauses")
    policy.setForeground(true)
    expect(policy.permitsPlayback, "Ordinary foreground may resume")
    policy.setBlocked(true)
    expect(!policy.permitsPlayback, "Modal/error/import gate pauses")
    policy.setBlocked(false)
    expect(policy.permitsPlayback, "Closing an ordinary modal may resume")

    policy.endInterruption(shouldResume: false)
    expect(
      policy.requiresUserResume, "A denied end is safe even if the begin notification was missed")
    expect(policy.requestUserResume(), "Missed begin still requires explicit active consent")
    expect(policy.permitsPlayback, "Only fresh consent releases denied unmatched end")

    policy.beginInterruption()
    expect(!policy.permitsPlayback, "Interruption pauses")
    expect(!policy.requestUserResume(), "User cannot bypass a live OS interruption")
    policy.setForeground(false)
    policy.setForeground(true)
    expect(!policy.permitsPlayback, "Foreground cannot end a live interruption")
    policy.endInterruption(shouldResume: true)
    expect(policy.permitsPlayback, "OS shouldResume permits automatic active resume")

    policy.beginInterruption()
    policy.endInterruption(shouldResume: false)
    expect(policy.resumeReason == .interruption, "Absent shouldResume requires consent")
    for _ in 0..<4 {
      policy.setForeground(false)
      expect(!policy.permitsPlayback, "Denied resume remains paused in background")
      policy.setForeground(true)
      expect(!policy.permitsPlayback, "Foreground never erases denied resume")
    }
    policy.endInterruption(shouldResume: true)
    expect(!policy.permitsPlayback, "Duplicate end cannot grant consent")
    policy.setBlocked(true)
    expect(!policy.canRequestUserResume, "Resume control disabled while blocked")
    expect(!policy.requestUserResume(), "Blocked clicks do not bank consent")
    policy.setBlocked(false)
    expect(!policy.permitsPlayback, "Closing modal still needs a fresh explicit click")
    expect(policy.requestUserResume(), "Active unblocked user action grants consent")
    expect(policy.permitsPlayback, "Explicit consent releases playback gate")

    policy.beginInterruption()
    policy.setForeground(false)
    policy.endInterruption(shouldResume: true)
    expect(!policy.permitsPlayback, "OS shouldResume does not bypass background")
    policy.setBlocked(true)
    policy.setForeground(true)
    expect(!policy.permitsPlayback, "OS shouldResume does not bypass modal")
    policy.setBlocked(false)
    expect(policy.permitsPlayback, "Allowed OS resume waits for all gates")

    policy.beginInterruption()
    policy.setForeground(false)
    policy.endInterruption(shouldResume: false)
    expect(!policy.requestUserResume(), "Inactive click does not bank consent")
    policy.setForeground(true)
    expect(!policy.permitsPlayback, "Denied background end remains latched")
    expect(policy.requestUserResume(), "Denied background end needs an active click")

    policy.loseOutputRoute()
    expect(policy.resumeReason == .routeLost, "Headphone loss records an explicit pause")
    expect(!policy.permitsPlayback, "Headphone loss never automatically uses speaker")
    policy.setForeground(false)
    policy.setForeground(true)
    expect(!policy.permitsPlayback, "Foreground cannot clear headphone pause")
    policy.setBlocked(true)
    expect(!policy.requestUserResume(), "Headphone resume cannot bypass modal")
    policy.setBlocked(false)
    expect(!policy.permitsPlayback, "Closing modal cannot clear headphone pause")
    expect(policy.requestUserResume(), "User may accept current output route")
    expect(policy.permitsPlayback, "Accepted output route releases shared AV gate")

    policy.beginInterruption()
    policy.loseOutputRoute()
    expect(!policy.requestUserResume(), "Route consent waits for interruption to end")
    policy.endInterruption(shouldResume: true)
    expect(policy.resumeReason == .routeLost, "OS shouldResume cannot clear route-loss consent")
    expect(!policy.permitsPlayback, "Route loss plus interruption stays silent")
    expect(policy.requestUserResume(), "Explicit consent resolves surviving route pause")
    policy.loseOutputRoute()
    policy.beginInterruption()
    policy.endInterruption(shouldResume: false)
    expect(policy.resumeReason == .routeLost, "Denied interruption preserves route warning")
    expect(policy.requestUserResume(), "Fresh consent resolves combined pause")

    policy.activationFailed()
    expect(!policy.permitsPlayback, "Session activation failure holds playback")
    expect(policy.resumeReason == .activationFailed, "Failed activation is visible")
    policy.setForeground(false)
    policy.setForeground(true)
    expect(!policy.permitsPlayback, "Foreground does not retry a failed activation without consent")
    expect(policy.requestUserResume(), "User may retry activation explicitly")
    policy.activationFailed()
    expect(!policy.permitsPlayback, "Failed retry re-latches instead of leaking playback")
    expect(policy.requestUserResume(), "Another retry requires another explicit action")

    // Exhaustive gate matrix: scene × modal/error/import × interruption × consent.
    // The same gate is injected into GameVideoView and guards audio/clock/EOF drain.
    for foreground in [false, true] {
      for blocked in [false, true] {
        for interrupted in [false, true] {
          for needsConsent in [false, true] {
            var candidate = MediaLifecyclePolicy()
            candidate.setForeground(foreground)
            candidate.setBlocked(blocked)
            if interrupted { candidate.beginInterruption() }
            if needsConsent { candidate.loseOutputRoute() }
            let eligible = foreground && !blocked && !interrupted
            expect(
              candidate.permitsPlayback == (eligible && !needsConsent), "Combined playback gate")
            expect(
              candidate.canRequestUserResume == (eligible && needsConsent),
              "Combined explicit-resume gate")
            expect(
              candidate.requestUserResume() == (eligible && needsConsent),
              "Only eligible fresh action consumes consent")
            expect(
              candidate.requiresUserResume == (needsConsent && !eligible),
              "Rejected action preserves consent latch")
            expect(
              candidate.permitsPlayback == eligible,
              "Explicit resume cannot bypass any remaining gate")
          }
        }
      }
    }
    print(
      "Media lifecycle: \(checks) deterministic policy assertions passed; no UI or playback exercised"
    )
  }
}
