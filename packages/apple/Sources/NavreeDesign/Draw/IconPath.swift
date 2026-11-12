import CoreGraphics
import Foundation
import NavreeProtocol
import SwiftUI

/// SVG path data (`M3 6h18a2 2 0 0 1 2 2z`) read into a `Path`: every command
/// of the SVG path grammar, absolute and relative, arcs turned into curves.
public enum SVGPath {
    public static func path(_ data: String) -> Path {
        var path = Path()
        var reader = Reader(data)
        var current = CGPoint.zero
        var start = CGPoint.zero
        var lastControl: CGPoint?
        var lastCommand: Character = " "
        var command: Character = " "

        while let next = reader.command(continuing: command) {
            command = next
            let relative = command.isLowercase
            let base = relative ? current : .zero
            switch command.uppercased().first ?? " " {
            case "M":
                guard let point = reader.point() else { return path }
                current = CGPoint(x: base.x + point.x, y: base.y + point.y)
                start = current
                path.move(to: current)
                // Pairs after the first of a moveto are linetos.
                command = relative ? "l" : "L"
                lastControl = nil
            case "L":
                guard let point = reader.point() else { return path }
                current = CGPoint(x: base.x + point.x, y: base.y + point.y)
                path.addLine(to: current)
                lastControl = nil
            case "H":
                guard let x = reader.number() else { return path }
                current = CGPoint(x: (relative ? current.x : 0) + x, y: current.y)
                path.addLine(to: current)
                lastControl = nil
            case "V":
                guard let y = reader.number() else { return path }
                current = CGPoint(x: current.x, y: (relative ? current.y : 0) + y)
                path.addLine(to: current)
                lastControl = nil
            case "C":
                guard let c1 = reader.point(), let c2 = reader.point(), let end = reader.point() else { return path }
                let control1 = CGPoint(x: base.x + c1.x, y: base.y + c1.y)
                let control2 = CGPoint(x: base.x + c2.x, y: base.y + c2.y)
                current = CGPoint(x: base.x + end.x, y: base.y + end.y)
                path.addCurve(to: current, control1: control1, control2: control2)
                lastControl = control2
            case "S":
                guard let c2 = reader.point(), let end = reader.point() else { return path }
                let reflected = "CcSs".contains(lastCommand) ? reflect(lastControl, about: current) : current
                let control2 = CGPoint(x: base.x + c2.x, y: base.y + c2.y)
                current = CGPoint(x: base.x + end.x, y: base.y + end.y)
                path.addCurve(to: current, control1: reflected, control2: control2)
                lastControl = control2
            case "Q":
                guard let c = reader.point(), let end = reader.point() else { return path }
                let control = CGPoint(x: base.x + c.x, y: base.y + c.y)
                current = CGPoint(x: base.x + end.x, y: base.y + end.y)
                path.addQuadCurve(to: current, control: control)
                lastControl = control
            case "T":
                guard let end = reader.point() else { return path }
                let control = "QqTt".contains(lastCommand) ? reflect(lastControl, about: current) : current
                current = CGPoint(x: base.x + end.x, y: base.y + end.y)
                path.addQuadCurve(to: current, control: control)
                lastControl = control
            case "A":
                guard let rx = reader.number(), let ry = reader.number(), let rotation = reader.number(),
                    let large = reader.flag(), let sweep = reader.flag(), let end = reader.point()
                else { return path }
                let target = CGPoint(x: base.x + end.x, y: base.y + end.y)
                arc(&path, from: current, to: target, radii: CGSize(width: rx, height: ry), rotation: rotation,
                    large: large, sweep: sweep)
                current = target
                lastControl = nil
            case "Z":
                path.closeSubpath()
                current = start
                lastControl = nil
            default:
                return path
            }
            lastCommand = command
        }
        return path
    }

