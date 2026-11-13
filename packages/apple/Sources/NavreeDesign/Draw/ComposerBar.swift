import NavreeProtocol
import SwiftUI

/// A conversation's reply as a bar at the foot of the screen, as Messages
/// keeps its field: above the tab bar and the keyboard, the conversation
/// scrolling under it with the system's edge effect. Its other parts (how it
/// is sent, saved replies) stand in a row over the field; the field grows
/// with what is written, and the send button beside it waits for words.
struct ComposerBar: View {
    let found: ComposerRules.Found
    @Environment(\.navree) private var context
    @Environment(\.navreeOverlay) private var overlay

    private var ref: FormRef { FormRef(found.form) }
    private var field: Navree.Field { found.message }
    private var label: String { field.label ?? field.name }

    private var text: String {
        context.values(ref, field.name)?.first ?? field.input.initialValues.first ?? ""
    }

    var body: some View {
        let others = ComposerRules.others(found)
        VStack(alignment: .leading, spacing: 8) {
            if !others.isEmpty {
                HStack(spacing: 12) {
                    ForEach(Array(others.enumerated()), id: \.offset) { _, part in
                        NodeView(part).fixedSize()
                    }
                    Spacer(minLength: 0)
                }
                .controlSize(.small)
            }
            HStack(alignment: .bottom, spacing: 8) {
                ZStack(alignment: .topLeading) {
                    // The words as they would wrap (one line to six) set the
                    // field's height: a field that grows by lines takes all the
                    // height a bar offers it, or none, by itself.
                    Text(text.isEmpty ? placeholder ?? label : text + " ")
                        .lineLimit(1...6)
                        .hidden()
                        .accessibilityHidden(true)
                    TextField(
                        placeholder ?? label,
                        text: Binding(get: { text }, set: { context.send(.change(form: ref, name: field.name, values: [$0])) }),
                        axis: .vertical
                    )
                    .lineLimit(1...6)
                    .textFieldStyle(.plain)
                    .frame(maxHeight: .infinity, alignment: .top)
                    .accessibilityLabel(label)
                }
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .background(.fill.tertiary, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                Button {
                    if let action = found.send.action {
                        context.send(.act(action, Place(form: ref, overlay: overlay, ask: PresentationRules.question(found.send))))
                    }
                } label: {
                    ButtonLabel(button: found.send, mode: .icon)
                }
                .buttonStyle(.borderedProminent)
                .buttonBorderShape(.circle)
                .disabled(!ComposerRules.canSend(text) || found.send.disabled || found.send.pending)
                .keyboardShortcut(.return, modifiers: .command)
            }
            let error = (context.errors(ref, field.name) ?? field.error.map { [$0] } ?? []).joined(separator: " ")
            if !error.isEmpty {
                Label(error, systemImage: "exclamationmark.circle.fill")
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .environment(\.navreeForm, ref)
    }

    private var placeholder: String? {
        if case .text(let input) = field.input { return input.placeholder }
        return nil
    }
}

/// The reply bar at the foot of a conversation, where the screen has one.
struct ComposerDock: ViewModifier {
    let found: ComposerRules.Found?

    func body(content: Content) -> some View {
        if let found {
            content.safeAreaBar(edge: .bottom) {
                ComposerBar(found: found)
            }
        } else {
            content
        }
    }
}
