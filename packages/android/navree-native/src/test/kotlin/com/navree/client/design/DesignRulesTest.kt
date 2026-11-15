package com.navree.client.design

import com.navree.client.Fixtures
import com.navree.protocol.Navree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private fun button(
    label: String,
    role: Navree.Role = Navree.Role.NONE,
    icon: Boolean = true,
    standard: Navree.Standard? = null,
    undoable: Boolean = false,
    action: Navree.Action? = Navree.Action.Navigate("/orders/1"),
) = Navree.Button(action = action, icon = if (icon) Navree.Icon(name = "star") else null, label = label, role = role, standard = standard, undoable = undoable)

private fun screen(
    vararg actions: Navree.Node,
    body: Navree.Node = Navree.Stack(),
    search: Navree.Search? = null,
    refresh: Boolean = false,
    subtitle: String? = null,
) = Navree.Screen(actions = actions.toList(), body = body, title = "Orders", search = search, refresh = refresh, subtitle = subtitle)

private fun label(screen: Navree.Screen, index: Int): String = (screen.actions[index] as Navree.Button).label

private val post = Navree.Action.Request(href = "/orders/1/cancel", method = Navree.Method.POST)

class WindowTest {
    @Test
    fun window_classes_follow_androids_breakpoints() {
        assertEquals(WidthClass.Compact, WidthClass.of(599))
        assertEquals(WidthClass.Medium, WidthClass.of(600))
        assertEquals(WidthClass.Medium, WidthClass.of(839))
        assertEquals(WidthClass.Expanded, WidthClass.of(840))
        assertEquals(WidthClass.Large, WidthClass.of(1200))
        assertEquals(WidthClass.ExtraLarge, WidthClass.of(1600))
        assertEquals(HeightClass.Compact, HeightClass.of(479))
        assertEquals(HeightClass.Medium, HeightClass.of(480))
        assertEquals(HeightClass.Expanded, HeightClass.of(900))
    }
}

class NavigationRulesTest {
    @Test
    fun a_phone_gets_a_navigation_bar_and_its_side_keeps_it() {
        assertEquals(NavigationKind.Bar, NavigationRules.plan(WindowInfo.Phone, listOf(5)).kind)
        assertEquals(NavigationKind.Bar, NavigationRules.plan(WindowInfo.PhoneLandscape, listOf(5)).kind)
    }

    @Test
    fun a_foldable_and_a_small_tablet_get_a_rail() {
        assertEquals(NavigationKind.Rail, NavigationRules.plan(WindowInfo.Foldable, listOf(5)).kind)
        assertEquals(NavigationKind.Rail, NavigationRules.plan(WindowInfo(900, 1200), listOf(5)).kind)
    }

    @Test
    fun a_large_window_gets_the_rail_expanded_not_a_drawer() {
        val plan = NavigationRules.plan(WindowInfo.Tablet, listOf(4, 3))
        assertEquals(NavigationKind.ExpandedRail, plan.kind)
        assertEquals(7, plan.shown.size)
        assertTrue("with more than one group, the groups are named", plan.groupHeadings)
    }

    @Test
    fun a_bar_holds_five_and_a_rail_seven_and_the_rest_are_behind_the_menu() {
        val phone = NavigationRules.plan(WindowInfo.Phone, listOf(4, 4))
        assertEquals(listOf(0, 1, 2, 3, 4), phone.shown)
        assertEquals(listOf(5, 6, 7), phone.more)
        assertTrue(phone.menuButton)
        val foldable = NavigationRules.plan(WindowInfo.Foldable, listOf(4, 4))
        assertEquals(listOf(7), foldable.more)
    }

    @Test
    fun one_destination_needs_no_navigation_and_two_still_get_a_bar() {
        assertEquals(NavigationKind.None, NavigationRules.plan(WindowInfo.Phone, listOf(1)).kind)
        assertEquals(NavigationKind.Bar, NavigationRules.plan(WindowInfo.Phone, listOf(2)).kind)
    }

    @Test
    fun at_the_largest_text_a_destinations_label_takes_two_lines_and_is_never_cut() {
        assertEquals(1, NavigationRules.plan(WindowInfo.Phone, listOf(3)).labelLines)
        assertEquals(2, NavigationRules.plan(WindowInfo.Phone.copy(fontScale = 2f), listOf(3)).labelLines)
    }
}

class ScreenRulesTest {
    private val root = ScreenContext(root = true, user = true)
    private val pushed = ScreenContext(canGoBack = true, user = true)

    @Test
    fun creation_is_the_floating_action_button_and_nowhere_else() {
        val orders = screen(button("Export", Navree.Role.SECONDARY), button("New order", Navree.Role.CREATE))
        val plan = ScreenRules.plan(orders, root, WindowInfo.Phone)
        assertEquals("New order", label(orders, plan.fab!!))
        assertEquals(FabPlace.BottomEnd, plan.fabPlace)
        assertTrue("at a first screen it carries its words", plan.fabExtended)
        assertTrue(plan.barActions.none { it.index == plan.fab } && plan.fab !in plan.overflow)
    }

    @Test
    fun the_standard_add_is_creation_too() {
        val orders = screen(button("Add", standard = Navree.Standard.ADD))
        assertEquals(0, ScreenRules.plan(orders, root, WindowInfo.Phone).fab)
    }

    @Test
    fun deeper_the_floating_action_button_is_the_icon_alone_and_at_the_largest_text_too() {
        val orders = screen(button("New order", Navree.Role.CREATE))
        assertFalse(ScreenRules.plan(orders, pushed, WindowInfo.Phone).fabExtended)
        assertFalse(ScreenRules.plan(orders, root, WindowInfo.Phone.copy(fontScale = 2f)).fabExtended)
    }

    @Test
    fun beside_a_rail_the_floating_action_button_moves_to_the_rails_head() {
        val orders = screen(button("New order", Navree.Role.CREATE))
        val rail = ScreenRules.plan(orders, root.copy(navigation = NavigationKind.Rail), WindowInfo.Foldable)
        assertEquals(FabPlace.RailHeader, rail.fabPlace)
        assertFalse(rail.fabExtended)
        assertTrue(ScreenRules.plan(orders, root.copy(navigation = NavigationKind.ExpandedRail), WindowInfo.Tablet).fabExtended)
    }

    @Test
    fun a_destructive_action_is_never_the_floating_button_nor_in_the_bar_and_closes_the_overflow() {
        val order = screen(
            button("Cancel order", Navree.Role.DESTRUCTIVE, action = post),
            button("Edit", Navree.Role.PRIMARY, standard = Navree.Standard.EDIT),
            button("Share", standard = Navree.Standard.SHARE),
            button("Delete", Navree.Role.CREATE, standard = Navree.Standard.DELETE),
        )
        val plan = ScreenRules.plan(order, pushed, WindowInfo.Tablet)
        assertNull("a destructive action, whatever else it says, is not the floating button", plan.fab)
        assertTrue(plan.barActions.none { ScreenRules.destructive(order.actions[it.index]) })
        assertEquals(listOf("Cancel order", "Delete"), plan.overflow.map { label(order, it) })
    }

