package com.navree.client.design

import com.navree.client.protocol.Action
import com.navree.client.protocol.BadgeNode
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.ChoiceKind
import com.navree.client.protocol.Column
import com.navree.client.protocol.ColumnRole
import com.navree.client.protocol.DateKind
import com.navree.client.protocol.FormNode
import com.navree.client.protocol.LinkNode
import com.navree.client.protocol.Node
import com.navree.client.protocol.SheetSize
import com.navree.client.protocol.Submit
import com.navree.client.protocol.TableRow
import com.navree.client.protocol.TextKind
import com.navree.client.protocol.TextLevel
import com.navree.client.protocol.TextNode
import com.navree.client.protocol.Tone
import com.navree.client.runtime.children
import com.navree.client.runtime.walk

/** A row of a table, read as a list row. */
data class TableRowAsListRow(
    /** The first line: what names the record. */
    val headline: String?,
    /** The lines under it: the rest of the naming cell, then the other columns a narrow screen keeps. */
    val lines: List<Node>,
    /** Status badges, in a row of their own. */
    val badges: List<Node>,
    /** The value at the row's end: the number of the row. */
    val trailing: Node?,
    /** What a tap on the row does. */
    val action: Action?,
)

/**
 * Lists and tables.
 *
 * Material 3 has no data table, and a phone has no room for one. Below 600 dp
 * of content a table is a list: each row a list item, the columns folded into
 * its lines. From 600 dp it is drawn as rows of cells under a header row.
 */
object ListRules {
    /** The least width, in dp, at which a table is drawn as a table. */
    const val TABLE_FROM_DP = 600

    /** The least a column gets, in dp. A table with more columns than fit is a list at any width. */
    const val COLUMN_LEAST_DP = 112

    fun tableAsList(contentWidthDp: Int, columns: Int): Boolean =
        contentWidthDp < TABLE_FROM_DP || columns * COLUMN_LEAST_DP > contentWidthDp

    /**
     * Folds a row's cells into a list row.
     *
     * - The column that names the record (its role is `title`, or it is the
     *   first) gives the first line. When that cell holds one control (a
     *   link, a button, a form with nothing but its button) and the row has
     *   no action of its own, the whole row takes that control's action and
     *   its label is the first line: on a phone a row is one touch target.
     * - A column a narrow screen may leave out is left out.
     * - Badges go in a row of their own; a `trailing` column, or else the
     *   first numeric one, goes to the row's end; the rest are lines.
     */
    fun asListRow(columns: List<Column>, row: TableRow): TableRowAsListRow {
        val cells = row.cells
        val titleAt = columns.indexOfFirst { it.role == ColumnRole.Title }.takeIf { it >= 0 } ?: 0
        val kept = cells.indices.filter { it != titleAt && columns.getOrNull(it)?.secondary != true }
        val trailingAt = kept.firstOrNull { columns.getOrNull(it)?.role == ColumnRole.Trailing }
            ?: kept.firstOrNull { columns.getOrNull(it)?.numeric == true }
            ?: kept.lastOrNull()?.takeIf { columns.none { column -> column.role != null || column.numeric } && cells.size >= 3 }
        val badges = kept.filter { it != trailingAt && (columns.getOrNull(it)?.role == ColumnRole.Status || cells[it] is BadgeNode) }
        val others = kept.filter { it != trailingAt && it !in badges }

        val title = cells.getOrNull(titleAt)
        val texts = title?.walk()?.filterIsInstance<TextNode>()?.filter { it.value.isNotBlank() }?.toList().orEmpty()
        val control = if (row.action == null) singleControl(title) else null
        val headline = control?.first ?: texts.firstOrNull()?.value ?: control?.first
        val rest = if (control != null) texts else texts.drop(1)
        return TableRowAsListRow(
            headline = headline,
            lines = rest + others.map { cells[it] },
            badges = badges.map { cells[it] },
            trailing = trailingAt?.let { cells[it] },
            action = row.action ?: control?.second,
        )
    }

