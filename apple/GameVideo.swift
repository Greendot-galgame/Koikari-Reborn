import AVFoundation
import SwiftUI

private struct GameVideoRequest: Equatable {
  let url: URL
  let sequence: UInt64
}

private final class GameVideoResources {
  var player: AVPlayer?
  var observers: [NSObjectProtocol] = []
  var status: NSKeyValueObservation?

  func clear() {
    status?.invalidate()
    status = nil
    for observer in observers { NotificationCenter.default.removeObserver(observer) }
    observers.removeAll()
    player?.pause()
    player?.currentItem?.cancelPendingSeeks()
    player?.replaceCurrentItem(with: nil)
    player = nil
  }

  deinit { clear() }
}

@MainActor
struct GameVideoView: View {
  let url: URL
  let sequence: UInt64
  let looping: Bool
  let skippable: Bool
  let muted: Bool
  let volume: Double
  let onCompleted: (UInt64) -> Void
  let onFailure: (String) -> Void

  @Environment(\.scenePhase) private var phase
  @StateObject private var playback = GameVideoPlayback()

  init(
    url: URL, sequence: UInt64, looping: Bool, skippable: Bool,
    muted: Bool, volume: Double, onCompleted: @escaping (UInt64) -> Void,
    onFailure: @escaping (String) -> Void
  ) {
    self.url = url
    self.sequence = sequence
    self.looping = looping
    self.skippable = skippable
    self.muted = muted
    self.volume = volume
    self.onCompleted = onCompleted
    self.onFailure = onFailure
  }

  var body: some View {
    ZStack(alignment: .bottomTrailing) {
      Color.black
      GameVideoSurface(player: playback.player)
      VStack(alignment: .trailing) {
        if playback.needsResume {
          Button("继续影片") { playback.resumeAfterInterruption() }
            .accessibilityIdentifier("resume-video")
        }
        if skippable {
          Button("跳过影片") { playback.skip() }
            .accessibilityIdentifier("skip-video")
        }
      }.buttonStyle(.bordered).controlSize(.large).padding(16)
    }
    .contentShape(Rectangle())
    .onTapGesture {}
    .onAppear { configure() }
    .onChange(of: GameVideoRequest(url: url, sequence: sequence)) { _ in configure() }
    .onChange(of: looping) { _ in updateOptions() }
    .onChange(of: skippable) { _ in updateOptions() }
    .onChange(of: muted) { _ in updateOptions() }
    .onChange(of: volume) { _ in updateOptions() }
    .onChange(of: phase) { playback.setActive($0 == .active) }
    .onDisappear { playback.stop() }
  }

  private func configure() {
    playback.setActive(phase == .active)
    playback.load(
      url: url, sequence: sequence, looping: looping, skippable: skippable,
      muted: muted, volume: volume, onCompleted: onCompleted, onFailure: onFailure)
  }

  private func updateOptions() {
    playback.setOptions(looping: looping, skippable: skippable, muted: muted, volume: volume)
  }
}

@MainActor
final class GameVideoPlayback: ObservableObject {
  @Published private(set) var player: AVPlayer?
  @Published private(set) var needsResume = false
  private let resources = GameVideoResources()
  private var request: GameVideoRequest?
  private var generation = UUID()
  private var finished = false
  private var active = false
  private var interrupted = false
  private var sessionNeedsActivation = false
  private var looping = false
  private var skippable = false
  private var seeking = false
  private var completed: ((UInt64) -> Void)?
  private var failure: ((String) -> Void)?

