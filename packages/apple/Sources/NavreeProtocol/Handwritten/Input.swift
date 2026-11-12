/// The control of a form's field. Its value is sent under the field's name,
/// exactly as a browser's form would send it.
public enum Input: Sendable, Equatable, Decodable {
    case text(TextInput)
    case toggle(ToggleInput)
    case choice(ChoiceInput)
    case choices(ChoicesInput)
    case date(DateInput)
    case slider(SliderInput)
    case colour(ColourInput)
    case file(FileInput)
    /// A control added after this client was built.
    case unrecognized(String)

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        let kind: String = try c.required("t")
        switch kind {
        case "text": self = .text(try TextInput(from: decoder))
        case "toggle": self = .toggle(try ToggleInput(from: decoder))
        case "choice": self = .choice(try ChoiceInput(from: decoder))
        case "choices": self = .choices(try ChoicesInput(from: decoder))
        case "date": self = .date(try DateInput(from: decoder))
        case "slider": self = .slider(try SliderInput(from: decoder))
        case "colour": self = .colour(try ColourInput(from: decoder))
        case "file": self = .file(try FileInput(from: decoder))
        default: self = .unrecognized(kind)
        }
    }

    /// The value the control holds as the screen arrives, as a form sends it.
    public var initialValues: [String] {
        switch self {
        case .text(let input): [input.value]
        case .toggle(let input): input.on ? [input.value ?? "on"] : []
        case .choice(let input): input.value.map { [$0] } ?? []
        case .choices(let input): input.values
        case .date(let input): [input.value]
        case .slider(let input): [input.value]
        case .colour(let input): [input.value]
        case .file, .unrecognized: []
        }
    }
}

/// Typed text.
public struct TextInput: Sendable, Equatable, Decodable {
    public var kind: TextKind
    public var value: String
    public var placeholder: String?
    public var maxLength: Int?
    public var min: String?
    public var max: String?
    public var step: String?
    /// What a system may fill it with, by the web's own names.
    public var autofill: String?
    public var readonly: Bool

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        kind = try c.field("kind", or: .plain)
        value = try c.field("value", or: "")
        placeholder = try c.optional("placeholder")
        maxLength = try c.optional("max_length")
        min = try c.optional("min")
        max = try c.optional("max")
        step = try c.optional("step")
        autofill = try c.optional("autofill")
        readonly = try c.field("readonly", or: false)
    }
}

/// On or off.
public struct ToggleInput: Sendable, Equatable, Decodable {
    public var kind: ToggleKind
    public var on: Bool
    /// What is sent when it is on (`on` when there is none).
    public var value: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        kind = try c.field("kind", or: .switch)
        on = try c.field("on", or: false)
        value = try c.optional("value")
    }
}

/// One thing that can be chosen.
public struct ChoiceOption: Sendable, Equatable, Hashable, Decodable {
    public var value: String
    public var label: String
    /// A few words beside the label: how many records hold it.
    public var detail: String?
    public var disabled: Bool

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        value = try c.field("value", or: "")
        label = try c.field("label", or: "")
        detail = try c.optional("detail")
        disabled = try c.field("disabled", or: false)
    }
}

/// One of several.
public struct ChoiceInput: Sendable, Equatable, Decodable {
    public var kind: ChoiceKind
    public var options: [ChoiceOption]
    public var value: String?
    public var placeholder: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        kind = try c.field("kind", or: .menu)
        options = try c.field("options", or: [])
        value = try c.optional("value")
        placeholder = try c.optional("placeholder")
    }
}

/// Any of several. Each chosen value is sent under the field's name.
public struct ChoicesInput: Sendable, Equatable, Decodable {
    public var options: [ChoiceOption]
    public var values: [String]

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        options = try c.field("options", or: [])
        values = try c.field("values", or: [])
    }
}

/// A date (`2026-11-12`), a time (`09:41`) or both (`2026-11-12T09:41`).
public struct DateInput: Sendable, Equatable, Decodable {
    public var kind: DateKind
    public var value: String
    public var min: String?
    public var max: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        kind = try c.field("kind", or: .date)
        value = try c.field("value", or: "")
        min = try c.optional("min")
        max = try c.optional("max")
    }
}

/// A number between two ends. The numbers are text, as the page wrote them.
public struct SliderInput: Sendable, Equatable, Decodable {
    public var min: String
    public var max: String
    public var step: String?
    public var value: String

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        min = try c.field("min", or: "0")
        max = try c.field("max", or: "100")
        step = try c.optional("step")
        value = try c.field("value", or: "0")
    }
}

/// A colour, sent as `#rrggbb`.
public struct ColourInput: Sendable, Equatable, Decodable {
    public var value: String

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        value = try c.field("value", or: "")
    }
}

/// Files. A field of this kind makes its form a multipart post.
public struct FileInput: Sendable, Equatable, Decodable {
    public var accept: String?
    public var multiple: Bool
    public var capture: String?

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        accept = try c.optional("accept")
        multiple = try c.field("multiple", or: false)
        capture = try c.optional("capture")
    }
}
