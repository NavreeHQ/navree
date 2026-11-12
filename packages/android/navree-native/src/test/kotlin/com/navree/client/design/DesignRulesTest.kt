package com.navree.client.design

import com.navree.client.Fixtures
import com.navree.client.protocol.ActionRole
import com.navree.client.protocol.ButtonNode
import com.navree.client.protocol.ChoiceKind
import com.navree.client.protocol.Column
import com.navree.client.protocol.DateKind
import com.navree.client.protocol.IconNode
import com.navree.client.protocol.Nav
import com.navree.client.protocol.NavGroup
import com.navree.client.protocol.NavItem
import com.navree.client.protocol.Screen
import com.navree.client.protocol.SheetSize
import com.navree.client.protocol.TableNode
import com.navree.client.protocol.TableRow
import com.navree.client.protocol.TextKind
import com.navree.client.protocol.TextLevel
import com.navree.client.protocol.TextNode
import com.navree.client.protocol.Tone
import com.navree.client.protocol.decodeEnvelope
import com.navree.client.protocol.decodeShell
import com.navree.client.protocol.Submit
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private fun screen(name: String): Screen = decodeEnvelope(Fixtures.envelope(name)).screen!!

private fun label(screen: Screen, index: Int): String = (screen.actions[index] as ButtonNode).label

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
    private val five = decodeShell(Fixtures.text("shell.json")).nav

    private fun nav(vararg groups: Int): Nav =
        Nav(groups.mapIndexed { g, size -> NavGroup("Group $g", List(size) { NavItem("Item $it", href = "/g$g/$it") }) })

    @Test
    fun a_phone_gets_a_navigation_bar() {
        val plan = NavigationRules.plan(WindowInfo.Phone, five)
        assertEquals(NavigationKind.Bar, plan.kind)
        assertEquals(listOf(0, 1, 2, 3, 4), plan.shown)
        assertFalse(plan.menuButton)
    }

    @Test
    fun a_phone_on_its_side_keeps_the_bar_because_the_window_is_short() {
        assertEquals(NavigationKind.Bar, NavigationRules.plan(WindowInfo.PhoneLandscape, five).kind)
    }

    @Test
    fun a_foldable_and_a_small_tablet_get_a_rail() {
        assertEquals(NavigationKind.Rail, NavigationRules.plan(WindowInfo.Foldable, five).kind)
        assertEquals(NavigationKind.Rail, NavigationRules.plan(WindowInfo(900, 1200), five).kind)
    }

    @Test
    fun a_large_window_gets_the_rail_expanded_not_a_drawer() {
        val plan = NavigationRules.plan(WindowInfo.Tablet, nav(4, 3))
        assertEquals(NavigationKind.ExpandedRail, plan.kind)
        assertEquals(7, plan.shown.size)
        assertTrue("with more than one group, the groups are named", plan.groupHeadings)
        assertFalse(NavigationRules.plan(WindowInfo.Desktop, five).groupHeadings)
    }

    @Test
    fun a_bar_holds_five_and_a_rail_seven_and_the_rest_are_behind_the_menu() {
        val phone = NavigationRules.plan(WindowInfo.Phone, nav(4, 4))
        assertEquals(listOf(0, 1, 2, 3, 4), phone.shown)
        assertEquals(listOf(5, 6, 7), phone.more)
        assertTrue(phone.menuButton)

        val foldable = NavigationRules.plan(WindowInfo.Foldable, nav(4, 4))
        assertEquals(7, foldable.shown.size)
        assertEquals(listOf(7), foldable.more)
        assertTrue(foldable.menuButton)
    }

    @Test
    fun one_destination_needs_no_navigation_and_two_still_get_a_bar() {
        assertEquals(NavigationKind.None, NavigationRules.plan(WindowInfo.Phone, nav(1)).kind)
        assertEquals(NavigationKind.None, NavigationRules.plan(WindowInfo.Tablet, nav()).kind)
        assertEquals(NavigationKind.Bar, NavigationRules.plan(WindowInfo.Phone, nav(2)).kind)
    }
}

