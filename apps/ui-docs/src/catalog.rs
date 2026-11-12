//! What the docs show: the components, grouped the way people look for them,
//! and every example, with the Rust code that produced it.

use navree::ui::{LazyItem, Node, View};

/// A group of components on the docs' landing page and in the sidebar.
pub struct Group {
    pub id: &'static str,
    pub title: &'static str,
    pub components: &'static [&'static str],
}

/// The groups, in the order the docs list them.
pub const GROUPS: &[Group] = &[
    Group {
        id: "actions",
        title: "Actions",
        components: &[
            "button",
            "button-group",
            "toggle",
            "toggle-group",
            "dropdown-menu",
            "context-menu",
            "menubar",
        ],
    },
    Group {
        id: "inputs",
        title: "Inputs",
        components: &[
            "field",
            "input",
            "input-group",
            "textarea",
            "native-select",
            "select",
            "combobox",
            "multi-select",
            "checkbox",
            "switch",
            "radio-group",
            "phone-input",
            "input-otp",
            "color-picker",
            "questionnaire",
            "upload",
            "forms",
        ],
    },
    Group {
        id: "dates",
        title: "Date and time",
        components: &[
            "calendar",
            "date-picker",
            "date-range-picker",
            "time-picker",
            "date-time-picker",
        ],
    },
    Group {
        id: "display",
        title: "Display",
        components: &[
            "card",
            "badge",
            "avatar",
            "avatar-group",
            "trace",
            "item",
            "typography",
            "kbd",
            "attachment",
            "picture",
            "markdown",
            "code-block",
            "message",
            "timeline",
            "activity-feed",
            "fact-card",
        ],
    },
    Group {
        id: "feedback",
        title: "Feedback",
        components: &[
            "alert",
            "toast",
            "empty",
            "empty-state",
            "error-page",
            "forbidden",
            "not-found",
            "sign-in-required",
            "progress",
            "skeleton",
            "spinner",
        ],
    },
    Group {
        id: "overlays",
        title: "Overlays",
        components: &[
            "dialog",
            "alert-dialog",
            "sheet",
            "drawer",
            "popover",
            "tooltip",
            "hover-card",
        ],
    },
    Group {
        id: "layout",
        title: "Layout",
        components: &[
            "layout",
            "split-pane",
            "accordion",
            "collapsible",
            "scroll-area",
            "separator",
            "aspect-ratio",
            "carousel",
            "direction",
        ],
    },
    Group {
        id: "navigation",
        title: "Navigation",
        components: &[
            "app-shell",
            "tabs",
            "navigation-menu",
            "breadcrumb",
            "pagination",
            "command",
        ],
    },
    Group {
        id: "flow",
        title: "Steps, ranges and trees",
        components: &["stepper", "wizard", "slider", "tree-view", "toolbar"],
    },
    Group {
        id: "data",
        title: "Data",
        components: &["table", "data-table", "lazy-list", "stat", "sortable"],
    },
    Group {
        id: "charts",
        title: "Charts",
        components: &[
            "line-chart",
            "area-chart",
            "bar-chart",
            "donut-chart",
            "sparkline",
            "funnel",
            "ring",
            "rings",
            "gauge",
            "day-clock",
            "meter",
            "mini-columns",
            "color-strip",
            "range-scale",
            "trend",
            "calendar-heatmap",
            "matrix-heatmap",
            "relation-list",
            "scatter-chart",
        ],
    },
    Group {
        id: "app-blocks",
        title: "App blocks",
        components: &[
            "info-tip",
            "chart-skeleton",
            "breathing-guide",
            "dock",
            "day-pager",
        ],
    },
    Group {
        id: "marketing",
        title: "Marketing",
        components: &[
            "site-page",
            "site-header",
            "hero",
            "section",
            "logo-cloud",
            "feature-grid",
            "feature-rows",
            "stat-band",
            "device-frames",
            "testimonials",
            "pricing",
            "faq",
            "cta-band",
            "newsletter",
            "legal-page",
            "site-footer",
        ],
    },
    Group {
        id: "assistant",
        title: "Assistant",
        components: &[
            "conversation",
            "chat-message",
            "prompt-input",
            "tool-call-card",
            "approval-card",
            "surface-frame",
            "suggestions",
            "sources",
            "thread-list",
        ],
    },
];

/// A component, or a part of the toolkit with a page of its own.
pub struct Component {
    /// The page's slug, and the folder of its previews.
    pub id: &'static str,
    /// The name shown on the page and the landing grid.
    pub name: &'static str,
    /// The docs page.
    pub href: &'static str,
    /// The Rust types whose props tables the page shows, the first being the one
    /// whose doc comment is the component's summary.
    pub types: &'static [&'static str],
    /// The example shown small on the landing page.
    pub thumb: &'static str,
    /// The example whose parts the anatomy diagram draws.
    pub anatomy: Option<&'static str>,
    /// The line under the name when the first type's doc comment doesn't say it:
    /// several pages can show one type, each for another use of it.
    pub about: Option<&'static str>,
}

