import SwiftUI

struct MessageGlyphState: Decodable {
  struct Frame: Decodable {
    var clip: [UInt32]
    var durationMs: UInt64
    enum CodingKeys: String, CodingKey {
      case clip
      case durationMs = "duration_ms"
    }
  }
  struct Timeline: Decodable {
    var frames: [Frame]
    var durationMs: UInt64
    enum CodingKeys: String, CodingKey {
      case frames
      case durationMs = "duration_ms"
    }
  }
  var asset: String
  var width: UInt32
  var height: UInt32
  var timeline: Timeline
}
struct MessageGlyphView: View {
  let glyph: MessageGlyphState
  let sheet: Image
  let elapsed: UInt64
  var body: some View {
    Canvas { context, _ in
      var time = elapsed % max(1, glyph.timeline.durationMs)
      var frame = glyph.timeline.frames[0]
      for candidate in glyph.timeline.frames {
        frame = candidate
        if time < candidate.durationMs { break }
        time -= candidate.durationMs
      }
      let clip = frame.clip
      context.scaleBy(x: 32 / CGFloat(clip[2]), y: 32 / CGFloat(clip[3]))
      context.clip(to: Path(CGRect(x: 0, y: 0, width: CGFloat(clip[2]), height: CGFloat(clip[3]))))
      context.draw(
        sheet,
        in: CGRect(
          x: -CGFloat(clip[0]), y: -CGFloat(clip[1]), width: CGFloat(glyph.width),
          height: CGFloat(glyph.height)))
    }.frame(width: 32, height: 32).allowsHitTesting(false).accessibilityLabel("继续阅读")
  }
}

struct CreditCommand: Decodable {
  var name: String
  var args: [String: String]
}
struct CreditsState: Decodable {
  var commands: [CreditCommand]
  var images: [String]
  var durationMs: UInt64
  var elapsedMs: UInt64
  var left: UInt32
  var width: UInt32
  var startDisp: UInt32
  var endDisp: UInt32
  enum CodingKeys: String, CodingKey {
    case commands, images, left, width
    case durationMs = "duration_ms"
    case elapsedMs = "elapsed_ms"
    case startDisp = "start_disp"
    case endDisp = "end_disp"
  }
}

/// Draw the ordered staff-roll command stream in the game's 2560×1440 space.
/// Scroll endpoints follow ExtStaffroll.start(time,startdisp,enddisp).
struct CreditsView: View {
  let credits: CreditsState
  let images: [String: Image]
  private struct Row {
    var text: Text = Text("")
    var image: String?
    var size: CGSize = .zero
    var fontSize: CGFloat = 28
    var spacing: CGFloat = 0
    var centered = false
    var hasText = false
  }
  private var rows: [Row] {
    var result: [Row] = []
    var row = Row()
    var fontSize: CGFloat = 28
    var color = Color.white
    var centered = false
    var spacing: CGFloat = 0
    func blank() -> Row {
      Row(fontSize: fontSize, spacing: spacing, centered: centered)
    }
    for command in credits.commands {
      let args = command.args
      switch command.name.lowercased() {
      case "font":
        if let value = args["size"].flatMap(Double.init) { fontSize = CGFloat(value) }
        if let value = args["color"] {
          let hex = value.lowercased().replacingOccurrences(of: "0x", with: "")
          if let rgb = UInt32(hex, radix: 16) {
            color = Color(
              red: Double((rgb >> 16) & 255) / 255,
              green: Double((rgb >> 8) & 255) / 255,
              blue: Double(rgb & 255) / 255)
          }
        }
      case "style":
        if let align = args["align"] { centered = align == "center" }
        if let value = args["linespacing"].flatMap(Double.init) { spacing = CGFloat(value) }
      case "resetstyle":
        centered = false
        spacing = 0
      case "text":
        row.text =
          row.text + Text(args["text"] ?? "").font(.system(size: fontSize)).foregroundColor(color)
        row.fontSize = max(row.fontSize, fontSize)
        row.centered = centered
        row.spacing = spacing
        row.hasText = true
      case "r":
        result.append(row)
        row = blank()
      case "graph":
        if row.hasText { result.append(row) }
        var image = blank()
        image.image = args["asset"]
        image.size = CGSize(
          width: Double(args["width"] ?? "0") ?? 0,
          height: Double(args["height"] ?? "0") ?? 0)
        result.append(image)
        row = blank()
      default: break  // Engine accepts and validates the bounded credits grammar.
      }
    }
    if row.hasText { result.append(row) }
    return result
  }
  var body: some View {
    Canvas { context, size in
      let scale = min(size.width / 2560, size.height / 1440)
      context.scaleBy(x: scale, y: scale)
      let width = CGFloat(max(1, credits.width))
      let rows = rows
      let resolved = rows.map { context.resolve($0.text) }
      let heights = rows.indices.map { index in
        rows[index].image != nil
          ? rows[index].size.height
          : max(
            rows[index].fontSize,
            resolved[index].measure(in: CGSize(width: width, height: .infinity)).height)
      }
      let total = rows.indices.reduce(CGFloat(0)) { $0 + heights[$1] + rows[$1].spacing }
      let fraction =
        credits.durationMs == 0 ? 1 : min(1, Double(credits.elapsedMs) / Double(credits.durationMs))
      var y = 1440 - CGFloat(credits.startDisp)
      y += (-total + CGFloat(credits.endDisp) - y) * fraction
      context.clip(to: Path(CGRect(x: CGFloat(credits.left), y: 0, width: width, height: 1440)))
      for index in rows.indices {
        let row = rows[index]
        if y + heights[index] >= 0 && y <= 1440 {
          if let name = row.image, let image = images[name] {
            context.draw(
              image,
              in: CGRect(
                x: CGFloat(credits.left) + (row.centered ? (width - row.size.width) / 2 : 0), y: y,
                width: row.size.width, height: row.size.height))
          } else if row.hasText {
            let measured = resolved[index].measure(in: CGSize(width: width, height: .infinity))
            let x = CGFloat(credits.left) + (row.centered ? (width - measured.width) / 2 : 0)
            context.draw(resolved[index], at: CGPoint(x: x, y: y), anchor: .topLeading)
          }
        }
        y += heights[index] + row.spacing
      }
    }.allowsHitTesting(false).accessibilityLabel("制作人员名单")
  }
}
