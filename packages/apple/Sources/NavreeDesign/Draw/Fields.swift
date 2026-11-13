import NavreeProtocol
import SwiftUI
import UniformTypeIdentifiers

/// A field of a form, drawn with the system's control for its input. The
/// server's message about a wrong value stands under the control, in words
/// with a symbol, never as a red border alone; the field's help stands there
/// when there is no message.
struct FieldControl: View {
    let field: Navree.Field
    @Environment(\.navree) private var context
    @Environment(\.navreeForm) private var form

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            control
                .disabled(field.disabled)
            if let error {
                Label(error, systemImage: "exclamationmark.circle.fill")
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .accessibilityLabel("\(field.label ?? field.name): \(error)")
            } else if let help = field.help {
                Text(help).font(.footnote).foregroundStyle(.secondary)
            }
        }
    }

    private var ref: FormRef { form ?? .loose }
    private var label: String { field.label ?? field.name }

    /// What is wrong with the value, as the runtime holds it.
    private var error: String? {
        let messages = context.errors(ref, field.name) ?? field.error.map { [$0] } ?? []
        return messages.isEmpty ? nil : messages.joined(separator: " ")
    }

    /// The values the field holds now: what the reader chose, or what came.
    private var values: [String] {
        context.values(ref, field.name) ?? field.input.initialValues
    }

    private func set(_ values: [String]) {
        context.send(.change(form: ref, name: field.name, values: values))
    }

    private var text: Binding<String> {
        Binding(get: { values.first ?? "" }, set: { set([$0]) })
    }

    @ViewBuilder private var control: some View {
        switch field.input {
        case .text(let input):
            TextControl(label: label, input: input, text: text, invalid: error != nil, id: field.id)
        case .toggle(let input):
            Toggle(label, isOn: Binding(get: { !values.isEmpty }, set: { set($0 ? [input.value ?? "on"] : []) }))
        case .choice(let input):
            ChoiceControl(label: label, input: input, value: Binding(get: { values.first ?? "" }, set: { set([$0]) }))
        case .choices(let input):
            // In a screen's list each option is a row of its own; drawn free,
            // the choices are a menu of toggles.
            Menu {
                ForEach(input.options, id: \.value) { option in
                    Toggle(option.label, isOn: Binding(
                        get: { values.contains(option.value) },
                        set: { on in set(on ? values + [option.value] : values.filter { $0 != option.value }) }
                    ))
                }
            } label: {
                LabeledContent(label, value: summary(input))
            }
        case .date(let input):
            DateControl(label: label, input: input, value: text)
        case .slider(let input):
            let low = Double(input.min) ?? 0
            let high = max(Double(input.max) ?? 100, low + 1)
            LabeledContent(label) {
                Slider(
                    value: Binding(get: { Double(values.first ?? "") ?? low }, set: { set([Self.number($0)]) }),
                    in: low...high, step: Double(input.step ?? "") ?? 1
                )
            }
        case .colour:
            ColorPicker(label, selection: Binding(
                get: { HexColor.color(values.first ?? "") ?? .accentColor },
                set: { set([HexColor.hex($0)]) }
            ))
        case .file(let input):
            FileControl(label: label, input: input, chosen: values) { urls in
                context.send(.files(form: ref, name: field.name, urls: urls))
            }
        case .range(let input):
            RangeControl(label: label, input: input, values: values) { set($0) }
        case .unknown:
            EmptyView()
        }
    }

    private func summary(_ input: Navree.ChoicesInput) -> String {
        let chosen = input.options.filter { values.contains($0.value) }.map(\.label)
        return chosen.isEmpty ? "Any" : chosen.joined(separator: ", ")
    }

    static func number(_ value: Double) -> String {
        value == value.rounded() ? String(Int(value)) : String(value)
    }
}

/// Typed text, with the keyboard and the autofill its kind asks for. With a
/// label, the label leads and the field follows, as in Settings.
struct TextControl: View {
    let label: String
    let input: Navree.TextInput
    let text: Binding<String>
    let invalid: Bool
    let id: String?
    @FocusState private var focused: Bool
    @Environment(\.navree) private var context