    @Test
    fun primary_comes_first_in_the_bar_and_a_compact_window_shows_two() {
        val order = screen(button("Print"), button("Share"), button("Edit", Navree.Role.PRIMARY), button("Archive"))
        val phone = ScreenRules.plan(order, pushed, WindowInfo.Phone)
        assertEquals("the second place goes to the overflow", listOf("Edit"), phone.barActions.map { label(order, it.index) })
        assertEquals(listOf("Print", "Share", "Archive"), phone.overflow.map { label(order, it) })
        val wide = ScreenRules.plan(order, pushed, WindowInfo.Tablet)
        assertEquals(listOf("Edit", "Print", "Share", "Archive"), wide.barActions.map { label(order, it.index) })
        assertTrue(wide.overflow.isEmpty())
    }

    @Test
    fun the_largest_text_gives_up_a_place_in_the_bar() {
        val order = screen(button("Edit", Navree.Role.PRIMARY), button("Share"))
        assertEquals(2, ScreenRules.plan(order, pushed, WindowInfo.Phone).barActions.size)
        val large = ScreenRules.plan(order, pushed, WindowInfo.Phone.copy(fontScale = 2f))
        assertEquals(0, large.barActions.size)
        assertEquals(2, large.overflow.size)
    }

    @Test
    fun an_action_with_no_icon_goes_to_the_overflow_but_one_primary_may_be_words() {
        val order = screen(button("Notes", icon = false), button("Ship", Navree.Role.PRIMARY, icon = false))
        val plan = ScreenRules.plan(order, pushed, WindowInfo.Tablet)
        assertEquals(listOf(BarAction(1, BarActionStyle.Text)), plan.barActions)
        assertEquals(listOf(0), plan.overflow)
    }

    @Test
    fun a_presented_screen_has_an_x_no_floating_button_and_no_cancel_in_the_bar() {
        val form = screen(button("New", Navree.Role.CREATE), button("Cancel", Navree.Role.CANCEL))
        val plan = ScreenRules.plan(form, ScreenContext(presentation = Presentation.FullScreenDialog), WindowInfo.Phone)
        assertEquals(NavigationIcon.Close, plan.navigationIcon)
        assertNull(plan.fab)
        assertTrue(plan.barActions.none { label(form, it.index) == "Cancel" })
    }

    @Test
    fun a_floating_toolbar_takes_the_place_of_the_floating_button() {
        val orders = screen(button("New order", Navree.Role.CREATE))
        val plan = ScreenRules.plan(orders, root.copy(toolbar = true), WindowInfo.Phone)
        assertNull(plan.fab)
        assertTrue("creation is an icon in the bar instead", plan.barActions.any { label(orders, it.index) == "New order" } || plan.overflow.isNotEmpty())
    }

    @Test
    fun a_toolbar_never_floats_above_a_navigation_bar() {
        assertFalse(ScreenRules.toolbarFloats(root))
        assertTrue("beside a rail it floats", ScreenRules.toolbarFloats(root.copy(navigation = NavigationKind.Rail)))
        assertTrue("a pushed screen has no navigation bar under it", ScreenRules.toolbarFloats(pushed))
    }

    @Test
    fun a_whole_form_is_finished_from_the_bar_and_inline_without_chrome() {
        val form = Navree.Form(id = "customer", method = Navree.Method.POST, children = listOf(button("Save", Navree.Role.PRIMARY, action = Navree.Action.Submit())))
        val plan = ScreenRules.plan(screen(body = form), pushed, WindowInfo.Phone)
        assertEquals(SubmitPlace.Bar, plan.submit)
        assertEquals(SubmitPlace.Inline, ScreenRules.plan(screen(body = form), pushed.copy(chrome = false), WindowInfo.Phone).submit)
    }

    @Test
    fun search_on_a_first_screen_is_the_bar_and_deeper_an_icon() {
        val search = Navree.Search(name = "q", placeholder = "Search orders")
        assertEquals(BarKind.Search, ScreenRules.plan(screen(search = search), root, WindowInfo.Phone).bar)
        val deeper = ScreenRules.plan(screen(search = search), pushed, WindowInfo.Phone)
        assertEquals(SearchPlace.Action, deeper.search)
        assertEquals(BarKind.Small, deeper.bar)
    }

    @Test
    fun a_pushed_screen_has_a_back_arrow_and_a_first_screen_none() {
        assertEquals(NavigationIcon.Back, ScreenRules.plan(screen(), pushed, WindowInfo.Phone).navigationIcon)
        assertEquals(NavigationIcon.None, ScreenRules.plan(screen(), root, WindowInfo.Phone).navigationIcon)
        assertEquals("the detail pane has the list beside it", NavigationIcon.None, ScreenRules.plan(screen(), ScreenContext(detailPane = true), WindowInfo.Tablet).navigationIcon)
    }

    @Test
    fun a_subtitle_takes_a_line_of_its_own_while_there_is_room() {
        assertEquals(BarKind.TwoLine, ScreenRules.plan(screen(subtitle = "Ludhiana"), pushed, WindowInfo.Phone).bar)
        assertEquals(BarKind.Small, ScreenRules.plan(screen(subtitle = "Ludhiana"), pushed, WindowInfo.PhoneLandscape).bar)
    }

    @Test
    fun a_task_is_full_screen_on_a_phone_and_a_dialog_on_a_tablet() {
        assertEquals(Presentation.FullScreenDialog, ScreenRules.presentation(Navree.Present.TASK, WindowInfo.Phone))
        assertEquals(Presentation.Dialog, ScreenRules.presentation(Navree.Present.TASK, WindowInfo.Tablet))
        assertEquals("full is full everywhere", Presentation.FullScreenDialog, ScreenRules.presentation(Navree.Present.FULL, WindowInfo.Tablet))
    }

    @Test
    fun a_sheet_rises_from_the_bottom_on_a_phone_and_is_a_dialog_on_a_tablet() {
        assertEquals(Presentation.BottomSheet, ScreenRules.presentation(Navree.Present.SHEET, WindowInfo.Phone))
        assertEquals(Presentation.Dialog, ScreenRules.presentation(Navree.Present.SHEET, WindowInfo.Tablet))
    }

    @Test
    fun a_glance_stands_beside_the_content_on_a_wide_window() {
        assertEquals(Presentation.BottomSheet, ScreenRules.sheet(task = false, window = WindowInfo.Phone))
        assertEquals(Presentation.SideSheet, ScreenRules.sheet(task = false, window = WindowInfo.Tablet))
        assertEquals(Presentation.Dialog, ScreenRules.sheet(task = true, window = WindowInfo.Tablet))
    }