const fn component(
    id: &'static str,
    name: &'static str,
    types: &'static [&'static str],
    thumb: &'static str,
) -> Component {
    Component {
        id,
        name,
        href: "",
        types,
        thumb,
        anatomy: None,
        about: None,
    }
}

impl Component {
    const fn page(mut self, href: &'static str) -> Self {
        self.href = href;
        self
    }

    const fn parts(mut self, example: &'static str) -> Self {
        self.anatomy = Some(example);
        self
    }

    const fn about(mut self, summary: &'static str) -> Self {
        self.about = Some(summary);
        self
    }

    /// The page's URL.
    pub fn url(&self) -> String {
        if self.href.is_empty() {
            format!("/docs/components/{}/", self.id)
        } else {
            self.href.to_owned()
        }
    }
}

/// Every component and toolkit area with previews.
pub const COMPONENTS: &[Component] = &[
    // Actions
    component(
        "button",
        "Button",
        &["Button", "ButtonVariant", "ButtonKind", "Size"],
        "thumb",
    )
    .parts("icon"),
    component("button-group", "ButtonGroup", &["ButtonGroup"], "thumb").parts("default"),
    component("toggle", "Toggle", &["Toggle"], "thumb").parts("default"),
    component(
        "toggle-group",
        "ToggleGroup",
        &["ToggleGroup", "ToggleItem", "ToggleVariant", "Orientation"],
        "thumb",
    )
    .parts("default"),
    component(
        "dropdown-menu",
        "DropdownMenu",
        &[
            "DropdownMenu",
            "MenuItem",
            "MenuCheckbox",
            "MenuRadios",
            "MenuGroup",
            "MenuSub",
        ],
        "thumb",
    )
    .parts("default"),
    component("context-menu", "ContextMenu", &["ContextMenu"], "thumb").parts("default"),
    component("menubar", "Menubar", &["Menubar", "MenubarMenu"], "thumb").parts("default"),
    // Inputs
    component("field", "Field", &["Field", "Label", "FieldSet"], "thumb").parts("help"),
    component("input", "Input", &["Input", "InputKind"], "default").parts("default"),
    component("input-group", "InputGroup", &["InputGroup"], "default").parts("default"),
    component("textarea", "Textarea", &["Textarea"], "default").parts("default"),
    component(
        "native-select",
        "NativeSelect",
        &["NativeSelect", "OptionItem"],
        "default",
    )
    .parts("default"),
    component(
        "select",
        "Select",
        &["Select", "Choice", "SelectLabels"],
        "thumb",
    )
    .parts("default"),
    component(
        "combobox",
        "Combobox",
        &["Combobox", "OptionQuery", "ChoiceList"],
        "thumb",
    )
    .parts("default"),
    component("multi-select", "MultiSelect", &["MultiSelect"], "thumb").parts("default"),
    component("checkbox", "Checkbox", &["Checkbox"], "default").parts("default"),
    component("switch", "Switch", &["Switch"], "thumb").parts("default"),
    component("radio-group", "RadioGroup", &["RadioGroup"], "thumb").parts("default"),
    component(
        "phone-input",
        "PhoneInput",
        &["PhoneInput", "PhoneLabels"],
        "thumb",
    )
    .parts("thumb"),
    component(
        "upload",
        "Upload",
        &[
            "Upload",
            "UploadedFile",
            "UploadLimits",
            "UploadLabels",
            "Uploads",
        ],
        "thumb",
    )
    .parts("default"),
    component("forms", "Forms", &["FormView"], "thumb").page("/docs/ui/forms/"),
    component(
        "input-otp",
        "InputOtp",
        &["InputOtp", "OtpKind", "OtpRules", "OtpError"],
        "thumb",
    )
    .parts("default"),
    component(
        "color-picker",
        "ColorPicker",
        &["ColorPicker"],
        "default",
    ),
    component(
        "questionnaire",
        "Questionnaire",
        &["Questionnaire", "Question", "QuestionnaireMove", "QuestionnaireLabels"],
        "default",
    ),
    component("avatar-group", "AvatarGroup", &["AvatarGroup"], "default"),
    component(
        "trace",
        "Trace",
        &["Trace", "TraceStep", "TraceState", "TraceLabels"],
        "default",
    ),
    // Date and time
    component(
        "calendar",
        "Calendar",
        &["Calendar", "CalendarMode", "CaptionLayout", "DateRules"],
        "thumb",
    )
    .parts("default"),
    component(
        "date-picker",
        "DatePicker",
        &["DatePicker", "PickerStyle", "DateLocale"],
        "thumb",
    )
    .parts("default"),
    component(
        "date-range-picker",
        "DateRangePicker",
        &[
            "DateRangePicker",
            "DatePreset",
            "DateRange",
            "DateRangeQuery",
            "RangeRules",
        ],
        "thumb",
    )
    .parts("default"),
    component(
        "time-picker",
        "TimePicker",
        &["TimePicker", "TimeRules", "Clock"],
        "thumb",
    )
    .parts("default"),
    component(
        "date-time-picker",
        "DateTimePicker",
        &["DateTimePicker"],
        "thumb",
    )
    .parts("default"),
    // Display
    component("card", "Card", &["Card"], "thumb").parts("default"),
    component(
        "badge",
        "Badge",
        &["Badge", "BadgeVariant", "Tone"],
        "default",
    )
    .parts("default"),
    component("avatar", "Avatar", &["Avatar"], "default").parts("default"),
    component("item", "Item", &["Item", "ItemVariant"], "thumb").parts("default"),
    component(
        "typography",
        "Heading and Text",
        &["Heading", "Text", "TextVariant"],
        "thumb",
    ),
    component("kbd", "Kbd", &["Kbd"], "default").parts("default"),
    component("attachment", "Attachment", &["Attachment"], "default").parts("default"),
    component(
        "markdown",
        "Markdown",
        &["Markdown", "MarkdownTrust", "MarkdownLabels"],
        "thumb",
    ),
    component("code-block", "CodeBlock", &["CodeBlock"], "thumb").parts("default"),
    component(
        "message",
        "Message and Bubble",
        &["Message", "Bubble", "BubbleSide", "Marker", "MarkerVariant"],
        "thumb",
    )
    .parts("default"),
    component(
        "timeline",
        "Timeline",
        &["Timeline", "TimelineEvent", "TimelineState", "Meta"],
        "thumb",
    )
    .parts("default"),
    component(
        "activity-feed",
        "ActivityFeed",
        &["ActivityFeed", "ActivityItem", "FeedHeight"],
        "thumb",
    )
    .parts("default"),
    component(
        "fact-card",
        "FactCard and FactGrid",
        &["FactCard", "FactGrid"],
        "thumb",
    )
    .parts("default"),
    // Feedback
    component("alert", "Alert", &["Alert", "AlertTone"], "default").parts("default"),
    component(
        "toast",
        "Toast",
        &[
            "Toast",
            "ToastAction",
            "ToastDuration",
            "ToastPosition",
            "Toaster",
        ],
        "thumb",
    )
    .parts("action"),
    component("empty", "Empty", &["Empty"], "thumb").parts("default"),
    component(
        "empty-state",
        "EmptyState",
        &["EmptyState", "EmptyKind"],
        "thumb",
    )
    .parts("default"),
    component(
        "error-page",
        "ErrorPage",
        &["ErrorPage", "State", "StateKind", "StateSize"],
        "thumb",
    )
    .parts("default"),
    component("forbidden", "Forbidden", &["Forbidden"], "thumb").parts("default"),
    component("not-found", "NotFound", &["NotFound"], "thumb").parts("default"),
    component(
        "sign-in-required",
        "SignInRequired",
        &["SignInRequired", "SessionEnded", "ReturnTo"],
        "thumb",
    )
    .parts("default"),
    component("progress", "Progress", &["Progress"], "thumb").parts("default"),
    component(
        "skeleton",
        "Skeleton",
        &["Skeleton", "SkeletonShape"],
        "default",
    )
    .parts("default"),
    component("spinner", "Spinner", &["Spinner"], "default").parts("default"),
    // Overlays
    component("dialog", "Dialog", &["Dialog"], "thumb").parts("thumb"),
    component("alert-dialog", "AlertDialog", &["AlertDialog"], "thumb").parts("thumb"),
    component(
        "sheet",
        "Sheet",
        &["Sheet", "SheetSide", "SheetWidth", "SheetBackdrop"],
        "thumb",
    )
    .parts("thumb"),
    component("drawer", "Drawer", &["Drawer"], "thumb").parts("thumb"),
    component(
        "popover",
        "Popover",
        &["Popover", "Side", "PopupAlign"],
        "thumb",
    )
    .parts("default"),
    component("tooltip", "Tooltip", &["Tooltip"], "thumb").parts("thumb"),
    component("hover-card", "HoverCard", &["HoverCard"], "thumb").parts("default"),
    // Layout
    component(
        "layout",
        "Stack, Row, Grid and Container",
        &[
            "Stack",
            "Row",
            "Grid",
            "Container",
            "Align",
            "Justify",
            "ContainerSize",
        ],
        "thumb",
    )
    .page("/docs/ui/layout/")
    .parts("stack"),
    component(
        "split-pane",
        "SplitPane",
        &[
            "SplitPane",
            "Pane",
            "SplitWidth",
            "SplitHeight",
            "SplitSizes",
            "SplitSave",
        ],
        "thumb",
    )
    .parts("default"),
    component("accordion", "Accordion", &["Accordion"], "default").parts("default"),
    component("collapsible", "Collapsible", &["Collapsible"], "default").parts("default"),
    component(
        "scroll-area",
        "ScrollArea",
        &["ScrollArea", "ScrollHeight"],
        "default",
    )
    .parts("default"),
    component("separator", "Separator", &["Separator"], "default").parts("default"),
    component(
        "aspect-ratio",
        "AspectRatio",
        &["AspectRatio", "Ratio"],
        "default",
    )
    .parts("default"),
    component(
        "carousel",
        "Carousel",
        &[
            "Carousel",
            "CarouselSlide",
            "CarouselLabels",
            "CarouselHeight",
        ],
        "thumb",
    )
    .parts("default"),
    component("direction", "Direction", &["Direction"], "default").parts("default"),
    // Navigation
    component(
        "app-shell",
        "App shell",
        &[
            "Shell", "NavGroup", "NavItem", "UserMenu", "AppPage", "AppShell",
        ],
        "default",
    )
    .page("/docs/ui/app-shell/")
    .parts("default"),
    component(
        "tabs",
        "Tabs",
        &["Tabs", "Tab", "TabsVariant", "Activation", "Orientation"],
        "thumb",
    )
    .parts("default"),
    component(
        "navigation-menu",
        "NavigationMenu",
        &["NavigationMenu", "NavMenuItem", "NavMenuLink"],
        "thumb",
    )
    .parts("default"),
    component("breadcrumb", "Breadcrumb", &["Breadcrumb"], "default").parts("default"),
    component("pagination", "Pagination", &["Pagination"], "default").parts("default"),
    component(
        "command",
        "Command",
        &["Command", "CommandGroup", "CommandItem"],
        "thumb",
    )
    .parts("default"),
    // Steps, ranges and trees
    component(
        "stepper",
        "Stepper",
        &["Stepper", "Step", "StepState", "StepperLabels"],
        "thumb",
    )
    .parts("default"),
    component(
        "wizard",
        "Wizard",
        &[
            "Wizard",
            "WizardStep",
            "WizardStepOf",
            "WizardPage",
            "WizardOutcome",
            "WizardAction",
            "WizardLabels",
            "Draft",
            "MemoryDrafts",
        ],
        "thumb",
    )
    .parts("default"),
    component(
        "slider",
        "Slider",
        &["Slider", "SliderSpec", "SliderError", "SliderLabels"],
        "thumb",
    )
    .parts("default"),
    component(
        "tree-view",
        "TreeView",
        &[
            "Tree",
            "TreeNode",
            "TreeSelect",
            "TreeState",
            "TreeSelection",
            "TreeChildren",
            "TreeLabels",
        ],
        "thumb",
    )
    .parts("default"),
    component(
        "toolbar",
        "Toolbar",
        &["Toolbar", "ToolbarItem", "ToolbarLabels"],
        "thumb",
    )
    .parts("default"),
    // Data
    component(
        "table",
        "Table",
        &["Table", "TableHead", "TableRowEl", "TableCell"],
        "thumb",
    )
    .parts("default"),
    component(
        "data-table",
        "DataTable",
        &["DataTable", "BulkAction", "RowAction"],
        "thumb",
    )
    .page("/docs/ui/data-table/")
    .parts("default"),
    component(
        "lazy-list",
        "LazyList",
        &[
            "LazyList",
            "LazyItem",
            "LazyOrder",
            "LazyEnd",
            "LazyLabels",
            "LazyQuery",
            "LazyPage",
        ],
        "thumb",
    )
    .parts("default"),
    component("stat", "Stat and Bars", &["Stat", "Bars"], "thumb").parts("default"),
    component(
        "sortable",
        "Sortable",
        &[
            "Sortable",
            "SortList",
            "SortItem",
            "SortLabels",
            "Move",
            "ListState",
        ],
        "thumb",
    )
    .parts("default"),
    // Charts
    component(
        "line-chart",
        "Line chart",
        &[
            "Chart",
            "Series",
            "Annotation",
            "At",
            "NumberFormat",
            "TableMode",
        ],
        "thumb",
    )
    .about("Change over time: lines on a shared axis, with gaps, targets, events and bands.")
    .parts("default"),
    component(
        "area-chart",
        "Area chart",
        &["Chart", "Series", "Annotation"],
        "thumb",
    )
    .about("A line with a wash under it. Stacked, the parts of a total over time.")
    .parts("default"),
    component(
        "bar-chart",
        "Bar chart",
        &["Chart", "Series", "Annotation", "Merge"],
        "thumb",
    )
    .about("Bars from one baseline to compare amounts: grouped, stacked, negative, vertical or horizontal.")
    .parts("default"),
    component("donut-chart", "Donut chart", &["Donut"], "thumb")
        .about("One whole split into a few parts, with the total in the middle. A pie has no hole.")
        .parts("default"),
    component("sparkline", "Sparkline", &["Sparkline"], "thumb")
        .about("A small line with no axes, beside a figure: the shape of a series at a glance.")
        .parts("default"),
    component("funnel", "Funnel", &["Funnel", "Stage"], "thumb")
        .about("How many reach each stage of an ordered process, and what share of the first and of the stage before.")
        .parts("default"),
    component(
        "ring",
        "Ring",
        &["Ring", "RingTrack", "Screen", "RadialLabels"],
        "thumb",
    )
    .about("One value on a ring: a score out of 100, steps toward a goal, with the value in the middle.")
    .parts("default"),
    component("rings", "Rings", &["Rings", "RingValue", "RingTrack"], "thumb")
        .about("Two to six values on rings inside each other, each against its own goal.")
        .parts("default"),
    component("gauge", "Gauge", &["Gauge", "RingTrack", "Screen"], "thumb")
        .about("One value on an open arc, 270 degrees or any sweep, on a scale that may start above zero.")
        .parts("default"),
    component("day-clock", "Day clock", &["DayClock", "Moment"], "thumb")
        .about("A day round a clock face: a wedge for each hour, its length the value, its colour a category, with moments marked.")
        .parts("default"),
    component("meter", "Meter", &["Meter", "Segment", "LinearLabels"], "thumb")
        .about("One bar split into segments that add up to the whole, with the share inside the wide ones and a legend of values.")
        .parts("default"),
    component(
        "mini-columns",
        "Mini columns",
        &["MiniColumns", "MiniColumn"],
        "thumb",
    )
    .about("Small multiples: one column per item, each split into the same parts and drawn to 100%.")
    .parts("default"),
    component("color-strip", "Colour strip", &["ColorStrip"], "thumb")
        .about("A row of equal cells, each in the colour of its category or of the band its reading falls in.")
        .parts("default"),
    component("range-scale", "Range scale", &["RangeScale"], "thumb")
        .about("One value on a scale between two ends, with a marker and a track that can be a gradient.")
        .parts("default"),
    component("trend", "Trend", &["Trend"], "thumb")
        .about("Which way something moved and by how much: an arrow, a signed change and a colour for what it means.")
        .parts("default"),
    component(
        "calendar-heatmap",
        "Calendar heat map",
        &["CalendarHeatmap", "HeatSteps", "GridLabels"],
        "thumb",
    )
    .about("A day per cell in weeks, shaded in five steps: how consistent a habit is over months. A day with no data is hollow, not zero.")
    .parts("default"),
    component(
        "matrix-heatmap",
        "Matrix heat map",
        &["MatrixHeatmap", "Relation", "GridLabels"],
        "thumb",
    )
    .about("Every measure against every other, such as a correlation matrix: a tone for the sign, depth for the strength, neutral for chance and blank for untested.")
    .parts("default"),
    component(
        "relation-list",
        "Relation list",
        &["RelationList", "Relation", "GridLabels"],
        "thumb",
    )
    .about("Pairs of measures ranked by how strongly they move together: the signed value, a bar from the centre by sign and a note on each row.")
    .parts("default"),
    component(
        "scatter-chart",
        "Scatter chart",
        &["Scatter", "PointSeries", "GridLabels"],
        "thumb",
    )
    .about("A translucent point for each pair of values, in one series or several: a cloud of beats, steps against resting heart rate. Axes can be hidden and both can share a range.")
    .parts("default"),
    // App blocks
    component("info-tip", "Info tip", &["InfoTip", "InfoMark"], "thumb")
        .about("A small \"?\" or (i) chip that opens a note about the thing beside it: how to read a chart, what a score is made of."),
    component("chart-skeleton", "Chart skeleton", &["ChartSkeleton"], "thumb")
        .about("A loading placeholder shaped like the chart that is coming: a ring, a gauge, a line, bars, a calendar, a matrix or a meter."),
    component(
        "breathing-guide",
        "Breathing guide",
        &["BreathingGuide", "BreathingLabels"],
        "thumb",
    )
    .about("A paced 4-7-8 breathing circle that runs from CSS alone and holds still when the reader asks for less motion."),
    component("dock", "Dock", &["Dock", "DockItem"], "thumb")
        .about("A floating row of icon links pinned to the bottom of the page: the main places of an app, with the current one lit."),
    component("day-pager", "Day pager", &["DayPager"], "thumb")
        .about("The day before, the day shown and the day after, with the day's scores and a calendar behind the date."),
    // Assistant
    component(
        "conversation",
        "Conversation",
        &["Conversation", "ChatLabels"],
        "default",
    )
    .about("The messages of a conversation with an assistant, oldest first, loading earlier ones as the reader scrolls up and merging an answer as it is written."),
    component(
        "chat-message",
        "ChatMessage",
        &["ChatMessage", "ChatState", "ChatRole"],
        "default",
    ),
    component("prompt-input", "PromptInput", &["PromptInput"], "default"),
    component(
        "tool-call-card",
        "ToolCallCard and Reasoning",
        &["ToolCallCard", "ToolState", "Reasoning"],
        "default",
    )
    .about("A tool the assistant called, with whether it ran and its arguments folded, and what the assistant thought before it answered."),
    component(
        "approval-card",
        "ApprovalCard",
        &["ApprovalCard", "ApprovalState"],
        "default",
    ),
    component(
        "surface-frame",
        "SurfaceFrame",
        &["SurfaceFrame", "FrameLabels"],
        "default",
    ),
    component("suggestions", "Suggestions", &["Suggestions"], "default"),
    component(
        "sources",
        "Sources and Citation",
        &["Sources", "Citation"],
        "default",
    )
    .about("The links an answer drew on, with the host of each that leaves the site, and numbered references to them."),
    component("thread-list", "ThreadList", &["ThreadList"], "default"),
    // Pages that aren't components
    component("foundations", "Foundations", &["Size"], "radius").page("/docs/ui/foundations/"),
    component(
        "themes",
        "Themes",
        &[
            "Theme",
            "Appearance",
            "Mode",
            "ThemeSetting",
            "ThemeMenu",
            "ShellLayout",
            "Collapse",
        ],
        "built-in",
    )
    .page("/docs/ui/themes/"),
    component(
        "app-styles",
        "App styles",
        &["AppSheet", "AppFont", "FontDisplay"],
        "brand",
    )
    .page("/docs/ui/app-styles/"),
    component(
        "theme-builder",
        "Theme builder",
        &[
            "ThemeBuilder",
            "BuilderDraft",
            "ThemeSpec",
            "Palette",
            "Token",
            "Rgb",
            "Radius",
            "Density",
            "Font",
        ],
        "default",
    )
    .page("/docs/ui/theme-builder/"),
    component("icons", "Icons", &["Icon"], "usage").page("/docs/ui/icons/"),
    component(
        "flags",
        "Flags",
        &["Flag", "FlagShape", "FlagSize", "CountryCode", "CodeKind"],
        "usage",
    )
    .page("/docs/ui/flags/"),
    component("commands", "Commands", &["Cmd", "Op", "Reply"], "toggle")
        .page("/docs/ui/commands-and-ops/"),
    component("regions", "Regions", &["Region"], "lazy").page("/docs/ui/regions/"),
    component(
        "errors-and-states",
        "Errors and states",
        &[
            "PagesOptions",
            "StateLabels",
            "KindWords",
            "ErrorShell",
            "StateKind",
        ],
        "kinds",
    )
    .page("/docs/ui/errors-and-states/"),
    component(
        "reports",
        "Reports",
        &[
            "Report",
            "ReportQuery",
            "Bucket",
            "Grain",
            "Metric",
            "Bucketing",
            "TimeColumn",
        ],
        "thumb",
    )
    .page("/docs/ui/reports/"),
    // Marketing
    component("picture", "Picture", &["Picture"], "default").parts("default"),
    component(
        "site-page",
        "SitePage",
        &["SitePage", "Brand"],
        "default",
    )
    .parts("default"),
    component("site-header", "SiteHeader", &["SiteHeader", "Brand"], "default")
        .parts("default"),
    component("hero", "Hero", &["Hero", "Backdrop"], "default").parts("default"),
    component(
        "section",
        "Section",
        &["Section", "SectionHeading", "HeadingAlign", "SectionTone"],
        "default",
    )
    .parts("default"),
    component("logo-cloud", "LogoCloud", &["LogoCloud"], "default").parts("default"),
    component(
        "feature-grid",
        "FeatureGrid",
        &["FeatureGrid", "Feature"],
        "default",
    )
    .parts("default"),
    component(
        "feature-rows",
        "FeatureRows",
        &["FeatureRows", "FeatureRow"],
        "default",
    )
    .parts("default"),
    component("stat-band", "StatBand", &["StatBand"], "default").parts("default"),
    component(
        "device-frames",
        "Device frames",
        &["BrowserFrame", "PhoneFrame"],
        "default",
    )
    .parts("default")
    .about("A screenshot or a live part of the product in a browser window or a phone, light or dark."),
    component(
        "testimonials",
        "Testimonials",
        &["Testimonials", "Testimonial"],
        "default",
    )
    .parts("default"),
    component("pricing", "Pricing", &["Pricing", "Plan"], "default").parts("default"),
    component("faq", "Faq", &["Faq"], "default").parts("default"),
    component("cta-band", "CtaBand", &["CtaBand"], "default").parts("default"),
    component("newsletter", "Newsletter", &["Newsletter"], "default").parts("default"),
    component(
        "legal-page",
        "LegalPage",
        &["LegalPage", "LegalSection"],
        "default",
    )
    .parts("default"),
    component(
        "site-footer",
        "SiteFooter",
        &["SiteFooter", "FooterColumn", "ThemeToggle"],
        "default",
    )
    .parts("default"),
    // The example apps' views, shown on `/ui/` rather than a docs page.
    component(
        "build-anything",
        "Build anything",
        &["DataTable", "Sheet", "ActivityFeed"],
        "store-drawer",
    )
    .page("/ui/#build-anything"),
];

