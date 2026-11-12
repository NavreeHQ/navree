package com.navree.client.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * One part of a screen. The kinds are closed: a kind this client does not
 * know decodes as [UnknownNode] and is drawn from its `alt` when it has one.
 * A field that holds its default is left out by the server, so every field
 * has one here.
 */
@Serializable(with = NodeSerializer::class)
sealed interface Node {
    val id: String?
}

// Structure

@Serializable
enum class Axis {
    @SerialName("vertical") Vertical,
    @SerialName("horizontal") Horizontal,
}

@Serializable
enum class Align {
    @SerialName("start") Start,
    @SerialName("center") Center,
    @SerialName("end") End,
    @SerialName("stretch") Stretch,
    @SerialName("baseline") Baseline,
}

@Serializable
enum class Justify {
    @SerialName("start") Start,
    @SerialName("center") Center,
    @SerialName("end") End,
    @SerialName("between") Between,
}

@Serializable
data class StackNode(
    override val id: String? = null,
    val axis: Axis = Axis.Vertical,
    /** The room between parts, in steps of the spacing scale: 0 is none. */
    val gap: Int = 0,
    val align: Align? = null,
    val justify: Justify? = null,
    /** Whether parts that do not fit go to a new line. */
    val wrap: Boolean = false,
    /** Not drawn until a command shows it. */
    val hidden: Boolean = false,
    val children: List<Node> = emptyList(),
) : Node

@Serializable
data class GridNode(
    override val id: String? = null,
    /** The most columns, on the widest screen. */
    val cols: Int? = null,
    /** The least a column may be wide, in density-independent units. */
    val min: Int? = null,
    val gap: Int = 0,
    val children: List<Node> = emptyList(),
) : Node

@Serializable
enum class SectionStyle {
    @SerialName("plain") Plain,
    @SerialName("card") Card,
    @SerialName("inset") Inset,
}

@Serializable
data class SectionNode(
    override val id: String? = null,
    val title: String? = null,
    val subtitle: String? = null,
    val style: SectionStyle = SectionStyle.Plain,
    val actions: List<Node> = emptyList(),
    val children: List<Node> = emptyList(),
    val footer: List<Node> = emptyList(),
) : Node

@Serializable
data class ListNode(
    override val id: String? = null,
    val children: List<Node> = emptyList(),
    /** The address of the next page of rows, by cursor; absent on the last page. */
    val more: String? = null,
    val ordered: Boolean = false,
) : Node

@Serializable
enum class RowState {
    @SerialName("done") Done,
    @SerialName("current") Current,
    @SerialName("upcoming") Upcoming,
}

@Serializable
data class RowNode(
    override val id: String? = null,
    val leading: Node? = null,
    val title: String = "",
    val subtitle: String? = null,
    /** More under the two lines. */
    val detail: List<Node> = emptyList(),
    val trailing: Node? = null,
    val badge: BadgeNode? = null,
    /** Where it stands in a sequence: a step of a timeline. */
    val state: RowState? = null,
    val action: Action? = null,
    val swipe: List<ButtonNode> = emptyList(),
    /** Buttons, sections of buttons and dividers. */
    val menu: List<Node> = emptyList(),
) : Node

@Serializable
data class SplitNode(
    override val id: String? = null,
    val primary: Node? = null,
    val detail: Node? = null,
) : Node

@Serializable
data class TabItem(
    val label: String = "",
    val icon: IconNode? = null,
    val selected: Boolean = false,
    val href: String? = null,
    val children: List<Node> = emptyList(),
)

@Serializable
data class TabsNode(
    override val id: String? = null,
    val items: List<TabItem> = emptyList(),
) : Node

@Serializable
data class DisclosureNode(
    override val id: String? = null,
    val label: String = "",
    val open: Boolean = false,
    val children: List<Node> = emptyList(),
) : Node

@Serializable
data class DividerNode(override val id: String? = null) : Node

@Serializable
data class SpacerNode(override val id: String? = null) : Node

/** A stream of events that says a region is out of date. */
@Serializable
data class Live(
    val src: String = "",
    val event: String? = null,
)

/**
 * A part of a screen with an address of its own. `lazy` with no children: it
 * is fetched from `src` once the screen is drawn, and the body of the answer
 * becomes its children. It is fetched again when the server says it is out of
 * date.
 */
@Serializable
data class RegionNode(
    override val id: String? = null,
    val src: String = "",
    val lazy: Boolean = false,
    val live: Live? = null,
    val children: List<Node> = emptyList(),
) : Node

// Content

@Serializable
enum class TextLevel {
    @SerialName("large_title") LargeTitle,
    @SerialName("title") Title,
    @SerialName("headline") Headline,
    @SerialName("body") Body,
    @SerialName("callout") Callout,
    @SerialName("caption") Caption,
    @SerialName("mono") Mono,
}

