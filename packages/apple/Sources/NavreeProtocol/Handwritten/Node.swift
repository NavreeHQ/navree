/// One part of a screen. A node says what a part is; the client's design layer
/// decides where it goes and how it looks.
public indirect enum Node: Sendable, Equatable, Decodable {
    case stack(Stack)
    case grid(Grid)
    case section(Section)
    case list(List)
    case row(Row)
    case split(Split)
    case tabs(Tabs)
    case disclosure(Disclosure)
    case divider(Divider)
    case spacer(Spacer)
    case region(Region)
    case text(Text)
    case rich(Rich)
    case image(Image)
    case icon(Icon)
    case avatar(Avatar)
    case badge(Badge)
    case progress(Progress)
    case banner(Banner)
    case empty(Empty)
    case facts(Facts)
    case table(Table)
    case chart(Chart)
    case code(Code)
    case button(Button)
    case link(Link)
    case menu(Menu)
    case form(Form)
    case field(Field)
    case sheet(Sheet)
    case dialog(Dialog)
    case popover(Popover)
    case native(NativeView)
    case web(WebRegion)
    /// A kind added after this client was built. It is drawn from `alt` when the
    /// server sent one, and is nothing otherwise.
    case unknown(Unknown)

    public init(from decoder: any Decoder) throws {
        let c = try decoder.wire()
        let kind: String = try c.required("t")
        switch kind {
        case "stack": self = .stack(try Stack(from: decoder))
        case "grid": self = .grid(try Grid(from: decoder))
        case "section": self = .section(try Section(from: decoder))
        case "list": self = .list(try List(from: decoder))
        case "row": self = .row(try Row(from: decoder))
        case "split": self = .split(try Split(from: decoder))
        case "tabs": self = .tabs(try Tabs(from: decoder))
        case "disclosure": self = .disclosure(try Disclosure(from: decoder))
        case "divider": self = .divider(Divider(id: try c.optional("id")))
        case "spacer": self = .spacer(Spacer(id: try c.optional("id")))
        case "region": self = .region(try Region(from: decoder))
        case "text": self = .text(try Text(from: decoder))
        case "rich": self = .rich(try Rich(from: decoder))
        case "image": self = .image(try Image(from: decoder))
        case "icon": self = .icon(try Icon(from: decoder))
        case "avatar": self = .avatar(try Avatar(from: decoder))
        case "badge": self = .badge(try Badge(from: decoder))
        case "progress": self = .progress(try Progress(from: decoder))
        case "banner": self = .banner(try Banner(from: decoder))
        case "empty": self = .empty(try Empty(from: decoder))
        case "facts": self = .facts(try Facts(from: decoder))
        case "table": self = .table(try Table(from: decoder))
        case "chart": self = .chart(try Chart(from: decoder))
        case "code": self = .code(try Code(from: decoder))
        case "button": self = .button(try Button(from: decoder))
        case "link": self = .link(try Link(from: decoder))
        case "menu": self = .menu(try Menu(from: decoder))
        case "form": self = .form(try Form(from: decoder))
        case "field": self = .field(try Field(from: decoder))
        case "sheet": self = .sheet(try Sheet(from: decoder))
        case "dialog": self = .dialog(try Dialog(from: decoder))
        case "popover": self = .popover(try Popover(from: decoder))
        case "native": self = .native(try NativeView(from: decoder))
        case "web": self = .web(try WebRegion(from: decoder))
        default:
            self = .unknown(Unknown(kind: kind, id: try? c.optional("id"), alt: try? c.optional("alt")))
        }
    }
}

extension Node {
    /// A kind this client does not know.
    public struct Unknown: Sendable, Equatable {
        public var kind: String
        public var id: String?
        public var alt: Node?
    }