    @Test
    fun two_panes_need_a_wide_window_and_a_list_first() {
        val list = screen(body = Navree.List(children = listOf(Navree.Row(title = "CS-220480"))))
        assertTrue(ScreenRules.twoPanes(WindowInfo.Tablet, list, 2))
        assertFalse(ScreenRules.twoPanes(WindowInfo.Phone, list, 2))
        assertFalse(ScreenRules.twoPanes(WindowInfo.Tablet, list, 1))
        assertFalse(ScreenRules.twoPanes(WindowInfo.Tablet, screen(body = Navree.Text(value = "Hello")), 2))
        val pane = screen(body = Navree.Pane(role = Navree.PaneRole.LIST))
        assertTrue("a pane that says it is a list is one", ScreenRules.twoPanes(WindowInfo.Tablet, pane, 2))
    }

    @Test
    fun the_real_orders_table_gives_its_search_box_and_its_filters_but_not_rows_a_page() {
        val orders = Fixtures.screen("store-demo/orders.json")
        val query = ScreenRules.query(orders)
        assertNotNull(query)
        query!!
        assertNotNull(query.search)
        assertTrue(query.filters.isNotEmpty())
        assertTrue(query.filters.none { it.name.contains("per_page") })
    }
}

class ConfirmRulesTest {
    @Test
    fun a_destructive_action_that_writes_asks_first() {
        assertTrue(ConfirmRules.asks(button("Cancel order", Navree.Role.DESTRUCTIVE, action = post), inDialog = false))
    }

    @Test
    fun one_that_can_be_undone_does_not_ask_its_undo_is_in_the_snackbar() {
        assertFalse(ConfirmRules.asks(button("Archive", Navree.Role.DESTRUCTIVE, undoable = true, action = post), inDialog = false))
    }

    @Test
    fun the_answer_of_a_dialog_is_not_asked_again_and_a_link_never_asks() {
        assertFalse(ConfirmRules.asks(button("Cancel order", Navree.Role.DESTRUCTIVE, action = post), inDialog = true))
        assertFalse(ConfirmRules.asks(button("Cancel order", Navree.Role.DESTRUCTIVE), inDialog = false))
        assertFalse(ConfirmRules.asks(button("Refund", Navree.Role.PRIMARY, action = post), inDialog = false))
    }
}

class ListRulesTest {
    private val columns = listOf(
        Navree.Column(label = "Photo", role = Navree.ColumnRole.LEADING),
        Navree.Column(label = "Order", role = Navree.ColumnRole.TITLE),
        Navree.Column(label = "Customer", role = Navree.ColumnRole.SUBTITLE),
        Navree.Column(label = "Status", role = Navree.ColumnRole.STATUS),
        Navree.Column(label = "Ships to", secondary = true),
        Navree.Column(label = "Total", numeric = true, role = Navree.ColumnRole.TRAILING),
        Navree.Column(label = "", role = Navree.ColumnRole.ACTIONS),
    )
    private val row = Navree.TableRow(
        action = Navree.Action.Navigate("/orders/1"),
        cells = listOf(
            Navree.Avatar(name = "Tejinder Randhawa"),
            Navree.Text(value = "CS-220480"),
            Navree.Text(value = "Tejinder Randhawa"),
            Navree.Badge(text = "Open", tone = Navree.Tone.INFO),
            Navree.Text(value = "Punjab"),
            Navree.Text(value = "₹1,24,500"),
            Navree.Menu(label = "Actions", items = listOf(Navree.Button(label = "Resend confirmation"))),
        ),
    )

    @Test
    fun a_row_is_folded_by_the_role_of_each_column() {
        val folded = ListRules.asListRow(columns, row)
        assertTrue(folded.leading is Navree.Avatar)
        assertEquals("CS-220480", folded.headline)
        assertEquals(listOf("Tejinder Randhawa"), folded.lines.map { (it as Navree.Text).value })
        assertEquals(listOf("Open"), folded.statuses.map { (it as Navree.Badge).text })
        assertEquals("₹1,24,500", (folded.trailing as Navree.Text).value)
        assertEquals(listOf("Resend confirmation"), folded.menu.map { (it as Navree.Button).label })
        assertEquals("the row opens what the server says it opens", Navree.Action.Navigate("/orders/1"), folded.action)
    }

    @Test
    fun a_column_a_narrow_screen_may_leave_out_is_left_out() {
        val folded = ListRules.asListRow(columns, row)
        assertTrue(folded.lines.none { (it as? Navree.Text)?.value == "Punjab" })
    }

    @Test
    fun without_roles_a_badge_is_a_status_and_the_first_number_trails() {
        val plain = listOf(Navree.Column(label = "Order"), Navree.Column(label = "Status"), Navree.Column(label = "Total", numeric = true))
        val folded = ListRules.asListRow(plain, Navree.TableRow(cells = listOf(Navree.Text(value = "CS-1"), Navree.Badge(text = "Paid"), Navree.Text(value = "₹500"))))
        assertEquals("CS-1", folded.headline)
        assertEquals(1, folded.statuses.size)
        assertEquals("₹500", (folded.trailing as Navree.Text).value)
    }

    @Test
    fun a_row_with_no_action_takes_the_one_control_its_title_holds() {
        val cell = Navree.Form(id = "peek-1", action = "/orders/1/peek", children = listOf(Navree.Button(label = "CS-220480", action = Navree.Action.Submit())))
        val folded = ListRules.asListRow(listOf(Navree.Column(label = "Order", role = Navree.ColumnRole.TITLE)), Navree.TableRow(cells = listOf(cell)))
        assertEquals("CS-220480", folded.headline)
        assertEquals(Navree.Action.Submit(form = "peek-1"), folded.action)
    }

    @Test
    fun a_row_that_opens_by_itself_is_still_named_by_the_control_in_its_title() {
        val cell = Navree.Stack(children = listOf(
            Navree.Form(id = "peek-1", children = listOf(Navree.Button(label = "CS-220480", action = Navree.Action.Submit()))),
            Navree.Text(value = "Tejinder Randhawa"),
        ))
        val folded = ListRules.asListRow(listOf(Navree.Column(label = "Order", role = Navree.ColumnRole.TITLE)), Navree.TableRow(action = Navree.Action.Navigate("/orders/1"), cells = listOf(cell)))
        assertEquals("CS-220480", folded.headline)
        assertEquals(listOf("Tejinder Randhawa"), folded.lines.map { (it as Navree.Text).value })
        assertEquals(Navree.Action.Navigate("/orders/1"), folded.action)
    }

    @Test
    fun the_real_orders_rows_each_have_a_headline_and_an_action() {
        val orders = Fixtures.screen("store-demo/orders.json")
        val table = ScreenRules.mainTable(orders.body)!!
        assertTrue(table.rows.isNotEmpty())
        for (each in table.rows) {
            val folded = ListRules.asListRow(table.columns, each)
            assertNotNull(folded.headline)
            assertNotNull("a row on a phone is one touch target", folded.action)
        }
    }

    @Test
    fun in_a_wide_table_the_name_takes_the_most_room_and_a_subtitle_column_none() {
        assertEquals(0f, ListRules.columnWeight(Navree.Column(label = "Customer", role = Navree.ColumnRole.SUBTITLE)))
        assertTrue(ListRules.columnWeight(Navree.Column(label = "Order", role = Navree.ColumnRole.TITLE)) > ListRules.columnWeight(Navree.Column(label = "Total", numeric = true)))
    }