class ScreenRulesTest {
    private val root = ScreenContext(root = true, user = true)
    private val pushed = ScreenContext(canGoBack = true, user = true)

    @Test
    fun on_a_phone_the_create_action_is_an_extended_floating_action_button() {
        val orders = screen("orders")
        val plan = ScreenRules.plan(orders, root, WindowInfo.Phone)
        assertEquals("New order", label(orders, plan.fab!!))
        assertEquals(FabPlace.BottomEnd, plan.fabPlace)
        assertTrue(plan.fabExtended)
        assertTrue("the create action is nowhere else", plan.barActions.none { it.index == plan.fab } && plan.fab !in plan.overflow)
    }

    @Test
    fun a_first_screen_with_search_has_a_search_bar_for_its_app_bar() {
        val orders = screen("orders")
        val plan = ScreenRules.plan(orders, root, WindowInfo.Phone)
        assertEquals(BarKind.Search, plan.bar)
        assertEquals(SearchPlace.Bar, plan.search)
        assertEquals(NavigationIcon.None, plan.navigationIcon)
        assertTrue(plan.avatar)
        assertTrue("beside the avatar there is room for the overflow only", plan.barActions.isEmpty())
        assertEquals(listOf("Filter", "Export as CSV"), plan.overflow.map { label(orders, it) })
        assertTrue(plan.pullToRefresh)
    }

    @Test
    fun without_a_signed_in_user_the_search_bar_has_room_for_two_actions() {
        val orders = screen("orders")
        val plan = ScreenRules.plan(orders, root.copy(user = false), WindowInfo.Phone)
        assertFalse(plan.avatar)
        assertEquals(listOf("Filter", "Export as CSV"), plan.barActions.map { label(orders, it.index) })
        assertTrue(plan.overflow.isEmpty())
    }

    @Test
    fun beside_a_rail_the_floating_action_button_moves_to_the_rails_head() {
        val orders = screen("orders")
        val foldable = ScreenRules.plan(orders, root.copy(navigation = NavigationKind.Rail), WindowInfo.Foldable)
        assertEquals(FabPlace.RailHeader, foldable.fabPlace)
        assertFalse("a collapsed rail has room for the icon only", foldable.fabExtended)
        val tablet = ScreenRules.plan(orders, root.copy(navigation = NavigationKind.ExpandedRail), WindowInfo.Tablet)
        assertEquals(FabPlace.RailHeader, tablet.fabPlace)
        assertTrue(tablet.fabExtended)
    }

    @Test
    fun a_deeper_screen_keeps_its_floating_action_button_small_and_in_the_corner() {
        val products = screen("products")
        val plan = ScreenRules.plan(products, pushed.copy(navigation = NavigationKind.Rail), WindowInfo.Foldable)
        assertEquals(FabPlace.BottomEnd, plan.fabPlace)
        assertFalse(plan.fabExtended)
        assertEquals("search on a deeper screen is an icon in the bar", SearchPlace.Action, plan.search)
        assertEquals(BarKind.Small, plan.bar)
        assertEquals(NavigationIcon.Back, plan.navigationIcon)
        assertFalse("the avatar belongs to first screens", plan.avatar)
    }

    @Test
    fun primary_goes_in_the_bar_the_rest_in_the_overflow_and_destructive_last() {
        val order = screen("order")
        val plan = ScreenRules.plan(order, pushed, WindowInfo.Phone)
        assertNull("nothing here creates", plan.fab)
        assertEquals(listOf("Edit"), plan.barActions.map { label(order, it.index) })
        assertEquals(BarActionStyle.Icon, plan.barActions.single().style)
        assertEquals(listOf("Share", "Print invoice", "Mark shipped", "Cancel order"), plan.overflow.map { label(order, it) })
        assertEquals(ActionRole.Destructive, (order.actions[plan.overflow.last()] as ButtonNode).role)
        assertEquals(NavigationIcon.Back, plan.navigationIcon)
    }

