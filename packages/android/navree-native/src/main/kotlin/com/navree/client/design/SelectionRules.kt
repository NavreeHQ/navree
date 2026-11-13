package com.navree.client.design

import com.navree.protocol.Navree

/**
 * Rows the person chooses to act on together, as Material's list selection
 * has it: a box at each row that can be chosen, the top app bar turned into a
 * contextual one while any row is chosen (how many, a way to clear them, and
 * the actions for them), and the offer to choose every row, also those not
 * loaded, once every row held is chosen.
 */
object SelectionRules {
    /** A row has a working box when its table can be chosen from, it has an id, and the server did not leave it out. */
    fun choosable(table: Navree.Table, row: Navree.TableRow): Boolean =
        table.select != null && table.id != null && row.id != null && !row.unchoosable

    /** The ids of the rows chosen. While every row is chosen across the pages, each row that can be chosen is. */
    fun chosen(table: Navree.Table, across: Boolean): List<String> =
        table.rows.filter { choosable(table, it) && (across || it.selected) }.mapNotNull { it.id }

    /** Whether a row's box is ticked. */
    fun ticked(table: Navree.Table, row: Navree.TableRow, across: Boolean): Boolean = choosable(table, row) && (across || row.selected)

    /** "Choose all" is offered when the table allows it, every row held is chosen, and it is not on already. */
    fun offerAcross(table: Navree.Table, across: Boolean): Boolean {
        if (table.select?.across == null || across) return false
        val rows = table.rows.filter { choosable(table, it) }
        return rows.isNotEmpty() && rows.all { it.selected }
    }

    /** The first table of a body whose rows can be chosen. */
    fun selectable(body: Navree.Node?): Navree.Table? =
        body?.walk()?.filterIsInstance<Navree.Table>()?.firstOrNull { it.select != null }

    /**
     * The form the chosen rows are sent with, when it holds nothing but its
     * buttons: those are the contextual bar's actions, so the form is drawn
     * nowhere else. A form with fields of its own stays where it is.
     */
    fun bulkForm(body: Navree.Node?, table: Navree.Table): Navree.Form? {
        val id = table.select?.form ?: return null
        val form = body?.walk()?.filterIsInstance<Navree.Form>()?.firstOrNull { it.id == id } ?: return null
        return form.takeIf { it.children.isNotEmpty() && it.children.all { child -> child is Navree.Button } }
    }

    /** Whether a tap on a row ticks it instead of opening it: while any row is chosen, as in Material's selection mode. */
    fun tapTicks(table: Navree.Table, row: Navree.TableRow, anyChosen: Boolean): Boolean = anyChosen && choosable(table, row)
}