/// One cell of a searchable gallery.
pub struct Entry {
    /// The first line under the cell: a name or a code.
    pub caption: String,
    /// A second, quieter line.
    pub detail: Option<String>,
    /// What a search matches, in lower case.
    pub terms: String,
    /// What the cell shows.
    pub node: Node,
}

/// How an example is drawn.
pub enum Kind {
    /// One tree.
    Single(fn() -> Node),
    /// A whole page: its body, drawn in the page's look (its theme, mode and
    /// density on `<html>`, and the stylesheets of its own themes).
    Page(fn() -> View),
    /// Several trees, each with a caption.
    Gallery(fn() -> Vec<(&'static str, Node)>),
    /// Many cells and a search box that filters them. For galleries too long to scan.
    Search {
        /// The search box's name for assistive technology and its placeholder.
        label: &'static str,
        /// What one match is called, to say how many there are: `icons`.
        noun: &'static str,
        draw: fn() -> Vec<Entry>,
    },
}

/// A page a list's source would answer with, drawn ahead of time. A preview has
/// no server, so the preview script answers the list's requests from these, in
/// order, and with an empty last page once they run out.
pub struct Answer {
    /// The source's path, as the list names it: `/desk/rows`.
    pub src: &'static str,
    /// The end the page is for: `start` or `end`.
    pub dir: &'static str,
    /// The items, in the list's order.
    pub items: Vec<LazyItem>,
    /// Whether more lies beyond them.
    pub more: bool,
}

/// How the preview frame places what it shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Centred: controls and small pieces.
    Center,
    /// From the top left, full width: forms, tables, cards.
    Start,
    /// Edge to edge, no padding: the app shell.
    Fill,
}

impl Layout {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Start => "start",
            Self::Fill => "fill",
        }
    }
}