    /** The label and the action of the one control a cell holds, when it holds exactly one. */
    private fun singleControl(cell: Node?): kotlin.Pair<String, Action>? {
        if (cell == null) return null
        val found = ArrayList<kotlin.Pair<String, Action>>()
        fun visit(node: Node, form: FormNode?) {
            when (node) {
                is ButtonNode -> {
                    val action = node.action
                    when {
                        action is Submit && action.form == null && form != null -> found += node.label to action.copy(form = form.key)
                        action != null -> found += node.label to action
                    }
                }
                is LinkNode -> node.action?.let { found += node.label to it }
                // A button inside a form sends that form.
                is FormNode -> for (child in node.children) visit(child, node)
                else -> for (child in node.children()) visit(child, form)
            }
        }
        visit(cell, null)
        return found.singleOrNull()
    }

    /**
     * How many columns a grid gets: as many as fit at the least width it
     * asks for (160 dp when it does not say), never more than it allows, and
     * at least one.
     */
    fun gridColumns(contentWidthDp: Int, minColumnDp: Int?, gapDp: Int, most: Int? = null): Int {
        val least = (minColumnDp ?: 160).coerceAtLeast(48)
        val fit = ((contentWidthDp + gapDp) / (least + gapDp)).coerceAtLeast(1)
        return if (most != null && most > 0) minOf(fit, most) else fit
    }
}

/** The control a choice is drawn with. */
enum class ChoiceWidget {
    /** A connected row of segmented buttons. */
    Segmented,

    /** Radio buttons, one under the other. */
    Radio,

    /** A text field that opens a menu of the options under it. */
    Menu,

    /** A dialog with the options as a list of radio buttons. */
    Dialog,
}

/** The control a set of choices is drawn with. */
enum class ChoicesWidget {
    /** Filter chips that wrap. */
    Chips,

    /** A dialog with the options as a list of checkboxes. */
    Dialog,
}

/** How a date or time is picked. */
enum class PickerMode {
    /** The calendar, or the clock dial. */
    Picker,

    /** Typed, because the window is too short for the calendar or the dial. */
    Input,
}

/** What the keyboard should be for a text field. */
data class KeyboardPlan(
    /** `text`, `email`, `password`, `number`, `decimal`, `phone` or `uri`. */
    val type: String,
    /** `none`, `words` or `sentences`. */
    val capitalise: String,
    val autoCorrect: Boolean,
    val singleLine: Boolean,
)

/**
 * Forms: which Material control each input becomes.
 *
 * - Text is an outlined text field with its label in the outline, help and
 *   errors as supporting text under it, a counter when there is a most, and a
 *   show-or-hide button for a password.
 * - One choice of a few is a segmented row (up to four on a compact window,
 *   five on wider ones) or radio buttons (up to five); otherwise a field that
 *   opens a menu, and past fifteen options a dialog with a list, because a
 *   menu that long cannot be scanned.
 * - Several choices are filter chips up to eight, and past that a dialog with
 *   checkboxes.
 * - A date opens the calendar in a dialog and a time the clock dial; in a
 *   short window both are typed instead.
 * - A switch sits at the end of its row and the whole row toggles it; a
 *   checkbox sits at the start.
 * - Fields do not stretch across a wide window: a form is at most 560 dp wide.
 */
object FormRules {
    const val FORM_MOST_DP = 560

    fun choice(kind: ChoiceKind, options: Int, window: WindowInfo): ChoiceWidget = when (kind) {
        ChoiceKind.Segmented -> {
            val most = if (window.width == WidthClass.Compact) 4 else 5
            if (options <= most && !window.largestText) ChoiceWidget.Segmented else menuOrDialog(options)
        }
        ChoiceKind.Radio -> if (options <= 5) ChoiceWidget.Radio else menuOrDialog(options)
        ChoiceKind.Menu -> menuOrDialog(options)
        ChoiceKind.List -> ChoiceWidget.Dialog
    }

    private fun menuOrDialog(options: Int): ChoiceWidget = if (options <= 15) ChoiceWidget.Menu else ChoiceWidget.Dialog

    fun choices(options: Int): ChoicesWidget = if (options <= 8) ChoicesWidget.Chips else ChoicesWidget.Dialog

    @Suppress("UNUSED_PARAMETER")
    fun picker(kind: DateKind, window: WindowInfo): PickerMode =
        if (window.height == HeightClass.Compact) PickerMode.Input else PickerMode.Picker