    var body: some View {
        let keyboard = ContentRules.keyboard(input)
        Group {
            switch input.kind {
            case .multiline:
                TextField(label, text: text, prompt: input.placeholder.map { Text($0) } ?? Text(label), axis: .vertical)
                    .lineLimit(3...8)
            case .password:
                LabeledContent(label) {
                    SecureField(label, text: text, prompt: input.placeholder.map { Text($0) })
                        .multilineTextAlignment(.trailing)
                }
            default:
                LabeledContent(label) {
                    TextField(label, text: text, prompt: input.placeholder.map { Text($0) })
                        .multilineTextAlignment(.trailing)
                }
            }
        }
        .focused($focused)
        .task(id: context.focus?.serial) {
            // The screen asks for the focus here: it moves, and the runtime hears so.
            guard let request = context.focus, request.target == id else { return }
            focused = true
            context.send(.focused(serial: request.serial))
        }
        .readOnly(input.readonly)
        .keyboard(keyboard)
        .accessibilityValue(invalid ? "Needs a change" : "")
    }
}

extension View {
    @ViewBuilder func readOnly(_ readOnly: Bool) -> some View {
        if readOnly { self.disabled(true) } else { self }
    }

    /// The keyboard, the autofill and the corrections a field asks for.
    @ViewBuilder func keyboard(_ keyboard: Keyboard) -> some View {
        #if os(iOS)
        self
            .keyboardType(Self.layout(keyboard.layout))
            .textContentType(Self.content(keyboard.content))
            .textInputAutocapitalization(keyboard.autocapitalizes ? .sentences : .never)
            .autocorrectionDisabled(!keyboard.autocorrects)
        #else
        self.autocorrectionDisabled(!keyboard.autocorrects)
        #endif
    }

    #if os(iOS)
    static func layout(_ layout: Keyboard.Layout) -> UIKeyboardType {
        switch layout {
        case .standard: .default
        case .email: .emailAddress
        case .url: .URL
        case .number: .numberPad
        case .decimal: .decimalPad
        case .phone: .phonePad
        case .oneTimeCode: .numberPad
        }
    }

    static func content(_ content: Keyboard.Content) -> UITextContentType? {
        switch content {
        case .none: nil
        case .email: .emailAddress
        case .password: .password
        case .newPassword: .newPassword
        case .phone: .telephoneNumber
        case .url: .URL
        case .oneTimeCode: .oneTimeCode
        case .name: .name
        case .username: .username
        }
    }
    #endif
}

/// One of several: a pop-up menu, a segmented control, or the choices as rows
/// with a checkmark (iOS has no radio buttons).
struct ChoiceControl: View {
    let label: String
    let input: Navree.ChoiceInput
    let value: Binding<String>

    var body: some View {
        let picker = Picker(label, selection: value) {
            if input.value == nil && !input.options.contains(where: { $0.value.isEmpty }) {
                Text(input.placeholder ?? "None").tag("")
            }
            ForEach(input.options, id: \.value) { option in
                Text(option.label).tag(option.value)
            }
        }
        switch ContentRules.control(.choice(input)) {
        case .segmented: picker.pickerStyle(.segmented)
        case .inlinePicker: picker.pickerStyle(.inline)
        #if os(iOS)
        case .navigationPicker: picker.pickerStyle(.navigationLink)
        #endif
        default: picker.pickerStyle(.menu)
        }
    }
}

/// A day, a time or both: the system's compact picker, and nothing chosen
/// until the reader sets one.
struct DateControl: View {
    let label: String
    let input: Navree.DateInput
    let value: Binding<String>