    @Test
    fun a_wider_window_shows_more_actions_and_the_largest_text_fewer() {
        val order = screen("order")
        val tablet = ScreenRules.plan(order, pushed, WindowInfo.Tablet)
        assertEquals(listOf("Edit", "Share"), tablet.barActions.map { label(order, it.index) })
        assertEquals(listOf("Print invoice", "Mark shipped", "Cancel order"), tablet.overflow.map { label(order, it) })

        val largeText = ScreenRules.plan(order, pushed, WindowInfo.Phone.copy(fontScale = 2f))
        assertTrue("at the largest text the bar keeps only the overflow", largeText.barActions.isEmpty())
        assertEquals(5, largeText.overflow.size)
    }

    @Test
    fun a_destructive_action_is_never_in_the_bar_even_when_there_is_room() {
        val only = Screen(
            title = "Customer",
            actions = listOf(ButtonNode(label = "Delete", role = ActionRole.Destructive, icon = IconNode(paths = listOf("M3 6h18")))),
        )
        val plan = ScreenRules.plan(only, pushed, WindowInfo.Desktop)
        assertTrue(plan.barActions.isEmpty())
        assertEquals(listOf(0), plan.overflow)
    }

    @Test
    fun one_primary_action_without_an_icon_may_be_a_text_button() {
        val screen = Screen(
            title = "Invoice",
            actions = listOf(ButtonNode(label = "Send", role = ActionRole.Primary), ButtonNode(label = "Download"), ButtonNode(label = "Duplicate")),
        )
        val plan = ScreenRules.plan(screen, pushed, WindowInfo.Phone)
        assertEquals(listOf(BarAction(0, BarActionStyle.Text)), plan.barActions)
        assertEquals(listOf(1, 2), plan.overflow)
    }

    @Test
    fun a_subtitle_sits_under_the_title_unless_the_window_is_short_or_the_text_at_its_largest() {
        val order = screen("order")
        val upright = ScreenRules.plan(order, pushed, WindowInfo.Phone)
        assertEquals(BarKind.TwoLine, upright.bar)
        assertEquals(BarScroll.Pinned, upright.scroll)
        val onItsSide = ScreenRules.plan(order, pushed, WindowInfo.PhoneLandscape)
        assertEquals(BarKind.Small, onItsSide.bar)
        assertEquals("in a short window the bar leaves as the content scrolls", BarScroll.EnterAlways, onItsSide.scroll)
        assertEquals(BarKind.Small, ScreenRules.plan(order, pushed, WindowInfo.Phone.copy(fontScale = 2f)).bar)
    }

    @Test
    fun the_search_box_of_a_tables_query_goes_to_the_app_bar_and_its_filters_become_chips() {
        val orders = decodeEnvelope(Fixtures.text("store-demo/orders.json")).screen!!
        val query = ScreenRules.query(orders)!!
        assertEquals("search", query.search?.name)
        assertEquals(listOf("Status", "Fulfilment", "Payment", "Ships to"), query.filters.take(4).map { it.label })
        val plan = ScreenRules.plan(orders, root, WindowInfo.Phone)
        assertEquals(BarKind.Search, plan.bar)
        assertTrue(plan.searchInQuery)
        assertFalse("a screen with a search of its own uses that", ScreenRules.plan(screen("orders"), root, WindowInfo.Phone).searchInQuery)
        assertTrue(ScreenRules.isCollection(orders.body))
    }

    @Test
    fun a_presented_form_is_a_full_screen_dialog_closed_with_an_x_and_saved_from_the_bar() {
        assertEquals(Presentation.FullScreenDialog, ScreenRules.presentation(full = false, WindowInfo.Phone))
        val edit = screen("order-edit")
        val plan = ScreenRules.plan(edit, ScreenContext(presentation = Presentation.FullScreenDialog), WindowInfo.Phone)
        assertEquals(NavigationIcon.Close, plan.navigationIcon)
        assertEquals(SubmitPlace.Bar, plan.submit)
        assertEquals(BarKind.Small, plan.bar)
        assertNull(plan.fab)
        assertFalse(plan.avatar)
        assertFalse("a form is not pulled to refresh", plan.pullToRefresh)
    }