    fun keyboard(kind: TextKind): KeyboardPlan = when (kind) {
        TextKind.Plain -> KeyboardPlan("text", "sentences", autoCorrect = true, singleLine = true)
        TextKind.Multiline -> KeyboardPlan("text", "sentences", autoCorrect = true, singleLine = false)
        TextKind.Email -> KeyboardPlan("email", "none", autoCorrect = false, singleLine = true)
        TextKind.Password -> KeyboardPlan("password", "none", autoCorrect = false, singleLine = true)
        TextKind.Number -> KeyboardPlan("number", "none", autoCorrect = false, singleLine = true)
        TextKind.OneTimeCode -> KeyboardPlan("number", "none", autoCorrect = false, singleLine = true)
        TextKind.Decimal -> KeyboardPlan("decimal", "none", autoCorrect = false, singleLine = true)
        TextKind.Phone -> KeyboardPlan("phone", "none", autoCorrect = false, singleLine = true)
        TextKind.Url -> KeyboardPlan("uri", "none", autoCorrect = false, singleLine = true)
        TextKind.Search -> KeyboardPlan("text", "none", autoCorrect = true, singleLine = true)
    }

    /** A name that reads like a person's or a place's is capitalised by words. */
    fun capitalise(autocomplete: String?, plan: KeyboardPlan): String = when (autocomplete) {
        "name", "given-name", "family-name", "additional-name", "organization", "address-level1", "address-level2" -> "words"
        "username", "email" -> "none"
        else -> plan.capitalise
    }

    /** A label, with the mark a required field carries. */
    fun label(label: String?, required: Boolean): String? = when {
        label == null -> null
        required -> "$label *"
        else -> label
    }
}

/** How a sheet of the tree sits in its bottom sheet. */
data class SheetPlan(
    /** Whether the sheet may rest half open before it is dragged up. */
    val partial: Boolean,
    /** Whether it fills the height when it is fully open. */
    val fullHeight: Boolean,
)

/** Overlays that come in the tree. */
object OverlayRules {
    fun sheet(size: SheetSize, window: WindowInfo): SheetPlan = when (size) {
        // A short window has no room for a half-open sheet.
        SheetSize.Medium -> SheetPlan(partial = window.height != HeightClass.Compact, fullHeight = false)
        SheetSize.Large -> SheetPlan(partial = false, fullHeight = false)
        SheetSize.Full -> SheetPlan(partial = false, fullHeight = true)
    }

    /** Dialog buttons sit side by side while there are two short ones; more, or long ones, stack. */
    fun dialogButtonsStack(labels: List<String>, window: WindowInfo): Boolean =
        labels.size > 2 || labels.sumOf { it.length } > 28 || (window.largestText && labels.sumOf { it.length } > 16)
}

/** The Material type role a text level is set in. */
enum class TypeRole { HeadlineMedium, TitleLarge, TitleMedium, BodyLarge, BodyMedium, BodySmall, Mono }

/** The pair of colour roles a tone is drawn with: a container and what sits on it. */
enum class ToneRole { Surface, Primary, Success, Warning, Error, Info }

/** Type, colour roles and spacing: the system's own scales, chosen by what the tree says a thing is. */
object ContentRules {
    fun type(level: TextLevel): TypeRole = when (level) {
        TextLevel.LargeTitle -> TypeRole.HeadlineMedium
        TextLevel.Title -> TypeRole.TitleLarge
        TextLevel.Headline -> TypeRole.TitleMedium
        TextLevel.Body -> TypeRole.BodyLarge
        TextLevel.Callout -> TypeRole.BodyMedium
        TextLevel.Caption -> TypeRole.BodySmall
        TextLevel.Mono -> TypeRole.Mono
    }

    fun tone(tone: Tone): ToneRole = when (tone) {
        Tone.Neutral -> ToneRole.Surface
        Tone.Accent -> ToneRole.Primary
        Tone.Success -> ToneRole.Success
        Tone.Warning -> ToneRole.Warning
        Tone.Danger -> ToneRole.Error
        Tone.Info -> ToneRole.Info
    }

    /** A gap of the tree in dp: a step of the spacing scale is 4 dp, Material's grid. */
    fun gap(steps: Int): Int = steps.coerceIn(0, 16) * 4

    /** The margin at the sides of a screen's content: 16 dp on a compact window, 24 dp on wider ones. */
    fun marginDp(window: WindowInfo): Int = if (window.width == WidthClass.Compact) 16 else 24

    /** The most a single column of content spans, in dp, so lines stay readable in a wide window. */
    const val CONTENT_MOST_DP = 840

    /** Label and value sit side by side until the text is large; then the value goes under its label. */
    fun factsStacked(window: WindowInfo): Boolean = window.largeText
}