@Serializable
enum class Tone {
    @SerialName("neutral") Neutral,
    @SerialName("accent") Accent,
    @SerialName("success") Success,
    @SerialName("warning") Warning,
    @SerialName("danger") Danger,
    @SerialName("info") Info,
}

@Serializable
data class TextNode(
    override val id: String? = null,
    val value: String = "",
    val style: TextLevel = TextLevel.Body,
    val tone: Tone = Tone.Neutral,
    /** Said more quietly than what is around it. */
    val muted: Boolean = false,
    val lines: Int? = null,
    /** The level of the heading it is, from 1, for assistive technology. */
    val heading: Int? = null,
) : Node

@Serializable
data class Run(
    val text: String = "",
    val bold: Boolean = false,
    val italic: Boolean = false,
    val code: Boolean = false,
    val href: String? = null,
)

@Serializable
data class RichNode(
    override val id: String? = null,
    val runs: List<Run> = emptyList(),
    val style: TextLevel = TextLevel.Body,
    val muted: Boolean = false,
) : Node

@Serializable
enum class Fit {
    @SerialName("contain") Contain,
    @SerialName("cover") Cover,
}

@Serializable
data class ImageNode(
    override val id: String? = null,
    val src: String = "",
    val alt: String = "",
    val width: Int? = null,
    val height: Int? = null,
    val fit: Fit? = null,
) : Node

/**
 * An icon in a 24 by 24 box: `paths` are stroked (width 2, round caps and
 * joins) and `fills` are filled. `name` is the icon's name in its set, for a
 * client that has a system symbol for it.
 */
@Serializable
data class IconNode(
    override val id: String? = null,
    val name: String? = null,
    val paths: List<String> = emptyList(),
    val fills: List<String> = emptyList(),
    @SerialName("view_box") val viewBox: String? = null,
    val label: String? = null,
    val tone: Tone = Tone.Neutral,
    /** The shapes as SVG markup or as shape objects, from a server that has not split them. */
    val shapes: JsonElement? = null,
) : Node

@Serializable
data class AvatarNode(
    override val id: String? = null,
    val name: String = "",
    val src: String? = null,
    val initials: String? = null,
) : Node

@Serializable
data class BadgeNode(
    override val id: String? = null,
    @Serializable(with = LooseString::class) val text: String? = null,
    val tone: Tone = Tone.Neutral,
    val dot: Boolean = false,
) : Node

@Serializable
data class ProgressNode(
    override val id: String? = null,
    /** How far, in thousandths. Absent when it is not known. */
    val value: Int? = null,
    val label: String? = null,
) : Node

@Serializable
data class BannerNode(
    override val id: String? = null,
    val tone: Tone = Tone.Neutral,
    val title: String? = null,
    val text: String? = null,
    val icon: IconNode? = null,
    val actions: List<Node> = emptyList(),
) : Node

@Serializable
data class EmptyNode(
    override val id: String? = null,
    val icon: IconNode? = null,
    val title: String = "",
    val text: String? = null,
    val actions: List<Node> = emptyList(),
) : Node

@Serializable
data class Fact(
    val label: String = "",
    val value: List<Node> = emptyList(),
    val icon: IconNode? = null,
    val tone: Tone = Tone.Neutral,
    /** More lines under the value. */
    val details: List<String> = emptyList(),
)

@Serializable
data class FactsNode(
    override val id: String? = null,
    val items: List<Fact> = emptyList(),
) : Node

@Serializable
enum class ColumnRole {
    @SerialName("title") Title,
    @SerialName("subtitle") Subtitle,
    @SerialName("status") Status,
    @SerialName("trailing") Trailing,
    @SerialName("actions") Actions,
}

@Serializable
enum class SortDir {
    @SerialName("asc") Ascending,
    @SerialName("desc") Descending,
}

@Serializable
data class ColumnSort(
    val href: String = "",
    /** The direction the table is sorted in by this column now; absent when it is not. */
    val dir: SortDir? = null,
)

@Serializable
data class Column(
    val key: String? = null,
    val label: String = "",
    val numeric: Boolean = false,
    val align: Align? = null,
    val sort: ColumnSort? = null,
    /** What the column is to its row. */
    val role: ColumnRole? = null,
    /** Whether a narrow screen may leave it out. */
    val secondary: Boolean = false,
)

@Serializable
data class TableRow(
    val id: String? = null,
    val cells: List<Node> = emptyList(),
    val action: Action? = null,
)

