package com.navree.client.design

import com.navree.protocol.Navree

/** A row of a table, read as a list row. */
data class TableRowAsListRow(
    /** What leads the row: a picture, an avatar, an icon. */
    val leading: Navree.Node?,
    /** The first line: what names the record. */
    val headline: String?,
    /** The lines under it: the rest of the naming cell, then the other columns a narrow screen keeps. */
    val lines: List<Navree.Node>,
    /** Status words, on a line of their own. */
    val statuses: List<Navree.Node>,
    /** The value at the row's end: the number of the row. */
    val trailing: Navree.Node?,
    /** The row's own actions, offered from a menu at its end and a long press. */
    val menu: List<Navree.Node>,
    /** What a tap on the row does. */
    val action: Navree.Action?,
)

/**
 * Lists and tables.
 *
 * Material 3 has no data table, and a phone has no room for one. Below 600 dp
 * of content a table is a list: each row a list item, the columns folded into
 * its parts by the role the server gave each column. From 600 dp it is drawn
 * as rows of cells under a header row.
 */
object ListRules {
    /** The least width, in dp, at which a table is drawn as a table. */
    const val TABLE_FROM_DP = 600

    /** The least a column gets, in dp. A table with more columns than fit is a list at any width. */
    const val COLUMN_LEAST_DP = 112

    fun tableAsList(contentWidthDp: Int, columns: Int): Boolean =
        contentWidthDp < TABLE_FROM_DP || columns * COLUMN_LEAST_DP > contentWidthDp

    /**
     * Whether the last line of a grid, when it has fewer parts than the
     * others, stretches its parts across the whole line. Parts that are whole
     * things in themselves (cards and figures) do, so no cell stands empty
     * beside them; parts that are not (words, rings, fields) keep the width
     * of the other lines' columns, as Material's own grids do.
     */
    fun stretchesLastLine(children: List<Navree.Node>): Boolean =
        children.isNotEmpty() && children.all { it is Navree.Card || it is Navree.Figure }

    /**
     * Folds a row's cells into a list row, by the role of each column.
     *
     * - `title` (or else the first column) gives the first line. When that
     *   cell holds one control, its label is the first line, and a row with
     *   no action of its own takes the control's action: on a phone a row is
     *   one touch target.
     * - `leading` leads the row; `subtitle` gives the lines under the title;
     *   `status` goes on a line of its own (and so does a badge whose column
     *   says nothing); `trailing` (or else the first numeric column) goes to
     *   the row's end; `actions` and the row's own actions are its menu.
     * - A column a narrow screen may leave out (`secondary`) is left out.
     */
    fun asListRow(columns: List<Navree.Column>, row: Navree.TableRow): TableRowAsListRow {
        val cells = row.cells
        fun role(index: Int) = columns.getOrNull(index)?.role
        val titleAt = columns.indexOfFirst { it.role == Navree.ColumnRole.TITLE }.takeIf { it >= 0 && it < cells.size } ?: 0
        val kept = cells.indices.filter { it != titleAt && columns.getOrNull(it)?.secondary != true }
        val leadingAt = kept.firstOrNull { role(it) == Navree.ColumnRole.LEADING }
        val actionsAt = kept.filter { role(it) == Navree.ColumnRole.ACTIONS }
        val trailingAt = kept.firstOrNull { role(it) == Navree.ColumnRole.TRAILING }
            ?: kept.firstOrNull { role(it) == null && columns.getOrNull(it)?.numeric == true }
            ?: kept.lastOrNull()?.takeIf { columns.none { column -> column.role != null || column.numeric } && cells.size >= 3 }
        val taken = setOfNotNull(leadingAt, trailingAt) + actionsAt
        val statuses = kept.filter { it !in taken && (role(it) == Navree.ColumnRole.STATUS || (role(it) == null && cells[it] is Navree.Badge)) }
        val subtitles = kept.filter { it !in taken && it !in statuses && role(it) == Navree.ColumnRole.SUBTITLE }
        val others = kept.filter { it !in taken && it !in statuses && it !in subtitles }

        val title = cells.getOrNull(titleAt)
        val texts = title?.walk()?.filterIsInstance<Navree.Text>()?.filter { it.value.isNotBlank() }?.toList().orEmpty()
        // A control in the naming cell names the record (an order's number) even when the row opens by itself.
        val control = singleControl(title)
        val headline = control?.first ?: texts.firstOrNull()?.value
        val rest = if (control != null) texts else texts.drop(1)
        val menu = actionsAt.flatMap { at -> menuItems(cells[at]) } + row.actions
        return TableRowAsListRow(
            leading = leadingAt?.let { cells[it] },
            headline = headline,
            // A line that says what a line above it already says is said once.
            lines = (rest + subtitles.map { cells[it] } + others.map { cells[it] }).distinctBy { (it as? Navree.Text)?.value?.trim() ?: it },
            statuses = statuses.map { cells[it] },
            trailing = trailingAt?.let { cells[it] },
            menu = menu,
            action = row.action ?: control?.second,
        )
    }

