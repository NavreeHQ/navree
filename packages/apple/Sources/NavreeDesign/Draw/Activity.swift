import NavreeProtocol
import SwiftUI

/// Something that happened, as a row of an activity list: its symbol in the
/// colour of what it means (or the person who did it), the words with their
/// names in bold, when it happened under them, and the thing it is about
/// opened by a tap. A quiet one is drawn in the secondary colour.
struct FeedRowPart: View {
    let row: Navree.FeedRow
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay
    @ScaledMetric(relativeTo: .body) private var lead: CGFloat = 28

    var body: some View {
        Opening(action: row.action, place: Place(overlay: overlay, from: row.id)) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                leading
                    .frame(width: lead)
                VStack(alignment: .leading, spacing: 3) {
                    Text(RunText.attributed(row.runs))
                        .foregroundStyle(row.muted ? Color.secondary : Color.primary)
                        .fixedSize(horizontal: false, vertical: true)
                    ForEach(Array(row.detail.enumerated()), id: \.offset) { _, part in
                        NodeView(part).font(.subheadline).foregroundStyle(Color.secondary)
                            .environment(\.navreeInRowDetail, true)
                    }
                    if let time = row.time {
                        // The secondary colour itself: inside a link a hierarchical
                        // style would take the link's tint.
                        Text(time).font(.subheadline).foregroundStyle(Color.secondary)
                    }
                }
                Spacer(minLength: 0)
            }
            .accessibilityElement(children: .combine)
        }
        .environment(\.openURL, OpenURLAction { url in
            context.send(.act(.navigate(Navree.Action.Navigate(href: url.absoluteString)), Place(overlay: overlay)))
            return .handled
        })
    }

    @ViewBuilder private var leading: some View {
        if let actor = row.actor {
            AvatarPart(avatar: actor)
        } else if let icon = row.icon {
            Glyph(icon: icon, icons: context.icons)
                .font(.body)
                .foregroundStyle(row.muted ? Color.secondary : Tones.color(row.tone ?? .accent))
                .accessibilityHidden(true)
        } else {
            Image(systemName: Tones.symbol(row.tone)).foregroundStyle(Tones.color(row.tone))
                .accessibilityHidden(true)
        }
    }
}

/// A message of a conversation, as Mail sets one out in a thread: who wrote it
/// beside their picture, when, its words, and how far it got. An internal note
/// says so in words with a symbol.
struct MessagePart: View {
    let message: Navree.Message
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            if !typeSize.isAccessibilitySize {
                AvatarPart(avatar: message.avatar ?? Navree.Avatar(name: message.author))
            }
            VStack(alignment: .leading, spacing: 6) {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .firstTextBaseline) {
                        heading
                        Spacer(minLength: 8)
                        time
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        heading
                        time
                    }
                }
                if message.note {
                    Label(MessageRules.noteWords, systemImage: "lock.fill")
                        .font(.subheadline)
                        .foregroundStyle(.orange)
                }
                ForEach(Array(message.body.enumerated()), id: \.offset) { _, part in
                    NodeView(part)
                }
                if let delivery = message.delivery, let words = MessageRules.delivery(delivery) {
                    Label(words.text, systemImage: words.symbol)
                        .font(.footnote)
                        .foregroundStyle(delivery == .failed ? Color.red : Color.secondary)
                }
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    private var heading: some View {
        Text(message.author).font(.headline)
    }

    @ViewBuilder private var time: some View {
        if let time = message.time {
            Text(time).font(.subheadline).foregroundStyle(.secondary)
        }
    }
}

/// Words in runs: bold, italic and code where the server says, links that
/// open in the app.
enum RunText {
    static func attributed(_ runs: [Navree.Run]) -> AttributedString {
        var result = AttributedString()
        for run in runs {
            var piece = AttributedString(run.text)
            var intent: InlinePresentationIntent = []
            if run.bold { intent.insert(.stronglyEmphasized) }
            if run.italic { intent.insert(.emphasized) }
            if run.code { intent.insert(.code) }
            if !intent.isEmpty { piece.inlinePresentationIntent = intent }
            if let href = run.href, let url = URL(string: href) { piece.link = url }
            result += piece
        }
        return result
    }
}
