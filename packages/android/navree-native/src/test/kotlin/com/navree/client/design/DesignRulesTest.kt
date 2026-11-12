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
    fun a_presented_route_is_full_screen_on_a_phone_and_a_dialog_on_a_tablet() {
        assertEquals(Presentation.FullScreenDialog, ScreenRules.presentation(false, WindowInfo.Phone))
        assertEquals(Presentation.Dialog, ScreenRules.presentation(false, WindowInfo.Tablet))
        assertEquals(Presentation.FullScreenDialog, ScreenRules.presentation(true, WindowInfo.Tablet))
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

    @Test
    fun a_bottom_sheet_rests_half_open_only_where_there_is_room() {
        assertTrue(OverlayRules.sheet(Navree.SheetSize.MEDIUM, WindowInfo.Phone).partial)
        assertFalse(OverlayRules.sheet(Navree.SheetSize.MEDIUM, WindowInfo.PhoneLandscape).partial)
        assertTrue(OverlayRules.sheet(Navree.SheetSize.FULL, WindowInfo.Phone).fullHeight)
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