    private static func reflect(_ point: CGPoint?, about center: CGPoint) -> CGPoint {
        guard let point else { return center }
        return CGPoint(x: 2 * center.x - point.x, y: 2 * center.y - point.y)
    }

    /// An elliptical arc as cubic curves, from the endpoint form of SVG's
    /// implementation notes (F.6.5 and F.6.6).
    static func arc(
        _ path: inout Path, from p0: CGPoint, to p1: CGPoint, radii: CGSize, rotation: Double, large: Bool, sweep: Bool
    ) {
        var rx = abs(radii.width)
        var ry = abs(radii.height)
        guard rx > 0, ry > 0, p0 != p1 else {
            path.addLine(to: p1)
            return
        }
        let phi = rotation * .pi / 180
        let cosPhi = cos(phi)
        let sinPhi = sin(phi)
        let dx = (p0.x - p1.x) / 2
        let dy = (p0.y - p1.y) / 2
        let x1 = cosPhi * dx + sinPhi * dy
        let y1 = -sinPhi * dx + cosPhi * dy
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
        if lambda > 1 {
            rx *= sqrt(lambda)
            ry *= sqrt(lambda)
        }
        let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
        let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1
        var factor = denominator == 0 ? 0 : sqrt(max(0, numerator / denominator))
        if large == sweep { factor = -factor }
        let cx1 = factor * rx * y1 / ry
        let cy1 = -factor * ry * x1 / rx
        let cx = cosPhi * cx1 - sinPhi * cy1 + (p0.x + p1.x) / 2
        let cy = sinPhi * cx1 + cosPhi * cy1 + (p0.y + p1.y) / 2

        func angle(_ ux: Double, _ uy: Double, _ vx: Double, _ vy: Double) -> Double {
            let value = atan2(ux * vy - uy * vx, ux * vx + uy * vy)
            return value
        }
        let theta1 = angle(1, 0, (x1 - cx1) / rx, (y1 - cy1) / ry)
        var delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry)
        if !sweep && delta > 0 { delta -= 2 * .pi }
        if sweep && delta < 0 { delta += 2 * .pi }

        let segments = max(1, Int(ceil(abs(delta) / (.pi / 2))))
        let step = delta / Double(segments)
        let k = 4.0 / 3.0 * tan(step / 4)
        var theta = theta1
        for _ in 0..<segments {
            let cos1 = cos(theta), sin1 = sin(theta)
            let cos2 = cos(theta + step), sin2 = sin(theta + step)
            let e1 = CGPoint(x: cos1 - k * sin1, y: sin1 + k * cos1)
            let e2 = CGPoint(x: cos2 + k * sin2, y: sin2 - k * cos2)
            let e3 = CGPoint(x: cos2, y: sin2)
            func map(_ point: CGPoint) -> CGPoint {
                CGPoint(
                    x: cx + rx * point.x * cosPhi - ry * point.y * sinPhi,
                    y: cy + rx * point.x * sinPhi + ry * point.y * cosPhi
                )
            }
            path.addCurve(to: map(e3), control1: map(e1), control2: map(e2))
            theta += step
        }
    }

    /// Reads commands, numbers and flags off path data.
    struct Reader {
        let scalars: [Unicode.Scalar]
        var index = 0

        init(_ text: String) {
            scalars = Array(text.unicodeScalars)
        }

        mutating func skip() {
            while index < scalars.count, scalars[index] == " " || scalars[index] == "," || scalars[index] == "\n"
                || scalars[index] == "\t" || scalars[index] == "\r"
            {
                index += 1
            }
        }

        /// The next command letter, or the last one again when numbers follow
        /// without a letter (an implicit repeat); `nil` at the end.
        mutating func command(continuing previous: Character) -> Character? {
            skip()
            guard index < scalars.count else { return nil }
            let scalar = scalars[index]
            if CharacterSet.letters.contains(scalar) {
                index += 1
                return Character(scalar)
            }
            return previous == " " ? nil : previous
        }

        mutating func number() -> Double? {
            skip()
            let begin = index
            var seenDot = false
            var seenExponent = false
            if index < scalars.count, scalars[index] == "-" || scalars[index] == "+" { index += 1 }
            while index < scalars.count {
                let scalar = scalars[index]
                if ("0"..."9").contains(scalar) {
                    index += 1
                } else if scalar == ".", !seenDot, !seenExponent {
                    seenDot = true
                    index += 1
                } else if scalar == "e" || scalar == "E", !seenExponent {
                    seenExponent = true
                    index += 1
                    if index < scalars.count, scalars[index] == "-" || scalars[index] == "+" { index += 1 }
                } else {
                    break
                }
            }
            guard index > begin else { return nil }
            var text = ""
            text.unicodeScalars.append(contentsOf: scalars[begin..<index])
            return Double(text)
        }

        /// An arc flag: one digit, which may stand right against the next number.
        mutating func flag() -> Bool? {
            skip()
            guard index < scalars.count else { return nil }
            let scalar = scalars[index]
            guard scalar == "0" || scalar == "1" else { return nil }
            index += 1
            return scalar == "1"
        }

        mutating func point() -> CGPoint? {
            guard let x = number(), let y = number() else { return nil }
            return CGPoint(x: x, y: y)
        }
    }
}