    @Test
    fun a_line_already_said_is_not_said_again() {
        val cols = listOf(Navree.Column(label = "Order", role = Navree.ColumnRole.TITLE), Navree.Column(label = "Customer", role = Navree.ColumnRole.SUBTITLE))
        val cell = Navree.Stack(children = listOf(Navree.Link(label = "CS-1", action = Navree.Action.Navigate("/orders/1")), Navree.Text(value = "Meera Gupta")))
        val folded = ListRules.asListRow(cols, Navree.TableRow(cells = listOf(cell, Navree.Text(value = "Meera Gupta"))))
        assertEquals(listOf("Meera Gupta"), folded.lines.map { (it as Navree.Text).value })
    }

    @Test
    fun at_the_largest_text_the_value_moves_under_the_text() {
        assertFalse(ListRules.trailingBelow(WindowInfo.Phone))
        assertTrue(ListRules.trailingBelow(WindowInfo.Phone.copy(fontScale = 2f)))
    }

    @Test
    fun a_table_is_a_list_on_a_phone_and_a_table_where_its_columns_fit() {
        assertTrue(ListRules.tableAsList(412, 4))
        assertFalse(ListRules.tableAsList(900, 4))
        assertTrue(ListRules.tableAsList(700, 7))
    }

    @Test
    fun a_grid_adds_columns_with_the_width() {
        assertEquals(1, ListRules.gridColumns(380, 200, 16))
        assertEquals(4, ListRules.gridColumns(900, 200, 16))
        assertEquals(3, ListRules.gridColumns(900, 200, 16, most = 3))
        assertEquals("at the largest text two columns become one", 1, ListRules.gridColumns(412, 160, 16, fontScale = 2f))
    }
}

class FormRulesTest {
    @Test
    fun a_few_choices_are_a_connected_button_group_and_many_a_menu_or_a_dialog() {
        assertEquals(ChoiceWidget.ButtonGroup, FormRules.choice(Navree.ChoiceKind.SEGMENTED, 4, WindowInfo.Phone))
        assertEquals(ChoiceWidget.Menu, FormRules.choice(Navree.ChoiceKind.SEGMENTED, 5, WindowInfo.Phone))
        assertEquals(ChoiceWidget.ButtonGroup, FormRules.choice(Navree.ChoiceKind.SEGMENTED, 5, WindowInfo.Tablet))
        assertEquals(ChoiceWidget.Menu, FormRules.choice(Navree.ChoiceKind.SEGMENTED, 3, WindowInfo.Phone.copy(fontScale = 2f)))
        assertEquals(ChoiceWidget.Dialog, FormRules.choice(Navree.ChoiceKind.MENU, 16, WindowInfo.Phone))
        assertEquals(ChoicesWidget.Chips, FormRules.choices(8))
        assertEquals(ChoicesWidget.Dialog, FormRules.choices(9))
    }

    @Test
    fun the_keyboard_goes_on_until_the_last_field_which_finishes() {
        assertEquals("next", FormRules.imeAction(Navree.TextKind.PLAIN, last = false))
        assertEquals("done", FormRules.imeAction(Navree.TextKind.EMAIL, last = true))
        assertEquals("search", FormRules.imeAction(Navree.TextKind.SEARCH, last = false))
    }

    @Test
    fun the_keyboard_learns_no_code_password_or_card_number() {
        assertEquals("secret_number", FormRules.keyboard(Navree.TextKind.ONE_TIME_CODE).type)
        assertEquals("secret_number", FormRules.keyboard(Navree.TextKind.NUMBER, "cc-number").type)
        assertEquals("secret_number", FormRules.keyboard(Navree.TextKind.PLAIN, "cc-csc").type)
        assertEquals("secret_number", FormRules.keyboard(Navree.TextKind.PLAIN, " One-Time-Code ").type)
        assertEquals("password", FormRules.keyboard(Navree.TextKind.PASSWORD, "one-time-code").type)
        assertEquals("number", FormRules.keyboard(Navree.TextKind.NUMBER, "postal-code").type)
        assertEquals(false, FormRules.keyboard(Navree.TextKind.PLAIN, "cc-number").autoCorrect)
    }

    @Test
    fun names_are_capitalised_by_words_and_emails_not_at_all() {
        val plain = FormRules.keyboard(Navree.TextKind.PLAIN)
        assertEquals("words", FormRules.capitalise("name", plain))
        assertEquals("none", FormRules.capitalise("email", plain))
        assertEquals("sentences", FormRules.capitalise(null, plain))
    }

    @Test
    fun a_short_window_types_dates_instead_of_the_calendar() {
        assertEquals(PickerMode.Picker, FormRules.picker(WindowInfo.Phone))
        assertEquals(PickerMode.Input, FormRules.picker(WindowInfo.PhoneLandscape))
    }

    @Test
    fun rows_a_page_is_not_a_filter() {
        val perPage = Navree.Field(name = "per_page", input = Navree.ChoiceInput(options = listOf(Navree.ChoiceOption(label = "25", value = "25"))))
        val status = Navree.Field(name = "filter[status][in]", input = Navree.ChoicesInput())
        assertFalse(FilterRules.isFilter(perPage))
        assertTrue(FilterRules.isFilter(status))
    }

    private fun choice(name: String) = Navree.Field(
        id = name,
        name = name,
        label = name,
        input = Navree.ChoiceInput(options = listOf(Navree.ChoiceOption(label = "One", value = "1"))),
    )

    @Test
    fun a_form_of_choices_that_asks_with_a_read_is_a_row_of_filters() {
        val clear = Navree.Link(label = "Clear filters", action = Navree.Action.Navigate("/"))
        val form = Navree.Form(
            id = "filters",
            children = listOf(
                Navree.Grid(
                    children = listOf(
                        choice("assignee"),
                        choice("label"),
                        Navree.Stack(axis = Navree.Axis.HORIZONTAL, children = listOf(clear)),
                    ),
                ),
            ),
        )
        val bar = FilterRules.bar(form)!!
        assertEquals(listOf("assignee", "label"), bar.fields.map { it.name })
        assertEquals(listOf<Navree.Node>(clear), bar.actions)
    }

    @Test
    fun a_form_that_posts_or_holds_other_content_stays_a_form() {
        val choices = listOf<Navree.Node>(choice("assignee"))
        assertNull("a post is a form to fill in", FilterRules.bar(Navree.Form(id = "f", method = Navree.Method.POST, children = choices)))
        assertNull("no id, nothing to tell the core", FilterRules.bar(Navree.Form(children = choices)))
        assertNull("two boxes of words are a form", FilterRules.bar(Navree.Form(id = "f", children = choices + Navree.Field(name = "q", input = Navree.TextInput()) + Navree.Field(name = "r", input = Navree.TextInput()))))
        assertNull("a password is not a search", FilterRules.bar(Navree.Form(id = "f", children = choices + Navree.Field(name = "q", input = Navree.TextInput(kind = Navree.TextKind.PASSWORD)))))
        assertNull("words among them", FilterRules.bar(Navree.Form(id = "f", children = choices + Navree.Text(value = "Pick"))))
        val switches = Navree.Form(id = "f", children = listOf(Navree.Field(name = "on", input = Navree.ToggleInput())))
        assertNull("only switches are settings", FilterRules.bar(switches))
    }

