package com.navree.client.protocol

import kotlinx.serialization.Serializable

/** Where a client asks for the shell. */
const val SHELL_PATH = "/_navree/native/shell"

/**
 * What wraps every screen: the app's name, its theme, its top-level
 * destinations, the signed-in user and how routes are presented. Fetched once
 * and again when its `build` changes.
 */
@Serializable
data class Shell(
    val navree: Int = PROTOCOL_MAJOR,
    val build: String? = null,
    val name: String = "",
    /** A few words under the name: which part of the business this is. */
    val context: String? = null,
    /** The address the app opens at. */
    val home: String = "/",
    val theme: Theme? = null,
    val nav: Nav = Nav(),
    val user: User? = null,
    val routes: List<Route> = emptyList(),
    val device: DeviceFeatures? = null,
    val upgrade: Upgrade? = null,
)

/** The brand's tokens. The client maps them into the system's own theme. */
@Serializable
data class Theme(
    val name: String? = null,
    /** The accent as `#rrggbb`: the seed of the tonal palette. */
    val accent: String? = null,
    @kotlinx.serialization.SerialName("accent_dark") val accentDark: String? = null,
    /** `sharp`, `rounded` or `round`. */
    val corner: String? = null,
    /** Whether the user's own wallpaper colours may replace the brand's. */
    val dynamic: Boolean? = null,
)

@Serializable
data class Nav(
    val groups: List<NavGroup> = emptyList(),
)

@Serializable
data class NavGroup(
    val label: String? = null,
    val items: List<NavItem> = emptyList(),
)

@Serializable
data class NavItem(
    val label: String = "",
    val icon: IconNode? = null,
    val href: String = "",
    @Serializable(with = LooseString::class) val badge: String? = null,
    /** Destinations under this one. */
    val items: List<NavItem> = emptyList(),
)

@Serializable
data class User(
    val name: String = "",
    val email: String? = null,
    val avatar: String? = null,
    val menu: List<Node> = emptyList(),
)

/** How addresses that match are presented, whatever link led to them. */
@Serializable
data class Route(
    /** A path; `*` stands for one segment and a trailing `**` for the rest. */
    val match: String = "",
    val present: Present? = null,
    val chrome: Boolean = true,
)

@Serializable
data class DeviceFeatures(
    val caps: List<String> = emptyList(),
)