    @Test
    fun in_a_wide_window_the_presented_form_is_a_dialog_with_its_button_under_the_fields() {
        assertEquals(Presentation.Dialog, ScreenRules.presentation(full = false, WindowInfo.Tablet))
        assertEquals(Presentation.Dialog, ScreenRules.presentation(full = false, WindowInfo.Foldable))
        assertEquals("a full cover is full at any size", Presentation.FullScreenDialog, ScreenRules.presentation(full = true, WindowInfo.Tablet))
        assertEquals("a short window has no room for a dialog", Presentation.FullScreenDialog, ScreenRules.presentation(full = false, WindowInfo.PhoneLandscape))
        val plan = ScreenRules.plan(screen("order-edit"), ScreenContext(presentation = Presentation.Dialog), WindowInfo.Tablet)
        assertEquals(SubmitPlace.Inline, plan.submit)
        assertEquals(NavigationIcon.Close, plan.navigationIcon)
    }

    @Test
    fun a_pushed_form_is_saved_from_the_bar_and_a_screen_with_no_chrome_keeps_its_button() {
        val edit = screen("order-edit")
        assertEquals(SubmitPlace.Bar, ScreenRules.plan(edit, pushed, WindowInfo.Phone).submit)
        val signIn = ScreenRules.plan(screen("sign-in"), ScreenContext(presentation = Presentation.FullScreenDialog, chrome = false), WindowInfo.Phone)
        assertEquals(BarKind.None, signIn.bar)
        assertEquals("the form is among other content, so its button stays with it", SubmitPlace.None, signIn.submit)
        val bare = ScreenRules.plan(edit, ScreenContext(chrome = false), WindowInfo.Phone)
        assertEquals(SubmitPlace.Inline, bare.submit)
        assertEquals(NavigationIcon.None, bare.navigationIcon)
    }

    @Test
    fun a_screen_pushed_inside_a_presented_one_goes_back_with_an_arrow() {
        val plan = ScreenRules.plan(screen("order"), ScreenContext(presentation = Presentation.FullScreenDialog, canGoBack = true), WindowInfo.Phone)
        assertEquals(NavigationIcon.Back, plan.navigationIcon)
    }

    @Test
    fun the_menu_button_is_on_first_screens_when_destinations_did_not_fit() {
        val home = screen("home")
        assertEquals(NavigationIcon.Menu, ScreenRules.plan(home, root.copy(menuButton = true), WindowInfo.Phone).navigationIcon)
        assertEquals(NavigationIcon.None, ScreenRules.plan(home, root, WindowInfo.Phone).navigationIcon)
        val plan = ScreenRules.plan(home, root, WindowInfo.Phone)
        assertEquals(BarKind.Small, plan.bar)
        assertEquals(listOf("Reports"), plan.barActions.map { label(home, it.index) })
        assertTrue(plan.overflow.isEmpty())
    }