    @Test
    fun one_box_of_words_goes_over_the_chips_and_the_button_that_sends_goes() {
        val words = Navree.Field(id = "q", name = "q", label = "Search conversations", input = Navree.TextInput())
        val form = Navree.Form(
            id = "f",
            children = listOf(words, choice("channel"), choice("priority"), Navree.Button(label = "Filter", action = Navree.Action.Submit())),
        )
        val bar = FilterRules.bar(form)!!
        assertEquals(words, bar.search)
        assertEquals(listOf("channel", "priority"), bar.fields.map { it.name })
        assertEquals(emptyList<Navree.Node>(), bar.actions)
    }

    @Test
    fun a_button_that_sends_the_form_is_not_one_of_its_filters_actions() {
        val send = Navree.Button(label = "Apply", action = Navree.Action.Submit())
        val form = Navree.Form(id = "f", children = listOf(choice("sort"), send))
        assertEquals(emptyList<Navree.Node>(), FilterRules.bar(form)!!.actions)
    }

    @Test
    fun a_bottom_sheet_rests_half_open_only_where_there_is_room() {
        assertTrue(OverlayRules.sheet(Navree.SheetSize.MEDIUM, WindowInfo.Phone).partial)
        assertFalse(OverlayRules.sheet(Navree.SheetSize.MEDIUM, WindowInfo.PhoneLandscape).partial)
        assertTrue(OverlayRules.sheet(Navree.SheetSize.FULL, WindowInfo.Phone).fullHeight)
    }
}

class SplitPanesTest {
    private fun pane(role: Navree.PaneRole, current: Boolean = false, vararg inside: Navree.Node) = Navree.Pane(role = role, current = current, children = inside.toList())

    @Test
    fun a_conversation_is_the_pane_in_hand_even_when_it_is_deep_in_the_detail() {
        val thread = pane(Navree.PaneRole.DETAIL, current = true)
        val customer = pane(Navree.PaneRole.INSPECTOR)
        val list = pane(Navree.PaneRole.LIST)
        val inner = Navree.Split(primary = thread, detail = customer)
        val outer = Navree.Split(primary = list, detail = inner)
        assertEquals(listOf<Navree.Node>(inner), ScreenRules.splitPanes(outer))
        assertEquals("the thread, the customer under it", listOf<Navree.Node>(thread, customer), ScreenRules.splitPanes(inner))
    }

    @Test
    fun a_list_in_hand_is_drawn_alone_and_a_split_with_none_in_hand_draws_both() {
        val list = pane(Navree.PaneRole.LIST, current = true)
        val empty = pane(Navree.PaneRole.DETAIL)
        assertEquals(listOf<Navree.Node>(list), ScreenRules.splitPanes(Navree.Split(primary = list, detail = empty)))
        val plain = Navree.Split(primary = pane(Navree.PaneRole.LIST), detail = empty)
        assertEquals(listOf(plain.primary, plain.detail), ScreenRules.splitPanes(plain))
    }
}

class BoardRulesTest {
    private fun section(title: String) = Navree.Section(title = title)

    @Test
    fun lanes_keep_a_width_a_card_reads_in_and_other_grids_keep_their_own() {
        val lanes = listOf(section("Backlog"), section("Ready"), section("Review"))
        assertEquals(240, ListRules.gridLeast(null, lanes))
        assertNull("a grid of figures", ListRules.gridLeast(null, listOf(Navree.Text(value = "1"), Navree.Text(value = "2"))))
        assertEquals("the page's own least width", 104, ListRules.gridLeast(104, lanes))
        assertEquals("a phone shows one lane", 1, ListRules.gridColumns(380, 240, 8))
        assertEquals("a tablet shows four", 4, ListRules.gridColumns(1100, 240, 8))
    }

    @Test
    fun lanes_stand_in_one_row_each_as_wide_as_what_fits_them_all_between_a_cards_least_and_a_lanes_most() {
        assertEquals("five lanes of a tablet's pane do not fit at the least: the row scrolls", 240f, ListRules.laneWidthDp(1100f, 5), 0.001f)
        assertEquals("three lanes share what is there", (1100f - 32f) / 3, ListRules.laneWidthDp(1100f, 3), 0.001f)
        assertEquals("two lanes in a very wide room stay together", 360f, ListRules.laneWidthDp(1400f, 2), 0.001f)
        assertEquals("at the largest text a lane is twice as wide", 480f, ListRules.laneWidthDp(1100f, 5, fontScale = 2f), 0.001f)
    }

    @Test
    fun a_grid_of_fields_gives_up_columns_a_grid_of_figures_does_not() {
        assertEquals(2, ListRules.gridColumns(379, null, 8, most = 4, controls = true))
        assertEquals(4, ListRules.gridColumns(379, null, 8, most = 4, controls = false))
    }

    @Test
    fun a_button_that_says_the_rows_title_is_the_rows_own_tap() {
        val button = Navree.Button(action = Navree.Action.Submit(), label = "Offline order sheets")
        val form = Navree.Form(id = "peek", children = listOf(button))
        val badge = Navree.Badge(text = "Feature")
        val row = Navree.Row(title = "Offline order sheets", detail = listOf(Navree.Stack(children = listOf(badge, form))))
        val found = ListRules.titleButton(row)
        assertEquals(button, found?.button)
        val pruned = ListRules.withoutTitleButton(row, found!!.form)
        assertEquals(listOf<Navree.Node>(Navree.Stack(children = listOf(badge))), pruned.detail)
        assertNull("a button that says something else", ListRules.titleButton(row.copy(title = "Another")))
        // The same row in a newer answer holds an equal form that is another object: it is taken out too.
        val again = Navree.Row(
            title = "Offline order sheets",
            detail = listOf(Navree.Stack(children = listOf(badge, Navree.Form(id = "peek", children = listOf(Navree.Button(action = Navree.Action.Submit(), label = "Offline order sheets")))))),
        )
        assertEquals(listOf<Navree.Node>(Navree.Stack(children = listOf(badge))), ListRules.withoutTitleButton(again, found.form).detail)
    }

    @Test
    fun a_field_takes_what_is_left_of_a_line_beside_a_button() {
        val field = Navree.Field(name = "quantity", input = Navree.TextInput())
        val button = Navree.Button(action = Navree.Action.Submit(), label = "Restock")
        assertTrue(CompositionRules.grows(field))
        assertFalse(CompositionRules.grows(button))
        // A line with a field in it is not turned (that would measure it twice before the room is known).
        val line = Navree.Stack(axis = Navree.Axis.HORIZONTAL, children = listOf(field, button))
        assertFalse(CompositionRules.turns(line))
        assertEquals(380f - 8f, CompositionRules.roomLeftDp(380f, line.children, 8f, 1f), 0.001f)
    }
}

