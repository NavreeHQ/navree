# Sources for the Android skill

Every page below was **read on 2026-11-12**. "Describes" is the version or date
the page itself states. The material.io pages are rendered by script, so they
were read in a real browser; developer.android.com pages are plain pages, and a
Markdown copy of one can be stale, so check its "Last updated" line. Guidance
is summarised and linked in `../SKILL.md`, never copied.

When Android or Material ships a new version, this list is stale: read the pages
again, update the date and the "describes" column, and re-check `../SKILL.md`
and `availability.md` against them.

## Versions and build

| Page | Used for | Describes |
|---|---|---|
| https://developer.android.com/about/versions , `/17`, `/17/behavior-changes-all`, `/17/behavior-changes-17`, `/16/behavior-changes-16` | Android 16 and 17 behaviour changes | Android 17 beta 4.1 notes (2026-06-01); the public release date was not confirmed |
| https://developer.android.com/build/releases/gradle-plugin and `/about-agp` | AGP 9.5.x, Gradle 9.6.0, JDK 17, build-tools 36 | AGP 9.5.0 (October 2026) |
| https://developer.android.com/develop/ui/compose/bom/bom-mapping | Compose BOM to library versions | BOMs to 2026.09.00 |
| https://developer.android.com/jetpack/androidx/releases/compose-material3 (and `compose`, `compose-material3-adaptive`, `navigation`, `activity`, `lifecycle`, `core`, `window`, `glance`, `wear-compose`) | Release notes; Material 3 1.4.0 and 1.5.0 alpha to rc01 | updates 2026-10-07 to 2026-11-04 |
| https://dl.google.com/dl/android/maven2/ and https://repo1.maven.org/maven2/ | Newest versions; each library's minimum compile SDK | 2026-11-12 |
| https://dl.google.com/android/repository/repository2-3.xml | SDK platforms and channels | 2026-11-12 |
| https://kotlinlang.org/docs/releases.html | Kotlin 2.4.21 | 2026-10-08 |

## Layout, navigation and system behaviour

| Page | Used for | Describes |
|---|---|---|
| https://developer.android.com/develop/ui/compose/layouts/adaptive/use-window-size-classes | Width and height classes | current |
| https://developer.android.com/docs/quality-guidelines/adaptive-app-quality and `/tier-1`, `/tier-2`, `/tier-3` | Large-screen quality tiers | last updated 2026-10-21 |
| https://developer.android.com/develop/ui/views/layout/edge-to-edge | Edge-to-edge | Android 15 and 16 |
| https://developer.android.com/guide/navigation/custom-back/predictive-back-gesture | Predictive back | AndroidX Activity 1.6 and later |
| https://developer.android.com/guide/topics/resources/app-languages | Per-app language | Android 13 and later |
| https://developer.android.com/develop/ui/views/launch/icon_design_adaptive | Themed icons | Android 13 and 16 QPR2 |
| https://developer.android.com/design/ui/wear and the Wear guides | Wear OS design (not built) | Material 3 Expressive for Wear |
| https://developer.android.com/develop/ui/compose/glance , `/develop/ui/views/notifications`, `/develop/ui/views/launch/shortcuts` | Surfaces outside the app | pages updated 2026-09 or earlier |

## Material 3

`https://m3.material.io/components/<name>/guidelines` for: navigation-bar,
navigation-rail, navigation-drawer, top-app-bar, bottom-app-bar (the toolbars),
floating-action-button, extended-fab, fab-menu, lists, cards, text-fields,
menus, date-pickers, time-pickers, switch, checkbox, radio-button, sliders,
segmented-buttons, chips, buttons, button-groups, icon-buttons, split-button,
bottom-sheets, side-sheets, dialogs, search, snackbar, progress-indicators,
loading-indicator, badges, tabs, carousel, tooltips. Material 3 Expressive, as
published on 2026-11-12. The pages for banners, dividers, empty states and data
tables returned no content, which is why the client defines those mappings
itself.

| Page | Used for |
|---|---|
| https://m3.material.io/foundations/layout/applying-layout/window-size-classes and `/canonical-layouts/{overview,list-detail,supporting-pane,feed}`, `/foundations/adaptive-design/large-screens/overview` | Breakpoints and canonical layouts |
| https://m3.material.io/foundations/layout/understanding-layout/{overview,spacing}, `/foundations/interaction/{gestures,states/overview}` | Spacing and interaction |
| https://m3.material.io/styles/color/dynamic-color/overview , `/styles/color/dynamic/user-generated-source`, `/styles/color/roles`, `/foundations/designing/color-contrast` | Dynamic colour, roles, contrast |
| https://m3.material.io/styles/typography/type-scale-tokens , `/typography/applying-type`, `/styles/shape/overview-principles`, `/styles/motion/overview/how-it-works`, `/styles/icons/overview`, `/styles/icons/applying-icons` | Type, shape, motion, icons |
| https://github.com/material-foundation/material-color-utilities | The colour space and the tonal-spot scheme (pushed 2026-11-06) |

## Google's own agent skills

https://github.com/android/skills (`jetpack-compose/adaptive`,
`system/edge-to-edge`, `wear/wear-compose-m3`, `system/glance-canonical-layouts`,
`navigation/navigation-3`), read on 2026-11-12; the `adaptive` skill is dated
2026-11-05. Load them beside this skill; do not contradict them.

## Not confirmed on 2026-11-12

The Android 17 public release date; Play's next target-level deadline; the
mapping of Android 14's contrast setting to Navree's contrast levels; the
exact numbers of the 2025 and 2026 tonal-spot palettes; the minimum compile SDK
of the third-party colour libraries.