    /** The items of an actions cell: a menu's items, or the buttons it holds. */
    private fun menuItems(cell: Navree.Node): List<Navree.Node> = when (cell) {
        is Navree.Menu -> cell.items
        is Navree.Button, is Navree.Link -> listOf(cell)
        else -> cell.childNodes().flatMap(::menuItems)
    }

    /** The label and the action of the one control a cell holds, when it holds exactly one. */
    private fun singleControl(cell: Navree.Node?): Pair<String, Navree.Action>? {
        if (cell == null) return null
        val found = ArrayList<Pair<String, Navree.Action>>()
        fun visit(node: Navree.Node, form: Navree.Form?) {
            when (node) {
                is Navree.Button -> {
                    val action = node.action
                    when {
                        action is Navree.Action.Submit && action.form == null && form?.id != null -> found += node.label to action.copy(form = form.id)
                        action != null -> found += node.label to action
                    }
                }
                is Navree.Link -> found += node.label to node.action
                // A button inside a form sends that form.
                is Navree.Form -> for (child in node.children) visit(child, node)
                else -> for (child in node.childNodes()) visit(child, form)
            }
        }
        visit(cell, null)
        return found.singleOrNull()
    }

    /**
     * How many columns a grid gets: as many as fit at the least width it
     * asks for (160 dp when it does not say), never more than it allows, and
     * at least one. The least width grows with the person's text size, so a
     * number is never broken across lines to keep a column.
     */
    fun gridColumns(contentWidthDp: Int, minColumnDp: Int?, gapDp: Int, most: Int? = null, fontScale: Float = 1f, count: Int = Int.MAX_VALUE): Int {
        // A grid that gives a count and no least width is that many columns: nothing makes it give one up.
        if (minColumnDp == null && most != null && most > 0) return minOf(most, count.coerceAtLeast(1))
        val least = ((minColumnDp ?: 160) * fontScale.coerceAtLeast(1f)).toInt().coerceAtLeast(48)
        val fit = ((contentWidthDp + gapDp) / (least + gapDp)).coerceAtLeast(1)
        return if (most != null && most > 0) minOf(fit, most) else fit
    }

    /**
     * The share of a wide table's width a column takes: the column that names
     * the record the most, numbers and statuses less, a row's actions least.
     * A `subtitle` column takes none: its words stand under the title.
     */
    fun columnWeight(column: Navree.Column?): Float = when {
        column == null -> 1f
        column.role == Navree.ColumnRole.SUBTITLE -> 0f
        column.role == Navree.ColumnRole.TITLE -> 1.6f
        column.role == Navree.ColumnRole.ACTIONS -> 0.5f
        column.role == Navree.ColumnRole.TRAILING || column.numeric -> 0.9f
        else -> 1f
    }

    /**
     * Whether a row's trailing value moves under its text. At large text sizes
     * a value kept beside the text squeezes the text into three lines; under
     * it, both read whole.
     */
    fun trailingBelow(window: WindowInfo): Boolean = window.largestText
}

/** The control a choice is drawn with. */
enum class ChoiceWidget {
    /** A connected button group (Material 3's replacement for segmented buttons). */
    ButtonGroup,

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
 * - One choice of a few is a connected button group (up to four on a compact
 *   window, five on wider ones) or radio buttons (up to five); otherwise a
 *   field that opens a menu, and past fifteen options a dialog with a list,
 *   because a menu that long cannot be scanned.
 * - Several choices are filter chips up to eight, and past that a dialog with
 *   checkboxes.
 * - A date opens the calendar in a dialog and a time the clock dial; in a
 *   short window both are typed instead.
 * - A switch sits at the end of its row and the whole row toggles it; a
 *   checkbox sits at the start.
 * - Fields do not stretch across a wide window: a form is at most 560 dp wide.
 * - The keyboard's action key says "Next" on every field but the last, which
 *   says "Done" (or "Search" for a search).
 */
object FormRules {
    const val FORM_MOST_DP = 560