class ColumnAlignTest {
    @Test
    fun a_stack_that_turns_starts_its_parts_at_the_start_when_it_is_a_column() {
        val card = Navree.Stack(align = Navree.Align.CENTER, wide = Navree.Axis.HORIZONTAL)
        assertEquals(Navree.Align.START, CompositionRules.columnAlign(card))
        assertFalse(CompositionRules.fillsWidth(card, Navree.Axis.VERTICAL, inLine = false))
        val plain = Navree.Stack(align = Navree.Align.CENTER)
        assertEquals(Navree.Align.CENTER, CompositionRules.columnAlign(plain))
        assertTrue(CompositionRules.fillsWidth(plain, Navree.Axis.VERTICAL, inLine = false))
    }
}

class FactColumnsTest {
    @Test
    fun facts_stand_in_two_columns_only_where_the_room_holds_two() {
        assertEquals(2, ContentRules.factColumns(WindowInfo.Tablet, 0f))
        assertEquals(2, ContentRules.factColumns(WindowInfo.Tablet, 900f))
        assertEquals("a dialog of 512 dp", 1, ContentRules.factColumns(WindowInfo.Tablet, 512f))
        assertEquals(1, ContentRules.factColumns(WindowInfo.Phone, 0f))
        assertEquals(1, ContentRules.factColumns(WindowInfo.Tablet.copy(fontScale = 1.5f), 900f))
    }
}

class ColumnShareTest {
    private fun column(label: String, role: Navree.ColumnRole? = null) = Navree.Column(label = label, role = role)
    private fun row(vararg cells: String) = Navree.TableRow(cells = cells.map { Navree.Text(value = it) })

    @Test
    fun a_column_of_long_places_takes_more_of_the_width_than_a_column_of_short_words() {
        val columns = listOf(column("Order", Navree.ColumnRole.TITLE), column("Payment"), column("Ships to"))
        val rows = listOf(row("CS-1", "Paid", "Mumbai, Maharashtra"), row("CS-2", "Paid", "Delhi"))
        val weights = ListRules.columnWeights(columns, rows)
        assertEquals(1.6f, weights[0], 0.001f)
        assertEquals(1f, weights[1], 0.001f)
        assertEquals(1.9f, weights[2], 0.001f)
    }

    @Test
    fun a_column_with_a_box_to_type_in_takes_room_for_the_box_and_the_button() {
        val columns = listOf(column("Product", Navree.ColumnRole.TITLE), column("Restock"))
        val typing = Navree.TableRow(
            cells = listOf(
                Navree.Text(value = "Brass lota"),
                Navree.Form(id = "r", children = listOf(Navree.Field(name = "units", input = Navree.TextInput()), Navree.Button(label = "Restock"))),
            ),
        )
        assertEquals(ListRules.TYPING_COLUMN_WEIGHT, ListRules.columnWeights(columns, listOf(typing))[1], 0.001f)
        assertEquals("a column of words keeps its share", 1f, ListRules.columnWeights(columns, listOf(row("Brass lota", "Open")))[1], 0.001f)
    }

    @Test
    fun a_table_with_a_box_to_type_in_counts_that_column_twice_for_its_room() {
        val columns = listOf(column("Order", Navree.ColumnRole.TITLE), column("Delivery note"))
        val typing = Navree.TableRow(cells = listOf(Navree.Text(value = "CH-1"), Navree.Field(name = "note", input = Navree.TextInput())))
        assertEquals(1, ListRules.typingColumns(columns, listOf(typing)))
        assertEquals(0, ListRules.typingColumns(columns, listOf(row("CH-1", "Open"))))
        // Six columns of 128 dp need 768 dp: an upright tablet's 740 dp holds them as a list, a wide one as a table.
        assertTrue(ListRules.tableAsList(740, 5 + 1))
        assertFalse(ListRules.tableAsList(1200, 5 + 1))
    }

    @Test
    fun at_the_largest_text_a_table_with_seven_columns_is_a_list_even_in_a_wide_pane() {
        assertFalse(ListRules.tableAsList(1000, 7, 1f))
        assertTrue(ListRules.tableAsList(1000, 7, 2f))
    }
}

class DetailRunsTest {
    @Test
    fun badges_that_follow_each_other_are_one_line_and_other_parts_keep_theirs() {
        val high = Navree.Badge(text = "High")
        val overdue = Navree.Badge(text = "Overdue 9 min")
        val note = Navree.Text(value = "Ananya")
        val runs = ListRules.detailRuns(listOf(high, overdue, note, high))
        assertEquals(listOf(listOf<Navree.Node>(high, overdue), listOf<Navree.Node>(note), listOf<Navree.Node>(high)), runs)
        assertEquals(emptyList<List<Navree.Node>>(), ListRules.detailRuns(emptyList()))
    }
}

class NavigationLabelTest {
    @Test
    fun five_labels_on_a_phone_keep_to_the_size_their_longest_word_fits() {
        val labels = listOf("All open", "Assigned to me", "Unassigned", "Snoozed", "Closed")
        val scale = NavigationRules.labelScale(2.0f, 412, labels)
        assertTrue("no word of five labels fits at twice the size: $scale", scale < 1.3f && scale >= 1f)
    }

    @Test
    fun a_label_that_does_not_fit_one_line_wraps_between_its_words() {
        val labels = listOf("All open", "Assigned to me", "Unassigned", "Snoozed", "Closed")
        assertEquals(2, NavigationRules.labelLines(412, labels, 1))
        assertEquals(1, NavigationRules.labelLines(412, listOf("Dashboard", "Orders", "Inventory"), 1))
    }

    @Test
    fun a_collapsed_rails_labels_keep_to_the_size_their_longest_word_fits_in_80_dp() {
        val labels = listOf("Dashboard", "Orders", "Inventory")
        val scale = NavigationRules.labelScale(2.0f, NavigationRules.RAIL_ITEM_DP * labels.size, labels)
        assertTrue("no word of these labels fits 80 dp at twice the size: $scale", scale < 1.3f && scale >= 1f)
    }

    @Test
    fun three_short_labels_follow_the_setting_all_the_way() {
        assertEquals(2.0f, NavigationRules.labelScale(2.0f, 412, listOf("Dashboard", "Orders", "Inventory")), 0.001f)
    }

    @Test
    fun text_smaller_than_the_default_is_kept() {
        assertEquals(0.85f, NavigationRules.labelScale(0.85f, 412, listOf("A", "B")), 0.001f)
    }
}

class TaskActionsTest {
    private fun action(label: String, role: Navree.Role) = Navree.Button(action = Navree.Action.Navigate("/x"), label = label, role = role)