    @Test
    fun a_wide_window_shows_a_list_and_what_was_opened_from_it_side_by_side() {
        val orders = screen("orders")
        assertTrue(ScreenRules.isCollection(orders.body))
        assertTrue(ScreenRules.isCollection(screen("products").body))
        assertFalse(ScreenRules.isCollection(screen("order-edit").body))
        assertFalse(ScreenRules.isCollection(screen("home").body))

        assertTrue(ScreenRules.twoPanes(WindowInfo.Tablet, orders, depth = 2))
        assertTrue(ScreenRules.twoPanes(WindowInfo(900, 1200), orders, depth = 3))
        assertFalse("with nothing opened the list has the window", ScreenRules.twoPanes(WindowInfo.Tablet, orders, depth = 1))
        assertFalse(ScreenRules.twoPanes(WindowInfo.Phone, orders, depth = 2))
        assertFalse(ScreenRules.twoPanes(WindowInfo.Foldable, orders, depth = 2))
        assertFalse(ScreenRules.twoPanes(WindowInfo.Tablet, screen("home"), depth = 2))
        assertEquals(360, ScreenRules.listPaneDp(WindowInfo(900, 1200)))
        assertEquals(412, ScreenRules.listPaneDp(WindowInfo.Tablet))

        val detail = ScreenRules.plan(screen("order"), ScreenContext(detailPane = true, user = true), WindowInfo.Tablet)
        assertEquals("the list is beside it, so there is nothing to go back to", NavigationIcon.None, detail.navigationIcon)
    }
}

class ContentRulesTest {
    private val products = screen("products").body as TableNode

    @Test
    fun a_table_is_a_list_on_a_phone_and_a_table_where_there_is_room() {
        assertTrue(ListRules.tableAsList(412, products.columns.size))
        assertTrue(ListRules.tableAsList(599, products.columns.size))
        assertFalse(ListRules.tableAsList(600, products.columns.size))
        assertFalse(ListRules.tableAsList(1100, products.columns.size))
        assertTrue("too many columns for the width is a list at any width", ListRules.tableAsList(700, 9))
    }

    @Test
    fun a_tables_row_folds_into_a_list_row_by_what_its_columns_say_they_are() {
        val row = ListRules.asListRow(products.columns, products.rows.first())
        assertEquals("Phulkari dupatta, red", row.headline)
        assertEquals(listOf(TextNode(value = "PH-204"), TextNode(value = "Clothing")), row.lines)
        assertEquals(TextNode(value = "₹4,999"), row.trailing)
        assertEquals("24 in stock", (row.badges.single() as com.navree.client.protocol.BadgeNode).text)
        assertEquals(products.rows.first().action, row.action)
    }

    @Test
    fun a_row_of_the_real_orders_table_is_one_touch_target_with_the_order_number_as_its_name() {
        val table = ScreenRules.mainTable(decodeEnvelope(Fixtures.text("store-demo/orders.json")).screen!!.body)!!
        val row = ListRules.asListRow(table.columns, table.rows.first())
        assertEquals("the one control of the naming cell gives the row its name", "CS-220480", row.headline)
        assertEquals("and its action: the form it stands in, sent", Submit(form = "@/orders/200000/peek"), row.action)
        val words = row.lines.filterIsInstance<TextNode>().map { it.value }
        assertEquals(listOf("Tejinder Randhawa", "Today, 21:42"), words)
        assertEquals("the columns a narrow screen may leave out are left out", 2, row.badges.size)
        assertNotNull("the number of the row is at its end", row.trailing)
    }

    @Test
    fun without_roles_a_tables_columns_are_placed_by_position() {
        val columns = listOf("Customer", "City", "Orders", "Spent").map { Column(label = it) }
        val cells = listOf("Harpreet Kaur", "Ludhiana", "12", "₹86,300").map { TextNode(value = it) }
        val row = ListRules.asListRow(columns, TableRow(cells = cells))
        assertEquals("Harpreet Kaur", row.headline)
        assertEquals(listOf(cells[1], cells[2]), row.lines)
        assertEquals(cells[3], row.trailing)
        assertTrue(row.badges.isEmpty())

        val two = ListRules.asListRow(columns.take(2), TableRow(cells = cells.take(2)))
        assertEquals(listOf(cells[1]), two.lines)
        assertNull("two columns are a title and a second line", two.trailing)
    }

    @Test
    fun a_grid_gets_as_many_columns_as_fit() {
        assertEquals(2, ListRules.gridColumns(380, 156, 8))
        assertEquals(1, ListRules.gridColumns(300, 156, 8))
        assertEquals(4, ListRules.gridColumns(792, 156, 8))
        assertEquals(1, ListRules.gridColumns(100, 400, 16))
    }