/// The width of a gallery's cells.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cells {
    /// About 140px: buttons, badges.
    Narrow,
    /// About 220px.
    Medium,
    /// The whole width, one per line: alerts, items.
    Wide,
    /// About 96px: icons.
    Icon,
    /// About 150px: flags with a name under them.
    Flag,
}

impl Cells {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Medium => "medium",
            Self::Wide => "wide",
            Self::Icon => "icon",
            Self::Flag => "flag",
        }
    }
}

/// One example: what it shows, and the function that builds it.
pub struct Example {
    pub component: &'static str,
    pub slug: &'static str,
    pub title: &'static str,
    pub kind: Kind,
    /// The function's body, as written.
    pub code: String,
    /// The preview's least height, in px.
    pub height: u32,
    pub layout: Layout,
    pub cells: Cells,
    /// Shows a closed dialog, sheet or popover in place, so the frame shows what
    /// it looks like before anyone clicks.
    pub show: bool,
    /// Draws each dialog or sheet whose parent is marked `data-preview-open` open
    /// at its own place over the page, with the scrim behind it: a drawer over its
    /// page.
    pub open: bool,
    /// Draws an app shell's sidebar open over the page, as it is on a phone after the
    /// button is pressed.
    pub sidebar_open: bool,
    /// Loads the core runtime: commands run in the preview.
    pub live: bool,
    /// Draws the live dot as it is in this state (`on`, `retry`, `paused`): a
    /// preview has no stream to follow, so every preview is drawn without one.
    pub streaming: Option<&'static str>,
    /// Drawn this many px wide and scaled to fit the column: for a whole page,
    /// whose layout changes below 768px.
    pub width: Option<u32>,
    /// The frame grows to what it shows. Off for a page drawn edge to edge,
    /// which fills a frame of its height, unless it asks for it again.
    pub fit: bool,
    /// What the sources of the lists in it would answer.
    pub answers: fn() -> Vec<Answer>,
}