    fun choice(kind: Navree.ChoiceKind, options: Int, window: WindowInfo): ChoiceWidget = when (kind) {
        Navree.ChoiceKind.SEGMENTED -> {
            val most = if (window.width == WidthClass.Compact) 4 else 5
            if (options <= most && !window.largestText) ChoiceWidget.ButtonGroup else menuOrDialog(options)
        }
        Navree.ChoiceKind.RADIO -> if (options <= 5) ChoiceWidget.Radio else menuOrDialog(options)
        Navree.ChoiceKind.MENU -> menuOrDialog(options)
        Navree.ChoiceKind.LIST -> ChoiceWidget.Dialog
    }

    private fun menuOrDialog(options: Int): ChoiceWidget = if (options <= 15) ChoiceWidget.Menu else ChoiceWidget.Dialog

    fun choices(options: Int): ChoicesWidget = if (options <= 8) ChoicesWidget.Chips else ChoicesWidget.Dialog

    fun picker(window: WindowInfo): PickerMode = if (window.height == HeightClass.Compact) PickerMode.Input else PickerMode.Picker

    fun keyboard(kind: Navree.TextKind): KeyboardPlan = when (kind) {
        Navree.TextKind.PLAIN -> KeyboardPlan("text", "sentences", autoCorrect = true, singleLine = true)
        Navree.TextKind.MULTILINE -> KeyboardPlan("text", "sentences", autoCorrect = true, singleLine = false)
        Navree.TextKind.EMAIL -> KeyboardPlan("email", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.PASSWORD -> KeyboardPlan("password", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.NUMBER -> KeyboardPlan("number", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.ONE_TIME_CODE -> KeyboardPlan("number", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.DECIMAL -> KeyboardPlan("decimal", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.PHONE -> KeyboardPlan("phone", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.URL -> KeyboardPlan("uri", "none", autoCorrect = false, singleLine = true)
        Navree.TextKind.SEARCH -> KeyboardPlan("text", "none", autoCorrect = true, singleLine = true)
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

    /** What the keyboard's action key does on a single-line field: go on, finish, or search. */
    fun imeAction(kind: Navree.TextKind, last: Boolean): String = when {
        kind == Navree.TextKind.SEARCH -> "search"
        last -> "done"
        else -> "next"
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
    /** The most a bottom sheet spans on a wide window, in dp: it does not stretch. */
    const val SHEET_MOST_DP = 640

    fun sheet(size: Navree.SheetSize, window: WindowInfo): SheetPlan = when (size) {
        // A short window has no room for a half-open sheet.
        Navree.SheetSize.MEDIUM -> SheetPlan(partial = window.height != HeightClass.Compact, fullHeight = false)
        Navree.SheetSize.LARGE -> SheetPlan(partial = false, fullHeight = false)
        Navree.SheetSize.FULL -> SheetPlan(partial = false, fullHeight = true)
    }

    /** Dialog buttons sit side by side while there are two short ones; more, or long ones, stack. */
    fun dialogButtonsStack(labels: List<String>, window: WindowInfo): Boolean =
        labels.size > 2 || labels.sumOf { it.length } > 28 || (window.largestText && labels.sumOf { it.length } > 16)
}

/** The Material type role a text style is set in. */
enum class TypeRole { DisplayMedium, HeadlineMedium, HeadlineSmall, TitleLarge, TitleMedium, BodyLarge, BodyMedium, BodySmall, Mono }

/** The pair of colour roles a tone is drawn with: a container and what sits on it. */
enum class ToneRole { Surface, Primary, Success, Warning, Error, Info }

/** Type, colour roles and spacing: the system's own scales, chosen by what the tree says a thing is. */
object ContentRules {
    fun type(style: Navree.TextStyle): TypeRole = when (style) {
        // The one figure a screen is about.
        Navree.TextStyle.DISPLAY -> TypeRole.DisplayMedium
        Navree.TextStyle.LARGE_TITLE -> TypeRole.HeadlineMedium
        // A title and the smaller second title are two steps of the scale, not one.
        Navree.TextStyle.TITLE -> TypeRole.HeadlineSmall
        Navree.TextStyle.TITLE2 -> TypeRole.TitleLarge
        Navree.TextStyle.HEADLINE -> TypeRole.TitleMedium
        Navree.TextStyle.BODY -> TypeRole.BodyLarge
        Navree.TextStyle.CALLOUT -> TypeRole.BodyMedium
        Navree.TextStyle.CAPTION -> TypeRole.BodySmall
        Navree.TextStyle.MONO -> TypeRole.Mono
    }

    fun tone(tone: Navree.Tone?): ToneRole = when (tone) {
        null, Navree.Tone.NEUTRAL -> ToneRole.Surface
        Navree.Tone.ACCENT -> ToneRole.Primary
        Navree.Tone.SUCCESS -> ToneRole.Success
        Navree.Tone.WARNING -> ToneRole.Warning
        Navree.Tone.DANGER -> ToneRole.Error
        Navree.Tone.INFO -> ToneRole.Info
    }

    /** A gap of the tree in dp: a step of the spacing scale is 4 dp, Material's grid. */
    fun gap(steps: Int): Int = steps.coerceIn(0, 16) * 4

    /** The margin at the sides of a screen's content: 16 dp on a compact window, 24 dp on wider ones. */
    fun marginDp(window: WindowInfo): Int = if (window.width == WidthClass.Compact) 16 else 24

    /** The most a single column of content spans, in dp, so lines stay readable in a wide window. */
    const val CONTENT_MOST_DP = 840

    /** The most a table drawn as a table spans: wider than a column of text, because its columns need the room. */
    const val TABLE_MOST_DP = 1440

    /**
     * The most a screen's one column spans when cards stand side by side in
     * it: wide enough for three, no wider, so a very wide window keeps the
     * content in one place and not stretched.
     */
    const val CARDS_MOST_DP = 1240

    /**
     * The widest the one column of a screen is. Everything in the screen
     * (words, grids, cards, rows) stands in this column, so they share their
     * start edge and their end edge; it is as wide as cards need only where
     * some cards stand side by side.
     */
    fun columnMostDp(cardsSideBySide: Boolean): Int = if (cardsSideBySide) CARDS_MOST_DP else CONTENT_MOST_DP

    /** The width of what a column holds, between its margins, in a pane this wide. */
    fun contentDp(paneDp: Float, mostDp: Int, marginDp: Int): Float = (minOf(paneDp, mostDp.toFloat()) - 2 * marginDp).coerceAtLeast(0f)

    /** The space between cards in a line, in dp. */
    const val CARD_GAP_DP = 16

    /** The width of each of `columns` cards in a line of content this wide. */
    fun cardWidthDp(contentDp: Float, columns: Int, gapDp: Int = CARD_GAP_DP): Float = (contentDp - gapDp * (columns - 1)) / columns

    /**
     * How many columns a line of `count` cards takes in a room of `columns`:
     * a line with fewer cards than columns (the last of a run, or a card
     * alone) stretches its cards across the whole line, so no cell stands
     * empty beside them.
     */
    fun lineColumns(count: Int, columns: Int): Int = count.coerceIn(1, columns.coerceAtLeast(1))

    /**
     * How many cards stand side by side in a pane this wide: one on a phone
     * and a medium window, two on an expanded one, three where it is very
     * wide. Larger text keeps fewer, so a card is never too narrow to read.
     */
    fun cardColumns(paneDp: Int, fontScale: Float = 1f): Int {
        val least = 400f * fontScale.coerceAtLeast(1f)
        if (paneDp < 840) return 1
        return (paneDp / least).toInt().coerceIn(1, 3)
    }

    /** Label and value sit side by side until the text is large; then the value goes under its label. */
    fun factsStacked(window: WindowInfo): Boolean = window.largeText

    /**
     * Facts a few and short enough to stand as small figures in columns: two
     * to eight, each a label over one short word or number, none with an
     * icon or lines of its own. Others keep the list of the system's own.
     */
    fun factsAsFigures(facts: Navree.Facts): Boolean =
        facts.items.size in 2..8 && facts.items.all { item ->
            item.icon == null && item.details.isEmpty() && (item.value.singleOrNull() as? Navree.Text)?.value?.let { it.length <= 18 } == true
        }

    /** How many columns facts as figures stand in, in a room this wide: at least 148 dp each, up to four, one at the largest text. */
    fun factColumns(roomDp: Float, count: Int, fontScale: Float): Int {
        val least = 148f * fontScale.coerceAtLeast(1f)
        return ((roomDp + FACT_GAP_DP) / (least + FACT_GAP_DP)).toInt().coerceIn(1, 4).coerceAtMost(count.coerceAtLeast(1))
    }

    const val FACT_GAP_DP = 16

    /** The widest a small figure of facts is, in dp: in a wide room the figures stand together at the start, not far apart. */
    const val FACT_MOST_DP = 240

    /** The width of each of `columns` small figures in a room this wide. */
    fun factCellDp(roomDp: Float, columns: Int): Float =
        minOf((roomDp - FACT_GAP_DP * (columns - 1)) / columns, FACT_MOST_DP.toFloat())

    /**
     * How a status is drawn. In a row it is a word in its tone's colour after
     * a small dot, as plain as the row's other lines; a status that stands
     * alone in the content keeps a tonal container, at the size of an assist
     * chip's label.
     */
    fun statusAsText(inRow: Boolean): Boolean = inRow
}