    @Test
    fun a_choice_becomes_the_material_control_that_fits_it() {
        val phone = WindowInfo.Phone
        assertEquals(ChoiceWidget.Segmented, FormRules.choice(ChoiceKind.Segmented, 3, phone))
        assertEquals(ChoiceWidget.Segmented, FormRules.choice(ChoiceKind.Segmented, 4, phone))
        assertEquals("five segments do not fit a phone", ChoiceWidget.Menu, FormRules.choice(ChoiceKind.Segmented, 5, phone))
        assertEquals(ChoiceWidget.Segmented, FormRules.choice(ChoiceKind.Segmented, 5, WindowInfo.Tablet))
        assertEquals("nor do segments fit the largest text", ChoiceWidget.Menu, FormRules.choice(ChoiceKind.Segmented, 3, phone.copy(fontScale = 2f)))
        assertEquals(ChoiceWidget.Radio, FormRules.choice(ChoiceKind.Radio, 5, phone))
        assertEquals(ChoiceWidget.Menu, FormRules.choice(ChoiceKind.Radio, 6, phone))
        assertEquals(ChoiceWidget.Menu, FormRules.choice(ChoiceKind.Menu, 15, phone))
        assertEquals("a menu that long cannot be scanned", ChoiceWidget.Dialog, FormRules.choice(ChoiceKind.Menu, 16, phone))
        assertEquals(ChoiceWidget.Dialog, FormRules.choice(ChoiceKind.List, 3, phone))
        assertEquals(ChoicesWidget.Chips, FormRules.choices(8))
        assertEquals(ChoicesWidget.Dialog, FormRules.choices(9))
    }

    @Test
    fun dates_and_times_are_typed_when_the_window_is_too_short_for_the_picker() {
        assertEquals(PickerMode.Picker, FormRules.picker(DateKind.Date, WindowInfo.Phone))
        assertEquals(PickerMode.Input, FormRules.picker(DateKind.Date, WindowInfo.PhoneLandscape))
        assertEquals(PickerMode.Input, FormRules.picker(DateKind.Time, WindowInfo.PhoneLandscape))
    }

    @Test
    fun each_kind_of_text_gets_its_keyboard() {
        assertEquals(KeyboardPlan("email", "none", autoCorrect = false, singleLine = true), FormRules.keyboard(TextKind.Email))
        assertEquals("phone", FormRules.keyboard(TextKind.Phone).type)
        assertEquals("decimal", FormRules.keyboard(TextKind.Decimal).type)
        assertEquals("number", FormRules.keyboard(TextKind.OneTimeCode).type)
        assertFalse(FormRules.keyboard(TextKind.Password).autoCorrect)
        assertFalse(FormRules.keyboard(TextKind.Multiline).singleLine)
        assertEquals("a name is capitalised word by word", "words", FormRules.capitalise("name", FormRules.keyboard(TextKind.Plain)))
        assertEquals("sentences", FormRules.capitalise(null, FormRules.keyboard(TextKind.Plain)))
        assertEquals("Full name *", FormRules.label("Full name", required = true))
        assertEquals("Email", FormRules.label("Email", required = false))
    }

    @Test
    fun sheets_and_dialogs_fit_the_window() {
        assertEquals(SheetPlan(partial = true, fullHeight = false), OverlayRules.sheet(SheetSize.Medium, WindowInfo.Phone))
        assertEquals("a short window has no room for a half-open sheet", SheetPlan(partial = false, fullHeight = false), OverlayRules.sheet(SheetSize.Medium, WindowInfo.PhoneLandscape))
        assertEquals(SheetPlan(partial = false, fullHeight = true), OverlayRules.sheet(SheetSize.Full, WindowInfo.Phone))
        assertFalse(OverlayRules.dialogButtonsStack(listOf("Keep order", "Cancel order"), WindowInfo.Phone))
        assertTrue(OverlayRules.dialogButtonsStack(listOf("Keep", "Refund", "Cancel"), WindowInfo.Phone))
        assertTrue(OverlayRules.dialogButtonsStack(listOf("Keep the order as it is", "Cancel the order"), WindowInfo.Phone))
    }

