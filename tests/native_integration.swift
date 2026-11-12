// Compile alongside the actual StoryChoice/StoryFlow and AUDIO FAILURE POLICY
// regions extracted from apple/KoikariApp.swift, without replacing native gates.
import Foundation

@main struct NativeIntegrationTests {
  static func main() throws {
    let decoder = JSONDecoder()
    for (json, allowed) in [
      (#"{"wait_ms":0}"#, true),
      (#"{"wait_ms":100,"wait_skippable":true}"#, true),
      (#"{"wait_ms":100,"wait_skippable":false}"#, false),
      (#"{"wait_ms":100}"#, false),
    ] {
      let flow = try decoder.decode(StoryFlow.self, from: Data(json.utf8))
      precondition(flow.acceptsAdvanceWait == allowed)
    }
    var failures = AudioFailurePolicy()
    failures.record("corrupt")
    precondition(!failures.mayDecode("corrupt"))
    precondition(failures.needsAcknowledgement("corrupt"))
    precondition(failures.mayDecode("healthy"))
    failures.skip("corrupt")
    precondition(!failures.mayDecode("corrupt"))
    precondition(!failures.needsAcknowledgement("corrupt"))
    failures.retry("corrupt")
    precondition(failures.mayDecode("corrupt"))
    precondition(!failures.needsAcknowledgement("corrupt"))
    for (muted, volume, outputFailed, allowed): (Bool, Float, Bool, Bool) in [
      (false, 1, false, true),
      (true, 1, false, false),
      (false, 0, false, false),
      (true, 0, false, false),
      (false, 1, true, false),
      (false, .nan, false, false),
      (false, .infinity, false, false),
      (false, -1, false, false),
    ] {
      precondition(
        AudioPlaybackGate.allows(muted: muted, volume: volume, outputFailed: outputFailed)
          == allowed)
    }
    print("19 native integration assertions passed")
  }
}