impl Example {
    pub fn new(
        component: &'static str,
        slug: &'static str,
        title: &'static str,
        kind: Kind,
        code: impl Into<String>,
    ) -> Self {
        Self {
            component,
            slug,
            title,
            kind,
            code: code.into(),
            height: 120,
            layout: Layout::Center,
            cells: Cells::Narrow,
            show: false,
            open: false,
            sidebar_open: false,
            live: false,
            streaming: None,
            width: None,
            fit: true,
            answers: Vec::new,
        }
    }

    /// `component/slug`.
    pub fn id(&self) -> String {
        format!("{}/{}", self.component, self.slug)
    }

    pub fn height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    pub fn start(mut self) -> Self {
        self.layout = Layout::Start;
        self
    }

    pub fn fill(mut self) -> Self {
        self.layout = Layout::Fill;
        self.fit = false;
        self
    }

    /// A page drawn edge to edge that shows all of itself: the frame grows to
    /// it, rather than showing its top.
    pub fn fit(mut self) -> Self {
        self.fit = true;
        self
    }

    pub fn narrow(mut self) -> Self {
        self.cells = Cells::Narrow;
        self
    }

    pub fn medium(mut self) -> Self {
        self.cells = Cells::Medium;
        self
    }

    pub fn wide(mut self) -> Self {
        self.cells = Cells::Wide;
        self
    }