    @Test
    fun a_task_with_many_actions_keeps_the_title_its_room_by_putting_them_at_the_foot() {
        val resend = action("Resend confirmation", Navree.Role.SECONDARY)
        val refund = action("Refund", Navree.Role.SECONDARY)
        val cancel = action("Cancel order", Navree.Role.DESTRUCTIVE)
        val open = action("Open as a page", Navree.Role.NAVIGATION)
        val placed = OverlayRules.taskActions(listOf(resend, refund, cancel, open), hasSubmit = false)
        assertNull("no action is the one that finishes the task", placed.bar)
        assertEquals(listOf(resend, refund, open, cancel), placed.foot)
    }

    @Test
    fun the_one_primary_action_finishes_the_task_from_the_bar() {
        val save = action("Save", Navree.Role.PRIMARY)
        val other = action("Discard changes", Navree.Role.SECONDARY)
        val placed = OverlayRules.taskActions(listOf(other, save), hasSubmit = false)
        assertEquals(save, placed.bar)
        assertEquals(listOf(other), placed.foot)
    }

    @Test
    fun a_forms_submit_takes_the_place_in_the_bar_and_a_way_out_is_not_an_action() {
        val save = action("Save", Navree.Role.PRIMARY)
        val cancel = action("Cancel", Navree.Role.CANCEL)
        val placed = OverlayRules.taskActions(listOf(save, cancel), hasSubmit = true)
        assertNull(placed.bar)
        assertEquals(listOf(save), placed.foot)
    }
}

class SymbolsTest {
    @Test
    fun every_standard_action_takes_a_system_symbol_the_client_carries() {
        for (standard in Navree.Standard.entries) {
            val symbol = Symbols.of(standard)
            assertTrue("$standard is $symbol", Symbols.has(symbol))
            assertNotNull(Symbols.paths(symbol, filled = true))
        }
    }

    @Test
    fun the_store_demos_destinations_map_to_symbols() {
        assertEquals("dashboard", Symbols.of("layout-dashboard"))
        assertEquals("shopping_bag", Symbols.of("shopping-bag"))
        assertEquals("inventory_2", Symbols.of("boxes"))
        assertEquals("a symbol's own name is taken as it is", "receipt_long", Symbols.of("receipt_long"))
        assertNull("a name that maps nowhere keeps the page's shape", Symbols.of("hologram"))
    }

    @Test
    fun every_mapped_name_is_a_symbol_the_client_carries_in_both_forms() {
        val names = listOf("layout-dashboard", "shopping-bag", "boxes", "truck", "users", "settings", "inbox", "plus", "pencil", "trash-2", "share-2", "x", "check", "bell")
        for (name in names) {
            val symbol = Symbols.of(name)!!
            assertNotNull("$name -> $symbol plain", Symbols.paths(symbol, filled = false))
            assertNotNull("$name -> $symbol filled", Symbols.paths(symbol, filled = true))
        }
    }
}

class ContentRulesTest {
    @Test
    fun text_styles_take_material_type_roles() {
        assertEquals(TypeRole.HeadlineMedium, ContentRules.type(Navree.TextStyle.LARGE_TITLE))
        assertEquals(TypeRole.BodyLarge, ContentRules.type(Navree.TextStyle.BODY))
        assertEquals(TypeRole.BodySmall, ContentRules.type(Navree.TextStyle.CAPTION))
        assertEquals("the one figure a screen is about", TypeRole.DisplayMedium, ContentRules.type(Navree.TextStyle.DISPLAY))
        assertEquals("a title and the second title are two steps apart", TypeRole.HeadlineSmall, ContentRules.type(Navree.TextStyle.TITLE))
        assertEquals(TypeRole.TitleLarge, ContentRules.type(Navree.TextStyle.TITLE2))
    }

    @Test
    fun tones_take_colour_roles_and_no_tone_is_the_surface() {
        assertEquals(ToneRole.Surface, ContentRules.tone(null))
        assertEquals(ToneRole.Error, ContentRules.tone(Navree.Tone.DANGER))
        assertEquals(ToneRole.Success, ContentRules.tone(Navree.Tone.SUCCESS))
    }

    @Test
    fun margins_and_gaps_are_on_materials_grid() {
        assertEquals(16, ContentRules.marginDp(WindowInfo.Phone))
        assertEquals(24, ContentRules.marginDp(WindowInfo.Tablet))
        assertEquals(8, ContentRules.gap(2))
        assertTrue(ContentRules.factsStacked(WindowInfo.Phone.copy(fontScale = 2f)))
    }

    @Test
    fun a_status_in_a_row_is_a_word_not_a_filled_label() {
        assertTrue(ContentRules.statusAsText(inRow = true))
    }
}

class TooltipRulesTest {
    @Test
    fun a_tooltip_keeps_a_margin_from_both_edges_of_the_window() {
        assertEquals("room on both sides: where it was", 120, TooltipRules.inset(120, 320, 800, 48))
        assertEquals("against the left edge", 48, TooltipRules.inset(0, 320, 800, 48))
        assertEquals("against the right edge", 800 - 320 - 48, TooltipRules.inset(500, 320, 800, 48))
        assertEquals("wider than the room: at the margin", 48, TooltipRules.inset(10, 790, 800, 48))
    }
}

class GridOfStacksTest {
    private val stacks = listOf<Navree.Node>(Navree.Stack(), Navree.Stack())

    @Test
    fun the_stacks_of_a_grid_are_one_column_in_a_narrow_window_and_stay_columns_in_a_wide_one() {
        assertTrue(ListRules.stacksAreOneColumn(stacks, null, 412))
        assertTrue("a medium window puts the side column under the main one", ListRules.stacksAreOneColumn(stacks, null, 700))
        assertFalse(ListRules.stacksAreOneColumn(stacks, null, 900))
        assertFalse("a grid that says its own least width", ListRules.stacksAreOneColumn(stacks, 160, 412))
        assertFalse("cards stay as many across as fit", ListRules.stacksAreOneColumn(listOf(Navree.Card(), Navree.Card()), null, 412))
    }
}

class LanesTest {
    private val lanes = listOf<Navree.Node>(Navree.Section(title = "Backlog"), Navree.Section(title = "Ready"), Navree.Section(title = "Done"))

    @Test
    fun lanes_are_one_a_tab_where_the_window_holds_one_column_of_them() {
        assertTrue(ListRules.oneLaneAtATime(lanes, columns = 1))
        assertFalse("room for two side by side", ListRules.oneLaneAtATime(lanes, columns = 2))
    }

    @Test
    fun a_grid_that_is_not_all_sections_is_not_lanes() {
        assertFalse(ListRules.areLanes(lanes + Navree.Text(value = "Note")))
        assertFalse("one section is no set of lanes", ListRules.areLanes(lanes.take(1)))
    }
}

class RepeatedTitleTest {
    private val heading = Navree.Text(value = "All open", heading = 2, style = Navree.TextStyle.TITLE)