    @Test
    fun what_a_thing_is_picks_its_type_its_colours_and_its_spacing() {
        assertEquals(TypeRole.HeadlineMedium, ContentRules.type(TextLevel.LargeTitle))
        assertEquals(TypeRole.BodyLarge, ContentRules.type(TextLevel.Body))
        assertEquals(TypeRole.BodySmall, ContentRules.type(TextLevel.Caption))
        assertEquals(ToneRole.Error, ContentRules.tone(Tone.Danger))
        assertEquals(ToneRole.Surface, ContentRules.tone(Tone.Neutral))
        assertEquals(listOf(0, 4, 8, 16, 64), listOf(0, 1, 2, 4, 40).map(ContentRules::gap))
        assertEquals(16, ContentRules.marginDp(WindowInfo.Phone))
        assertEquals(24, ContentRules.marginDp(WindowInfo.Foldable))
        assertFalse(ContentRules.factsStacked(WindowInfo.Phone))
        assertTrue("with large text a value goes under its label", ContentRules.factsStacked(WindowInfo.Phone.copy(fontScale = 1.3f)))
    }
}

class IconShapesTest {
    @Test
    fun shapes_are_read_from_path_data_from_objects_and_from_markup() {
        val paths = IconShapes.read(JsonArray(listOf(JsonPrimitive("M5 12h14"), JsonPrimitive("M12 5v14"))))
        assertEquals(listOf(IconPath("M5 12h14"), IconPath("M12 5v14")), paths)

        val markup = "<path d=\"M16 21v-2a4 4 0 0 0-4-4H6\"/><circle cx=\"9\" cy=\"7\" r=\"4\"/>" +
            "<rect width=\"20\" height=\"5\" x=\"2\" y=\"3\" rx=\"1\"/><line x1=\"2\" x2=\"22\" y1=\"10\" y2=\"10\"/>" +
            "<polyline points=\"22 12 16 12 14 15\"/><polygon points=\"1,2 3,4 5,6\"/><circle cx=\"7.5\" cy=\"7.5\" r=\".5\" fill=\"currentColor\"/>"
        val read = IconShapes.read(JsonPrimitive(markup))
        assertEquals(
            listOf(
                IconPath("M16 21v-2a4 4 0 0 0-4-4H6"),
                IconPath("M5 7a4 4 0 1 0 8 0a4 4 0 1 0 -8 0Z"),
                IconPath("M3 3h18a1 1 0 0 1 1 1v3a1 1 0 0 1 -1 1h-18a1 1 0 0 1 -1 -1v-3a1 1 0 0 1 1 -1Z"),
                IconPath("M2 10L22 10"),
                IconPath("M22 12L16 12L14 15"),
                IconPath("M1 2L3 4L5 6Z"),
                IconPath("M7 7.5a0.5 0.5 0 1 0 1 0a0.5 0.5 0 1 0 -1 0Z", fill = true),
            ),
            read,
        )
    }

    @Test
    fun what_is_not_a_shape_is_left_out() {
        val hostile = "<script>alert(1)</script><image href=\"https://evil.example/x.png\"/><path d=\"M1 1h2\" onclick=\"x()\"/><circle r=\"0\"/>"
        assertEquals(listOf(IconPath("M1 1h2")), IconShapes.read(JsonPrimitive(hostile)))
        assertTrue(IconShapes.read(null).isEmpty())
    }

    @Test
    fun every_icon_of_the_fixtures_has_shapes() {
        val shell = decodeShell(Fixtures.text("shell.json"))
        for (item in shell.nav.groups.flatMap { it.items }) {
            assertTrue("${item.label} has an icon to draw", item.icon!!.paths.isNotEmpty())
        }
    }
}