@Serializable
data class TableNode(
    override val id: String? = null,
    val caption: String? = null,
    val columns: List<Column> = emptyList(),
    val rows: List<TableRow> = emptyList(),
    /** The form that searches and filters the table: a `get` to its action. */
    val query: FormNode? = null,
    val more: String? = null,
    val prev: String? = null,
    val summary: String? = null,
    /** What stands in the rows' place when there are none. */
    val empty: Node? = null,
) : Node

@Serializable
data class Series(
    val label: String = "",
    val values: List<Double?> = emptyList(),
    val shown: List<String> = emptyList(),
)

@Serializable
data class ChartNode(
    override val id: String? = null,
    val kind: String = "line",
    val title: String? = null,
    val labels: List<String> = emptyList(),
    val series: List<Series> = emptyList(),
) : Node

@Serializable
data class CodeNode(
    override val id: String? = null,
    val text: String = "",
    val lang: String? = null,
) : Node

// Controls

@Serializable
enum class ActionRole {
    @SerialName("none") None,
    @SerialName("primary") Primary,
    @SerialName("secondary") Secondary,
    @SerialName("destructive") Destructive,
    @SerialName("cancel") Cancel,
    @SerialName("confirm") Confirm,
    @SerialName("create") Create,
    @SerialName("navigation") Navigation,
}

@Serializable
enum class Prominence {
    @SerialName("high") High,
    @SerialName("medium") Medium,
    @SerialName("low") Low,
}

@Serializable
data class ButtonNode(
    override val id: String? = null,
    val label: String = "",
    val icon: IconNode? = null,
    /** Whether the page drew the icon alone. */
    @SerialName("icon_only") val iconOnly: Boolean = false,
    val role: ActionRole = ActionRole.None,
    val prominence: Prominence = Prominence.Medium,
    val disabled: Boolean = false,
    val pending: Boolean = false,
    val action: Action? = null,
) : Node

@Serializable
data class LinkNode(
    override val id: String? = null,
    val label: String = "",
    val action: Action? = null,
    val children: List<Node> = emptyList(),
) : Node

@Serializable
data class MenuNode(
    override val id: String? = null,
    val label: String = "",
    val icon: IconNode? = null,
    /** Buttons, sections of buttons and dividers. */
    val items: List<Node> = emptyList(),
) : Node

/** A value a form always sends. One with an id can be set by a command. */
@Serializable
data class Hidden(
    val name: String = "",
    @Serializable(with = LooseString::class) val value: String? = null,
    val id: String? = null,
)

/**
 * A form. Its submit is the button among its children whose action is
 * `submit`. A form with no method is a `get`.
 */
@Serializable
data class FormNode(
    override val id: String? = null,
    val method: String = "get",
    val action: String = "",
    @Serializable(with = HiddenFields::class) val hidden: List<Hidden> = emptyList(),
    val children: List<Node> = emptyList(),
) : Node {
    /** What the form goes by here: its id, or its address when it has none. */
    val key: String get() = id ?: "@$action"
}

@Serializable
data class FieldNode(
    override val id: String? = null,
    val name: String = "",
    val label: String? = null,
    val help: String? = null,
    @Serializable(with = LooseStringList::class) val error: List<String> = emptyList(),
    val required: Boolean = false,
    val disabled: Boolean = false,
    val input: Input = UnknownInput(),
) : Node

// Overlays

@Serializable
enum class SheetSize {
    @SerialName("medium") Medium,
    @SerialName("large") Large,
    @SerialName("full") Full,
}

@Serializable
data class SheetNode(
    override val id: String? = null,
    val title: String? = null,
    val subtitle: String? = null,
    val size: SheetSize = SheetSize.Large,
    val open: Boolean = false,
    val children: List<Node> = emptyList(),
    val actions: List<Node> = emptyList(),
) : Node

@Serializable
data class DialogNode(
    override val id: String? = null,
    val title: String? = null,
    val text: String? = null,
    /** A question with buttons only, asked before something that cannot be undone. */
    val alert: Boolean = false,
    val open: Boolean = false,
    val children: List<Node> = emptyList(),
    val actions: List<Node> = emptyList(),
) : Node

@Serializable
data class PopoverNode(
    override val id: String? = null,
    val children: List<Node> = emptyList(),
) : Node

// Escapes

/** A view a plugin draws by name. Without the plugin, `alt` is drawn. */
@Serializable
data class NativeNode(
    override val id: String? = null,
    val name: String = "",
    val props: JsonElement? = null,
    val alt: Node? = null,
) : Node

/** HTML for a small region. This client refuses it and shows the whole page in the web view. */
@Serializable
data class WebNode(
    override val id: String? = null,
    val html: String = "",
) : Node

/** A kind from a newer vocabulary. */
@Serializable
data class UnknownNode(
    override val id: String? = null,
    @SerialName("t") val kind: String = "",
    val alt: Node? = null,
) : Node