    @Test
    fun a_heading_that_says_the_bars_title_is_not_said_again() {
        assertTrue(ScreenRules.repeatsTitle(heading, "All open", titleInBar = true))
        assertTrue("case and spaces do not matter", ScreenRules.repeatsTitle(heading, " all open ", titleInBar = true))
        assertFalse("another title", ScreenRules.repeatsTitle(heading, "Closed", titleInBar = true))
        assertFalse("words that are no heading", ScreenRules.repeatsTitle(Navree.Text(value = "All open"), "All open", titleInBar = true))
        assertFalse("a screen with no title", ScreenRules.repeatsTitle(heading, null, titleInBar = true))
    }

    @Test
    fun a_heading_stays_where_the_bar_does_not_say_the_title() {
        assertFalse(ScreenRules.repeatsTitle(heading, "All open", titleInBar = false))
    }

    @Test
    fun the_plan_says_whether_the_bar_draws_the_title() {
        val body = Navree.Text(value = "Sign in", heading = 1)
        val plain = Navree.Screen(title = "Sign in", body = body)
        assertTrue("a plain bar", ScreenRules.plan(plain, ScreenContext(root = true), WindowInfo.Phone).titleInBar)
        assertFalse("a screen without the app's chrome has no bar", ScreenRules.plan(plain, ScreenContext(chrome = false), WindowInfo.Phone).titleInBar)
        val searching = Navree.Screen(title = "Orders", body = body, search = Navree.Search(name = "q"))
        assertFalse("the search bar shows the field", ScreenRules.plan(searching, ScreenContext(root = true), WindowInfo.Phone).titleInBar)
        val days = Navree.Screen(title = "Sleep", body = body, pager = Navree.DayPager(date = "2026-11-13", day = "Friday"))
        assertFalse("a bar that holds the day pager shows the day", ScreenRules.plan(days, ScreenContext(root = true), WindowInfo.Phone).titleInBar)
    }
}

class OpenRowTest {
    @Test
    fun the_row_that_leads_to_the_page_shown_is_the_open_one() {
        assertTrue(ListRules.shows("/inbox/11", "/inbox/11"))
        assertTrue("a full address", ListRules.shows("http://10.0.2.2:8083/inbox/11?after=3", "/inbox/11"))
        assertFalse("another page", ListRules.shows("/inbox/211", "/inbox/11"))
        assertFalse("the list's own page", ListRules.shows("/inbox", "/inbox/11"))
        assertFalse("nothing shown", ListRules.shows(null, "/inbox/11"))
    }
}

class OwnLinkTest {
    @Test
    fun a_row_that_is_a_link_does_not_hold_a_second_link_to_the_same_place() {
        val runs = listOf(Navree.Run(text = "Ravneet packed "), Navree.Run(text = "CH-1042", href = "/orders/1042"), Navree.Run(text = " and sent ", href = "/other"))
        val kept = ListRules.withoutOwnLink(runs, Navree.Action.Navigate("/orders/1042"))
        assertNull(kept[1].href)
        assertEquals("a link to another place stays", "/other", kept[2].href)
        assertEquals("a row that goes nowhere keeps its links", runs, ListRules.withoutOwnLink(runs, null))
    }
}

class WideListStatesTest {
    private val columns = listOf(
        Navree.Column(label = "Order", role = Navree.ColumnRole.TITLE),
        Navree.Column(label = "Status", role = Navree.ColumnRole.STATUS),
        Navree.Column(label = "Payment", secondary = true),
        Navree.Column(label = "Ships to", secondary = true),
    )
    private val row = Navree.TableRow(cells = listOf(Navree.Text(value = "CS-1"), Navree.Badge(text = "Open"), Navree.Badge(text = "Paid"), Navree.Text(value = "Ludhiana")))

    @Test
    fun a_list_on_a_phone_leaves_out_the_columns_a_narrow_screen_may_leave_out() {
        val folded = ListRules.asListRow(columns, row)
        assertEquals(listOf<Navree.Node>(Navree.Badge(text = "Open")), folded.statuses)
    }

    @Test
    fun a_wide_list_keeps_every_column_that_says_a_state_and_still_leaves_out_the_other_words() {
        val folded = ListRules.asListRow(columns, row, wide = true)
        assertEquals(listOf<Navree.Node>(Navree.Badge(text = "Open"), Navree.Badge(text = "Paid")), folded.statuses)
        assertFalse(folded.lines.any { (it as? Navree.Text)?.value == "Ludhiana" })
    }
}

class ShortLineTest {
    @Test
    fun a_line_with_a_dial_that_leaves_most_of_the_card_empty_is_put_in_the_middle() {
        assertTrue(CompositionRules.holdsDial(listOf(Navree.Ring(value = 83.0), Navree.Text(value = "words"))))
        assertFalse(CompositionRules.holdsDial(listOf(Navree.Text(value = "words"))))
        assertTrue(CompositionRules.centresShortLine(usedPx = 550, roomPx = 1265))
        assertFalse("a line that fills most of its card stays at the start", CompositionRules.centresShortLine(usedPx = 900, roomPx = 1265))
        assertFalse("no room known", CompositionRules.centresShortLine(usedPx = 100, roomPx = 0))
    }

    @Test
    fun the_cards_of_a_line_take_the_tallest_height_seen_and_never_less() {
        assertEquals(300, CompositionRules.tallest(0, 300))
        assertEquals(300, CompositionRules.tallest(300, 180))
        assertEquals(420, CompositionRules.tallest(300, 420))
    }

    @Test
    fun a_block_at_the_end_of_a_table_column_has_its_lines_end_where_the_widest_ends() {
        val block = Navree.Stack(children = listOf(Navree.Text(value = "0"), Navree.Text(value = "Reorder at 14")))
        assertEquals(Navree.Align.END, CompositionRules.columnAlignIn(block, endAligned = true))
        assertNull(CompositionRules.columnAlignIn(block, endAligned = false))
    }
}

class ActionsInALineTest {
    @Test
    fun a_form_of_only_buttons_is_a_group_of_actions_that_flows_with_the_line() {
        val close = Navree.Form(id = "close", children = listOf(Navree.Button(action = Navree.Action.Submit(), label = "Close")))
        assertTrue(FormRules.onlyButtons(close))
        assertFalse(FormRules.onlyButtons(Navree.Form(children = listOf(Navree.Field(name = "note", input = Navree.TextInput()), Navree.Button(action = Navree.Action.Submit(), label = "Save")))))
        assertFalse("an empty form", FormRules.onlyButtons(Navree.Form()))
    }

    @Test
    fun a_menu_that_says_its_value_shows_it_beside_its_icon_and_a_more_icon_stays_alone() {
        assertTrue(MenuRules.showsLabel("circle-dot", "Open"))
        assertTrue(MenuRules.showsLabel("signal-high", "High"))
        assertFalse(MenuRules.showsLabel("more-vertical", "More"))
        assertFalse(MenuRules.showsLabel("ellipsis", "Actions"))
        assertFalse("no icon: a labelled button already", MenuRules.showsLabel(null, "Sort"))
        assertFalse(MenuRules.showsLabel("circle-dot", " "))
    }
}