/// An icon's own shape, drawn as Apple draws a template symbol: in the colour
/// around it, stroked at a weight that matches the text beside it.
public struct IconShapeView: View {
    let paths: [String]
    let fills: [String]
    let viewBox: CGRect

    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 20

    public init(paths: [String], fills: [String], viewBox: String?) {
        self.paths = paths
        self.fills = fills
        self.viewBox = Self.box(viewBox)
    }

    public var body: some View {
        Canvas { context, canvas in
            let scale = min(canvas.width / viewBox.width, canvas.height / viewBox.height)
            let transform = CGAffineTransform(translationX: -viewBox.minX, y: -viewBox.minY)
                .concatenating(CGAffineTransform(scaleX: scale, y: scale))
            let shading = GraphicsContext.Shading.foreground
            for data in paths {
                let path = SVGPath.path(data).applying(transform)
                context.stroke(path, with: shading, style: StrokeStyle(lineWidth: 2 * scale, lineCap: .round, lineJoin: .round))
            }
            for data in fills {
                context.fill(SVGPath.path(data).applying(transform), with: shading)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    static func box(_ text: String?) -> CGRect {
        let numbers = (text ?? "").split(whereSeparator: { $0 == " " || $0 == "," }).compactMap { Double($0) }
        guard numbers.count == 4, numbers[2] > 0, numbers[3] > 0 else { return CGRect(x: 0, y: 0, width: 24, height: 24) }
        return CGRect(x: numbers[0], y: numbers[1], width: numbers[2], height: numbers[3])
    }
}

/// An icon's own shape as an image, for the places that take only images: a
/// tab bar item, a sidebar row. It is a template, so it takes the tint.
@MainActor
public enum IconImage {
    public static func image(_ icon: Node.Icon, icons: [String: IconShape] = [:]) -> Image? {
        let shape = icon.name.flatMap { icons[$0] }
        let paths = icon.paths.isEmpty ? shape?.paths ?? [] : icon.paths
        let fills = icon.fills.isEmpty ? shape?.fills ?? [] : icon.fills
        guard !paths.isEmpty || !fills.isEmpty else { return nil }
        let view = IconShapeView(paths: paths, fills: fills, viewBox: icon.viewBox ?? shape?.viewBox)
            .foregroundStyle(.black)
            .frame(width: 24, height: 24)
        let renderer = ImageRenderer(content: view)
        renderer.scale = 3
        guard let cgImage = renderer.cgImage else { return nil }
        return Image(decorative: cgImage, scale: 3).renderingMode(.template)
    }
}