    pub fn show(mut self) -> Self {
        self.show = true;
        self
    }

    /// Draws the dialogs and sheets inside an element marked `data-preview-open`
    /// open over their page, as they look after a click.
    pub fn open(mut self) -> Self {
        self.open = true;
        self
    }

    /// Draws the app shell's sidebar open, as it is on a phone after its button is
    /// pressed (the page marks it `data-mobile-open`). Use it with a width under 768 px.
    pub fn sidebar_open(mut self) -> Self {
        self.sidebar_open = true;
        self
    }

    pub fn width(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn live(mut self) -> Self {
        self.live = true;
        self
    }

    /// Shows the live dot in `state`.
    pub fn streaming(mut self, state: &'static str) -> Self {
        self.streaming = Some(state);
        self
    }

    /// Answers the requests of the lists in it from `answers`.
    pub fn answers(mut self, answers: fn() -> Vec<Answer>) -> Self {
        self.answers = answers;
        self
    }
}

/// Top-level items of `source` as written, each from its attributes and doc
/// comment to its closing brace, one after another: an example app's own code,
/// shown as the app has it.
pub fn items(source: &str, starts: &[&str]) -> String {
    let source = source.replace("\r\n", "\n");
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for start in starts {
        let at = lines
            .iter()
            .position(|line| {
                line.starts_with(&format!("{start}("))
                    || line.starts_with(&format!("{start} {{"))
                    || line.starts_with(&format!("{start}<"))
            })
            .unwrap_or_else(|| panic!("no `{start}` at the start of a line"));
        let mut first = at;
        // Up through the doc comment and the attributes, an attribute that rustfmt
        // spread over several lines too.
        while first > 0 {
            let above = lines[first - 1];
            if above.starts_with("#[")
                || above.starts_with("///")
                || above.starts_with(")]")
                || above.starts_with("    ")
            {
                first -= 1;
            } else {
                break;
            }
        }
        let end = (at..lines.len())
            .find(|line| lines[*line] == "}")
            .unwrap_or_else(|| panic!("`{start}` doesn't end"));
        out.push(lines[first..=end].join("\n"));
    }
    out.join("\n\n")
}

/// The body of `fn name` in `source`, dedented: what the docs show as its code.
///
/// Examples are top-level functions formatted by rustfmt, so the body starts
/// after the first line of the signature that ends in `{` and ends at the first
/// line that is `}` alone.
pub fn body_of(source: &str, name: &str) -> String {
    let source = source.replace("\r\n", "\n");
    let lines: Vec<&str> = source.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.starts_with(&format!("fn {name}(")))
        .unwrap_or_else(|| panic!("no `fn {name}(` at the start of a line"));
    let open = (start..lines.len())
        .find(|at| lines[*at].ends_with('{'))
        .unwrap_or_else(|| panic!("`fn {name}` has no body"));
    let close = (open..lines.len())
        .find(|at| lines[*at] == "}")
        .unwrap_or_else(|| panic!("`fn {name}` doesn't end"));
    lines[open + 1..close]
        .iter()
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `single!(component, slug, title, function)`: an example drawn as one tree.
macro_rules! single {
    ($component:literal, $slug:literal, $title:literal, $function:ident) => {
        $crate::catalog::Example::new(
            $component,
            $slug,
            $title,
            $crate::catalog::Kind::Single(|| ::navree::ui::Node::list($function())),
            $crate::catalog::body_of(SOURCE, stringify!($function)),
        )
    };
}

/// `gallery!(component, slug, title, function)`: an example drawn as captioned
/// cells. The function returns `(caption, component)` pairs.
macro_rules! gallery {
    ($component:literal, $slug:literal, $title:literal, $function:ident) => {
        $crate::catalog::Example::new(
            $component,
            $slug,
            $title,
            $crate::catalog::Kind::Gallery(|| {
                $function()
                    .into_iter()
                    .map(|(caption, node)| (caption, ::navree::ui::Node::list(node)))
                    .collect()
            }),
            $crate::catalog::body_of(SOURCE, stringify!($function)),
        )
        .medium()
    };
}

/// `page!(component, slug, title, function)`: an example of a whole page in the
/// app shell. The function returns an `AppPage`; the frame shows its body, in
/// the page's look.
macro_rules! page {
    ($component:literal, $slug:literal, $title:literal, $function:ident) => {
        $crate::catalog::Example::new(
            $component,
            $slug,
            $title,
            $crate::catalog::Kind::Page(|| $function().to_view()),
            $crate::catalog::body_of(SOURCE, stringify!($function)),
        )
    };
}

pub(crate) use {gallery, page, single};