    var body: some View {
        if value.wrappedValue.isEmpty {
            LabeledContent(label) {
                Button("Set") { value.wrappedValue = WebDate.text(Date(), kind: input.kind) }
                    .buttonStyle(.borderless)
            }
        } else {
            DatePicker(
                label,
                selection: Binding(
                    get: { WebDate.date(value.wrappedValue, kind: input.kind) ?? Date() },
                    set: { value.wrappedValue = WebDate.text($0, kind: input.kind) }
                ),
                in: range,
                displayedComponents: components
            )
        }
    }

    private var components: DatePickerComponents {
        switch input.kind {
        case .date: .date
        case .time: .hourAndMinute
        case .datetime: [.date, .hourAndMinute]
        }
    }

    private var range: ClosedRange<Date> {
        let low = input.min.flatMap { WebDate.date($0, kind: input.kind) } ?? .distantPast
        let high = input.max.flatMap { WebDate.date($0, kind: input.kind) } ?? .distantFuture
        return low...max(low, high)
    }
}

/// A span of days (or times): the system's two compact pickers under the
/// field's name, the end never before the start, as Calendar sets an event's
/// start and end.
struct RangeControl: View {
    let label: String
    let input: Navree.RangeInput
    let values: [String]
    let set: ([String]) -> Void

    var body: some View {
        let from = values.first ?? ""
        let to = values.count > 1 ? values[1] : ""
        VStack(alignment: .leading, spacing: 8) {
            Text(label)
            DateControl(
                label: RangeRules.startWords, input: Navree.DateInput(kind: input.kind, max: input.max, min: input.min, value: from),
                value: Binding(get: { from }, set: { set(RangeRules.values(from: $0, to: to, kind: input.kind)) })
            )
            DateControl(
                label: RangeRules.endWords,
                input: Navree.DateInput(kind: input.kind, max: input.max, min: from.isEmpty ? input.min : from, value: to),
                value: Binding(get: { to }, set: { set(RangeRules.values(from: from, to: $0, kind: input.kind)) })
            )
        }
    }
}

/// Files: the system's file browser, limited to the types the field takes.
/// The chosen files' names are shown; the app keeps their bytes until the
/// form sends them.
struct FileControl: View {
    let label: String
    let input: Navree.FileInput
    let chosen: [String]
    let choose: ([URL]) -> Void
    @State private var importing = false

    var body: some View {
        LabeledContent(label) {
            Button(chosen.isEmpty ? String(localized: "Choose…") : chosen.joined(separator: ", ")) { importing = true }
                .buttonStyle(.borderless)
                .lineLimit(2)
        }
        .fileImporter(
            isPresented: $importing, allowedContentTypes: FileRules.types(input.accept),
            allowsMultipleSelection: input.multiple
        ) { result in
            if case .success(let urls) = result, !urls.isEmpty { choose(urls) }
        }
    }
}

/// Dates as the web's own fields send them: `2026-11-12`, `09:41`,
/// `2026-11-12T09:41`, in the device's time zone.
public enum WebDate {
    static func formatter(_ kind: Navree.DateKind) -> DateFormatter {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.dateFormat = switch kind {
        case .date: "yyyy-MM-dd"
        case .time: "HH:mm"
        case .datetime: "yyyy-MM-dd'T'HH:mm"
        }
        return formatter
    }

    public static func date(_ text: String, kind: Navree.DateKind) -> Date? {
        formatter(kind).date(from: text)
    }

    public static func text(_ date: Date, kind: Navree.DateKind) -> String {
        formatter(kind).string(from: date)
    }
}

/// Colours as `#rrggbb`.
enum HexColor {
    static func color(_ hex: String) -> Color? {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard digits.count == 6, let value = UInt32(digits, radix: 16) else { return nil }
        return Color(
            red: Double((value >> 16) & 0xFF) / 255, green: Double((value >> 8) & 0xFF) / 255,
            blue: Double(value & 0xFF) / 255
        )
    }

    @MainActor static func hex(_ color: Color) -> String {
        let resolved = color.resolve(in: EnvironmentValues())
        func byte(_ value: Float) -> Int { Int((max(0, min(1, value)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", byte(resolved.red), byte(resolved.green), byte(resolved.blue))
    }
}