    /// Parts one after another.
    public struct Stack: Sendable, Equatable, Decodable {
        public var id: String?
        public var axis: StackAxis
        /// The room between parts, in steps of the spacing scale (4 points each).
        public var gap: Int
        public var align: Align?
        public var justify: Justify?
        public var wrap: Bool
        /// Not drawn until a command shows it.
        public var hidden: Bool
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            axis = try c.field("axis", or: .vertical)
            gap = try c.field("gap", or: 0)
            align = try c.optional("align")
            justify = try c.optional("justify")
            wrap = try c.field("wrap", or: false)
            hidden = try c.field("hidden", or: false)
            children = try c.field("children", or: [])
        }
    }

    /// Parts in columns, as many as fit.
    public struct Grid: Sendable, Equatable, Decodable {
        public var id: String?
        public var cols: Int?
        /// The least a column may be wide, in points.
        public var min: Int?
        public var gap: Int
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            cols = try c.optional("cols")
            min = try c.optional("min")
            gap = try c.field("gap", or: 0)
            children = try c.field("children", or: [])
        }
    }

    /// A titled group of parts.
    public struct Section: Sendable, Equatable, Decodable {
        public var id: String?
        public var title: String?
        public var subtitle: String?
        public var style: SectionStyle
        public var actions: [Node]
        public var children: [Node]
        public var footer: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            title = try c.optional("title")
            subtitle = try c.optional("subtitle")
            style = try c.field("style", or: .plain)
            actions = try c.field("actions", or: [])
            children = try c.field("children", or: [])
            footer = try c.field("footer", or: [])
        }
    }

    /// Rows that may go on: more load by cursor.
    public struct List: Sendable, Equatable, Decodable {
        public var id: String?
        public var children: [Node]
        public var more: String?
        public var ordered: Bool

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            children = try c.field("children", or: [])
            more = try c.optional("more")
            ordered = try c.field("ordered", or: false)
        }
    }

    /// One row of a list.
    public struct Row: Sendable, Equatable, Decodable {
        public var id: String?
        public var leading: Node?
        public var title: String
        public var subtitle: String?
        public var detail: [Node]
        public var trailing: Node?
        public var badge: Badge?
        public var state: RowState?
        public var action: Action?
        public var swipe: [Button]
        public var menu: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            leading = try c.optional("leading")
            title = try c.field("title", or: "")
            subtitle = try c.optional("subtitle")
            detail = try c.field("detail", or: [])
            trailing = try c.optional("trailing")
            badge = try c.optional("badge")
            state = try c.optional("state")
            action = try c.optional("action")
            swipe = try c.field("swipe", or: [])
            menu = try c.field("menu", or: [])
        }
    }

    /// A primary pane and its detail.
    public struct Split: Sendable, Equatable, Decodable {
        public var id: String?
        public var primary: Node
        public var detail: Node

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            primary = try c.required("primary")
            detail = try c.required("detail")
        }
    }

    /// Views a reader switches between.
    public struct Tabs: Sendable, Equatable, Decodable {
        public var id: String?
        public var items: [Tab]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            items = try c.field("items", or: [])
        }
    }

    /// One tab: a view held here, or a link to the page that holds it.
    public struct Tab: Sendable, Equatable, Decodable {
        public var label: String
        public var icon: Icon?
        public var selected: Bool
        public var href: String?
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            label = try c.field("label", or: "")
            icon = try c.optional("icon")
            selected = try c.field("selected", or: false)
            href = try c.optional("href")
            children = try c.field("children", or: [])
        }
    }

    /// A part that opens and closes under its label.
    public struct Disclosure: Sendable, Equatable, Decodable {
        public var id: String?
        public var label: String
        public var open: Bool
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            label = try c.field("label", or: "")
            open = try c.field("open", or: false)
            children = try c.field("children", or: [])
        }
    }

    /// A line between parts.
    public struct Divider: Sendable, Equatable {
        public var id: String?
    }

    /// Room that takes what is left along a stack.
    public struct Spacer: Sendable, Equatable {
        public var id: String?
    }

    /// A part that fetches itself: once after the screen is drawn (`lazy`), and
    /// again whenever its stream speaks (`live`).
    public struct Region: Sendable, Equatable, Decodable {
        public var id: String
        public var src: String
        public var lazy: Bool
        public var live: Live?
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.required("id")
            src = try c.field("src", or: "")
            lazy = try c.field("lazy", or: false)
            live = try c.optional("live")
            children = try c.field("children", or: [])
        }
    }

    /// A stream of server-sent events a region listens to.
    public struct Live: Sendable, Equatable, Decodable {
        public var src: String
        public var event: String?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            src = try c.required("src")
            event = try c.optional("event")
        }
    }

    /// Text in one style.
    public struct Text: Sendable, Equatable, Decodable {
        public var id: String?
        public var value: String
        public var style: TextStyle
        public var tone: Tone?
        public var muted: Bool
        public var lines: Int?
        /// The level of the heading it is, from 1, for assistive technology.
        public var heading: Int?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            value = try c.field("value", or: "")
            style = try c.field("style", or: .body)
            tone = try c.optional("tone")
            muted = try c.field("muted", or: false)
            lines = try c.optional("lines")
            heading = try c.optional("heading")
        }
    }

    /// Text in runs: bold, italic, code and links.
    public struct Rich: Sendable, Equatable, Decodable {
        public var id: String?
        public var runs: [Run]
        public var style: TextStyle
        public var muted: Bool

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            runs = try c.field("runs", or: [])
            style = try c.field("style", or: .body)
            muted = try c.field("muted", or: false)
        }
    }

    /// One run of rich text.
    public struct Run: Sendable, Equatable, Decodable {
        public var text: String
        public var bold: Bool
        public var italic: Bool
        public var code: Bool
        public var href: String?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            text = try c.field("text", or: "")
            bold = try c.field("bold", or: false)
            italic = try c.field("italic", or: false)
            code = try c.field("code", or: false)
            href = try c.optional("href")
        }
    }

    /// A picture.
    public struct Image: Sendable, Equatable, Decodable {
        public var id: String?
        public var src: String
        /// What it shows, for a reader who can't see it. Empty for decoration.
        public var alt: String
        public var width: Int?
        public var height: Int?
        public var fit: Fit?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            src = try c.field("src", or: "")
            alt = try c.field("alt", or: "")
            width = try c.optional("width")
            height = try c.optional("height")
            fit = try c.optional("fit")
        }
    }

    /// An icon: the name a system symbol can be found by, and path data to draw
    /// when there is none.
    public struct Icon: Sendable, Equatable, Decodable {
        public var id: String?
        /// The icon's name in the toolkit's set, such as `shopping-bag`.
        public var name: String?
        /// Shapes to stroke (two units wide in a box of 24, round ends and joins).
        public var paths: [String]
        /// Shapes to fill.
        public var fills: [String]
        /// The box the paths are drawn in, as `min-x min-y width height`.
        public var viewBox: String?
        /// What the icon says when it stands alone. A decoration has none.
        public var label: String?
        public var tone: Tone?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            name = try c.optional("name")
            paths = try c.field("paths", or: [])
            fills = try c.field("fills", or: [])
            viewBox = try c.optional("view_box")
            label = try c.optional("label")
            tone = try c.optional("tone")
        }

        public init(name: String?, paths: [String] = [], label: String? = nil) {
            self.name = name
            self.paths = paths
            self.fills = []
            self.label = label
        }
    }

    /// A person's picture, or their initials when there is none.
    public struct Avatar: Sendable, Equatable, Decodable {
        public var id: String?
        public var name: String
        public var src: String?
        public var initials: String?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            name = try c.field("name", or: "")
            src = try c.optional("src")
            initials = try c.optional("initials")
        }
    }

    /// A short label with a tone.
    public struct Badge: Sendable, Equatable, Decodable {
        public var id: String?
        public var text: String
        public var tone: Tone?
        public var dot: Bool

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            text = try c.field("text", or: "")
            tone = try c.optional("tone")
            dot = try c.field("dot", or: false)
        }
    }

    /// How far something has got.
    public struct Progress: Sendable, Equatable, Decodable {
        public var id: String?
        /// How far, in thousandths. Nothing when it is not known.
        public var value: Int?
        public var label: String?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            value = try c.optional("value")
            label = try c.optional("label")
        }
    }

    /// A message in place, with a tone.
    public struct Banner: Sendable, Equatable, Decodable {
        public var id: String?
        public var tone: Tone?
        public var title: String?
        public var text: String?
        public var icon: Icon?
        public var actions: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            tone = try c.optional("tone")
            title = try c.optional("title")
            text = try c.optional("text")
            icon = try c.optional("icon")
            actions = try c.field("actions", or: [])
        }
    }

    /// What a place shows when it holds nothing.
    public struct Empty: Sendable, Equatable, Decodable {
        public var id: String?
        public var icon: Icon?
        public var title: String
        public var text: String?
        public var actions: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            icon = try c.optional("icon")
            title = try c.field("title", or: "")
            text = try c.optional("text")
            actions = try c.field("actions", or: [])
        }
    }

    /// Labels with their values.
    public struct Facts: Sendable, Equatable, Decodable {
        public var id: String?
        public var items: [Fact]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            items = try c.field("items", or: [])
        }
    }

    /// One fact: a label, its value, and lines of detail under it.
    public struct Fact: Sendable, Equatable, Decodable {
        public var label: String
        public var value: [Node]
        public var icon: Icon?
        public var tone: Tone?
        public var details: [String]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            label = try c.field("label", or: "")
            value = try c.field("value", or: [])
            icon = try c.optional("icon")
            tone = try c.optional("tone")
            details = try c.field("details", or: [])
        }
    }

    /// Records in columns.
    public struct Table: Sendable, Equatable, Decodable {
        public var id: String?
        public var caption: String?
        public var columns: [Column]
        public var rows: [TableRow]
        /// The form that asks for other records: a search field and filters.
        public var query: Form?
        public var more: String?
        public var prev: String?
        public var summary: String?
        public var empty: Node?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            caption = try c.optional("caption")
            columns = try c.field("columns", or: [])
            rows = try c.field("rows", or: [])
            query = try c.optional("query")
            more = try c.optional("more")
            prev = try c.optional("prev")
            summary = try c.optional("summary")
            empty = try c.optional("empty")
        }
    }

    /// One column of a table.
    public struct Column: Sendable, Equatable, Decodable {
        public var key: String?
        public var label: String
        public var numeric: Bool
        public var align: Align?
        public var sort: Sort?
        public var role: ColumnRole?
        /// Whether a narrow screen may leave it out.
        public var secondary: Bool

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            key = try c.optional("key")
            label = try c.field("label", or: "")
            numeric = try c.field("numeric", or: false)
            align = try c.optional("align")
            sort = try c.optional("sort")
            role = try c.optional("role")
            secondary = try c.field("secondary", or: false)
        }
    }

    /// How a column sorts.
    public struct Sort: Sendable, Equatable, Decodable {
        public var href: String
        public var dir: SortDirection?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            href = try c.required("href")
            dir = try c.optional("dir")
        }
    }

    /// One record of a table.
    public struct TableRow: Sendable, Equatable, Decodable {
        public var id: String?
        public var cells: [Node]
        public var action: Action?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            cells = try c.field("cells", or: [])
            action = try c.optional("action")
        }
    }

    /// A chart, as the numbers it draws.
    public struct Chart: Sendable, Equatable, Decodable {
        public var id: String?
        public var kind: ChartKind
        public var title: String?
        public var labels: [String]
        public var series: [Series]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            kind = try c.field("kind", or: .line)
            title = try c.optional("title")
            labels = try c.field("labels", or: [])
            series = try c.field("series", or: [])
        }
    }

    /// One series of a chart.
    public struct Series: Sendable, Equatable, Decodable {
        public var label: String
        public var values: [Double?]
        /// Its values in the page's words (`₹1,24,050`), one a label.
        public var shown: [String]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            label = try c.field("label", or: "")
            values = try c.field("values", or: [])
            shown = try c.field("shown", or: [])
        }
    }

    /// Code, in a fixed-width face.
    public struct Code: Sendable, Equatable, Decodable {
        public var id: String?
        public var text: String
        public var lang: String?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            text = try c.field("text", or: "")
            lang = try c.optional("lang")
        }
    }

    /// A button.
    public struct Button: Sendable, Equatable, Decodable {
        public var id: String?
        /// What it says; an icon-only button keeps it for assistive technology.
        public var label: String
        public var icon: Icon?
        public var iconOnly: Bool
        public var role: Role
        public var prominence: ButtonProminence
        public var disabled: Bool
        public var pending: Bool
        public var action: Action?
        /// A standard action the system has its own symbol and words for.
        public var standard: StandardAction?
        /// Whether what a destructive action does can be undone: then it asks
        /// no confirmation first.
        public var undoable: Bool

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            label = try c.field("label", or: "")
            icon = try c.optional("icon")
            iconOnly = try c.field("icon_only", or: false)
            role = try c.field("role", or: .plain)
            prominence = try c.field("prominence", or: .medium)
            disabled = try c.field("disabled", or: false)
            pending = try c.field("pending", or: false)
            action = try c.optional("action")
            standard = try c.optional("standard")
            undoable = try c.field("undoable", or: false)
        }

        public init(label: String, role: Role = .plain, icon: Icon? = nil, action: Action? = nil) {
            self.label = label
            self.icon = icon
            self.iconOnly = false
            self.role = role
            self.prominence = .medium
            self.disabled = false
            self.pending = false
            self.action = action
            self.undoable = false
        }
    }

    /// A link: words, or parts that lead somewhere together.
    public struct Link: Sendable, Equatable, Decodable {
        public var id: String?
        public var label: String
        public var action: Action
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            label = try c.field("label", or: "")
            action = try c.required("action")
            children = try c.field("children", or: [])
        }
    }

    /// A button that opens a list of actions.
    public struct Menu: Sendable, Equatable, Decodable {
        public var id: String?
        public var label: String
        public var icon: Icon?
        /// Buttons, links, sections of them and dividers.
        public var items: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            label = try c.field("label", or: "")
            icon = try c.optional("icon")
            items = try c.field("items", or: [])
        }
    }

    /// A form. The client holds its fields and sends them with the hidden values.
    public struct Form: Sendable, Equatable, Decodable {
        public var id: String?
        public var method: Method
        /// Where it is sent: the screen's own address when empty.
        public var action: String
        public var hidden: [Hidden]
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            method = try c.field("method", or: .get)
            action = try c.field("action", or: "")
            hidden = try c.field("hidden", or: [])
            children = try c.field("children", or: [])
        }
    }

    /// A value a form sends that the reader never sees.
    public struct Hidden: Sendable, Equatable, Decodable {
        public var name: String
        public var value: String
        public var id: String?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            name = try c.required("name")
            value = try c.field("value", or: "")
            id = try c.optional("id")
        }
    }

    /// One field of a form.
    public struct Field: Sendable, Equatable, Decodable {
        /// The id of the field's control: what a `focus` names.
        public var id: String?
        public var name: String
        public var label: String?
        public var help: String?
        public var error: String?
        public var required: Bool
        public var disabled: Bool
        public var input: Input

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            name = try c.field("name", or: "")
            label = try c.optional("label")
            help = try c.optional("help")
            error = try c.optional("error")
            required = try c.field("required", or: false)
            disabled = try c.field("disabled", or: false)
            input = try c.required("input")
        }
    }

    /// A sheet over the screen. It draws nothing until it is opened.
    public struct Sheet: Sendable, Equatable, Decodable {
        public var id: String
        public var title: String?
        public var subtitle: String?
        public var size: SheetSize
        public var open: Bool
        public var children: [Node]
        public var actions: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.required("id")
            title = try c.optional("title")
            subtitle = try c.optional("subtitle")
            size = try c.field("size", or: .large)
            open = try c.field("open", or: false)
            children = try c.field("children", or: [])
            actions = try c.field("actions", or: [])
        }
    }

    /// A dialog. It draws nothing until it is opened.
    public struct Dialog: Sendable, Equatable, Decodable {
        public var id: String
        public var title: String?
        public var text: String?
        /// A question with buttons only: the system's alert.
        public var alert: Bool
        public var open: Bool
        public var children: [Node]
        public var actions: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.required("id")
            title = try c.optional("title")
            text = try c.optional("text")
            alert = try c.field("alert", or: false)
            open = try c.field("open", or: false)
            children = try c.field("children", or: [])
            actions = try c.field("actions", or: [])
        }
    }

    /// A small panel anchored to what opened it.
    public struct Popover: Sendable, Equatable, Decodable {
        public var id: String
        public var children: [Node]

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.required("id")
            children = try c.field("children", or: [])
        }
    }

    /// A plugin's own view. A client that has no view of this name draws `alt`.
    public struct NativeView: Sendable, Equatable, Decodable {
        public var id: String?
        public var name: String
        public var props: JSONValue
        public var alt: Node?

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            name = try c.field("name", or: "")
            props = try c.field("props", or: .null)
            alt = try c.optional("alt")
        }
    }

    /// Markup for a small region.
    public struct WebRegion: Sendable, Equatable, Decodable {
        public var id: String?
        public var html: String

        public init(from decoder: any Decoder) throws {
            let c = try decoder.wire()
            id = try c.optional("id")
            html = try c.field("html", or: "")
        }
    }
}