  func load(
    url: URL, sequence: UInt64, looping: Bool, skippable: Bool,
    muted: Bool, volume: Double, onCompleted: @escaping (UInt64) -> Void,
    onFailure: @escaping (String) -> Void
  ) {
    let next = GameVideoRequest(url: url, sequence: sequence)
    if request == next {
      completed = onCompleted
      failure = onFailure
      setOptions(looping: looping, skippable: skippable, muted: muted, volume: volume)
      return
    }
    stop()
    request = next
    completed = onCompleted
    failure = onFailure
    let token = generation
    guard url.isFileURL, url.pathExtension.lowercased() == "mp4" else {
      fail("Video requires a local exported MP4 file.", token: token)
      return
    }
    let item = AVPlayerItem(url: url)
    let player = AVPlayer(playerItem: item)
    player.actionAtItemEnd = .pause
    self.player = player
    resources.player = player
    setOptions(looping: looping, skippable: skippable, muted: muted, volume: volume)

    let center = NotificationCenter.default
    resources.observers.append(
      center.addObserver(
        forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main
      ) {
        [weak self] _ in
        Task { @MainActor [weak self] in self?.ended(token: token) }
      })
    resources.observers.append(
      center.addObserver(
        forName: AVPlayerItem.failedToPlayToEndTimeNotification, object: item, queue: .main
      ) { [weak self] _ in
        Task { @MainActor [weak self] in self?.fail("Video playback failed.", token: token) }
      })
    resources.status = item.observe(\.status, options: [.initial, .new]) { [weak self] item, _ in
      let failed = item.status == .failed
      Task { @MainActor [weak self] in
        guard let self, self.generation == token, !self.finished else { return }
        if failed { self.fail("Video could not be opened or decoded.", token: token) }
      }
    }
    #if os(iOS)
      resources.observers.append(
        center.addObserver(
          forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] notification in
          let type = (notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? NSNumber)?
            .uintValue
          let options =
            (notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? NSNumber)?
            .uintValue ?? 0
          Task { @MainActor [weak self] in
            guard let self, self.generation == token, !self.finished else { return }
            if type == AVAudioSession.InterruptionType.began.rawValue {
              self.interrupted = true
              self.needsResume = false
              self.sessionNeedsActivation = true
              self.player?.pause()
            } else if type == AVAudioSession.InterruptionType.ended.rawValue {
              self.interrupted =
                !AVAudioSession.InterruptionOptions(rawValue: options).contains(.shouldResume)
              self.needsResume = self.interrupted
              self.resumeIfAllowed()
            }
          }
        })
    #endif
    resumeIfAllowed()
  }

  func setOptions(looping: Bool, skippable: Bool, muted: Bool, volume: Double) {
    self.looping = looping
    self.skippable = skippable
    player?.isMuted = muted
    player?.volume = Float(volume.isFinite ? min(1, max(0, volume)) : 0)
  }

  func setActive(_ active: Bool) {
    self.active = active
    if active {
      resumeIfAllowed()
    } else {
      player?.pause()
    }
  }

  private func resumeIfAllowed() {
    guard active, !interrupted, !finished, !seeking else { return }
    #if os(iOS)
      if sessionNeedsActivation {
        do {
          try AVAudioSession.sharedInstance().setActive(true)
          sessionNeedsActivation = false
        } catch {
          fail("Video audio session could not resume.", token: generation)
          return
        }
      }
    #endif
    player?.play()
  }

  func resumeAfterInterruption() {
    guard active, needsResume, !finished else { return }
    interrupted = false
    needsResume = false
    resumeIfAllowed()
  }

  func skip() {
    guard skippable else { return }
    complete(token: generation)
  }

  private func ended(token: UUID) {
    guard generation == token, !finished, !seeking, let player else { return }
    guard looping else {
      complete(token: token)
      return
    }
    seeking = true
    player.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] success in
      Task { @MainActor [weak self] in
        guard let self, self.generation == token, !self.finished else { return }
        self.seeking = false
        if success {
          self.resumeIfAllowed()
        } else {
          self.fail("Video loop seek failed.", token: token)
        }
      }
    }
  }

  private func complete(token: UUID) {
    guard generation == token, !finished, let sequence = request?.sequence else { return }
    finished = true
    let callback = completed
    releasePlayer()
    completed = nil
    failure = nil
    callback?(sequence)
  }

  private func fail(_ message: String, token: UUID) {
    guard generation == token, !finished else { return }
    finished = true
    let callback = failure
    releasePlayer()
    completed = nil
    failure = nil
    callback?(message)
  }

  func stop() {
    generation = UUID()
    releasePlayer()
    request = nil
    completed = nil
    failure = nil
    finished = false
    interrupted = false
    sessionNeedsActivation = false
    seeking = false
  }

  private func releasePlayer() {
    resources.clear()
    player = nil
    needsResume = false
  }
}

#if os(macOS)
  import AppKit

  @MainActor
  private final class GameVideoSurfaceView: NSView {
    let videoLayer = AVPlayerLayer()

    override init(frame frameRect: NSRect) {
      super.init(frame: frameRect)
      wantsLayer = true
      videoLayer.videoGravity = .resizeAspect
      layer = videoLayer
    }

    required init?(coder: NSCoder) { nil }
  }

  private struct GameVideoSurface: NSViewRepresentable {
    let player: AVPlayer?

    func makeNSView(context: Context) -> GameVideoSurfaceView {
      let view = GameVideoSurfaceView(frame: .zero)
      view.videoLayer.player = player
      return view
    }

    func updateNSView(_ view: GameVideoSurfaceView, context: Context) {
      view.videoLayer.player = player
    }

    static func dismantleNSView(_ view: GameVideoSurfaceView, coordinator: ()) {
      view.videoLayer.player = nil
    }
  }
#else
  import UIKit

  @MainActor
  private final class GameVideoSurfaceView: UIView {
    override class var layerClass: AnyClass { AVPlayerLayer.self }
    var videoLayer: AVPlayerLayer { layer as! AVPlayerLayer }
  }

  private struct GameVideoSurface: UIViewRepresentable {
    let player: AVPlayer?

    func makeUIView(context: Context) -> GameVideoSurfaceView {
      let view = GameVideoSurfaceView(frame: .zero)
      view.videoLayer.videoGravity = .resizeAspect
      view.videoLayer.player = player
      return view
    }

    func updateUIView(_ view: GameVideoSurfaceView, context: Context) {
      view.videoLayer.player = player
    }

    static func dismantleUIView(_ view: GameVideoSurfaceView, coordinator: ()) {
      view.videoLayer.player = nil
    }
  }
#endif
