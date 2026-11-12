package com.navree.client.design

import com.navree.protocol.Navree

/**
 * Which Material Symbol stands for an icon the page names. Android's own apps
 * draw their actions and destinations with Material Symbols: outlined at
 * rest, filled when selected. A page names its icons in the web's icon set,
 * so the names that mean the same thing are mapped here; an icon whose name
 * maps nowhere keeps the page's own shape.
 *
 * Standard actions (share, delete, add, edit, search, done, cancel, more)
 * always take the system's symbol, whatever icon the page sent.
 */
object Symbols {
    /** The symbol of each standard action. */
    fun of(standard: Navree.Standard): String = when (standard) {
        Navree.Standard.SHARE -> "share"
        Navree.Standard.DELETE -> "delete"
        Navree.Standard.ADD -> "add"
        Navree.Standard.EDIT -> "edit"
        Navree.Standard.SEARCH -> "search"
        Navree.Standard.DONE -> "check"
        Navree.Standard.CANCEL -> "close"
        Navree.Standard.MORE -> "more_vert"
    }

    /**
     * The symbol for a page's icon name, or null. A name that already is a
     * symbol's (`shopping_bag`) is taken as it is.
     */
    fun of(name: String?): String? {
        if (name == null) return null
        val key = name.trim().lowercase()
        names[key]?.let { return it }
        val snake = key.replace('-', '_')
        return snake.takeIf { it in SymbolPaths.all }
    }

    /** Whether the client carries the drawing of this symbol. */
    fun has(symbol: String): Boolean = symbol in SymbolPaths.all

    /** The path data of a symbol, plain or filled. */
    fun paths(symbol: String, filled: Boolean): String? = SymbolPaths.all[symbol]?.let { if (filled) it.second else it.first }

    private val names: Map<String, String> = mapOf(
        "layout-dashboard" to "dashboard",
        "gauge" to "dashboard",
        "home" to "home",
        "house" to "home",
        "shopping-bag" to "shopping_bag",
        "shopping-cart" to "shopping_bag",
        "boxes" to "inventory_2",
        "box" to "inventory_2",
        "package" to "package_2",
        "archive" to "archive",
        "truck" to "local_shipping",
        "users" to "group",
        "users-round" to "group",
        "user" to "person",
        "user-round" to "person",
        "circle-user" to "account_circle",
        "settings" to "settings",
        "cog" to "settings",
        "inbox" to "inbox",
        "mail" to "mail",
        "message-square" to "chat",
        "message-circle" to "chat",
        "messages-square" to "chat",
        "kanban" to "view_kanban",
        "square-kanban" to "view_kanban",
        "bar-chart" to "bar_chart",
        "bar-chart-3" to "bar_chart",
        "chart-bar" to "bar_chart",
        "chart-column" to "bar_chart",
        "line-chart" to "show_chart",
        "chart-line" to "show_chart",
        "bell" to "notifications",
        "search" to "search",
        "plus" to "add",
        "circle-plus" to "add",
        "pencil" to "edit",
        "pen" to "edit",
        "square-pen" to "edit",
        "trash" to "delete",
        "trash-2" to "delete",
        "share" to "share",
        "share-2" to "share",
        "x" to "close",
        "check" to "check",
        "check-check" to "done_all",
        "circle-check" to "check_circle",
        "check-circle" to "check_circle",
        "more-vertical" to "more_vert",
        "ellipsis-vertical" to "more_vert",
        "more-horizontal" to "more_horiz",
        "ellipsis" to "more_horiz",
        "log-out" to "logout",
        "receipt" to "receipt_long",
        "receipt-text" to "receipt_long",
        "credit-card" to "credit_card",
        "wallet" to "account_balance_wallet",
        "indian-rupee" to "payments",
        "banknote" to "payments",
        "calendar" to "calendar_today",
        "calendar-days" to "calendar_today",
        "clock" to "schedule",
        "file" to "description",
        "file-text" to "description",
        "download" to "download",
        "upload" to "upload",
        "copy" to "content_copy",
        "printer" to "print",
        "refresh-cw" to "refresh",
        "rotate-cw" to "refresh",
        "filter" to "filter_list",
        "list-filter" to "filter_list",
        "arrow-up-down" to "sort",
        "star" to "star",
        "heart" to "favorite",
        "tag" to "sell",
        "tags" to "sell",
        "store" to "storefront",
        "layout-grid" to "grid_view",
        "list" to "list",
        "folder" to "folder",
        "image" to "image",
        "link" to "link",
        "external-link" to "open_in_new",
        "help-circle" to "help",
        "circle-help" to "help",
        "info" to "info",
        "alert-triangle" to "warning",
        "triangle-alert" to "warning",
        "alert-circle" to "error",
        "circle-alert" to "error",
        "undo" to "undo",
        "undo-2" to "undo",
        "send" to "send",
        "paperclip" to "attach_file",
        "phone" to "call",
        "map-pin" to "location_on",
        "globe" to "language",
        "lock" to "lock",
        "key" to "key",
        "eye" to "visibility",
        "eye-off" to "visibility_off",
        "history" to "history",
        "arrow-left" to "arrow_back",
        "arrow-right" to "arrow_forward",
        "chevron-right" to "chevron_right",
        "chevron-down" to "expand_more",
        "chevron-up" to "expand_less",
        "trending-up" to "trending_up",
        "trending-down" to "trending_down",
        "menu" to "menu",
    )
}
