# navree-ui components, and the design elements they take

Phases 3 and 5. Every component here exists in `crates/navree-ui`
(`src/components`, `src/dates`, `src/flow`, `src/pages`, `src/form.rs`,
`src/select`, `src/sortable`, `src/table`, `src/theme`, `src/shell.rs`),
imported from `navree::ui` (or `navree::ui::prelude::*`). A design element
maps to one row. When no row fits, the element is a gap: record it with the
"Coming" table at the end, never build a stand-in from raw elements, a
stylesheet or a script. The skill [`navree-ui`](../../navree-ui/SKILL.md)
teaches building the pages themselves, and has the full index of components.

Each component is a Rust struct with builder methods that becomes a
`Node`. Variants are enums, so a typo doesn't compile. Components carry no
outer margin: spacing between them comes from `Stack`, `Row` and `Grid`.
The stylesheet reads only `data-*` attributes: a `class` does nothing, and
`style` is dropped.

## The frame

| Design element | navree-ui | Notes |
|---|---|---|
| App frame: sidebar, top bar, content | `Shell` (registered once), `AppShell` (extractor), `AppPage` | `shell.page(title).crumb(..).actions(..).body(..)` |
| Sidebar group, link, nested link, count | `NavGroup::new(label)`, `NavGroup::plain()`, `NavItem::new(label, href).icon(..).count(..).sub(..)` | `NavItem::current(true)` when several links differ only in their query |
| Product name, environment tag | `Shell::new(name)`, `.context("Production")` | the context is text, not a pill |
| Search in the top bar | `Shell::search(node)` | a form with `Input::search`, or a command palette: `Node::list((Command::trigger(id, label), palette))` |
| User at the foot of the sidebar | `UserMenu::new(name, email).link(..).action(..)` | `action` posts; the avatar is an icon, never initials |
| Breadcrumbs | `AppPage::crumb(label, href)`; on a plain page `Breadcrumb::new().link(..).current(..)` | |
| Page title and page buttons | `shell.page(title)`, `.actions(..)` | one primary action per page |
| A page outside the shell (sign-in, receipt) | `View::new(title).body(..)` | |
| A module's own frame (a tool page) | `Shell::for_request(path, &headers)` | |
| Sidebar layouts, folding | `Shell::layout(ShellLayout::Sidebar \| Inset \| Floating \| Header)`, `Shell::collapse(Collapse::Icons \| Offcanvas \| Never)` | the choice is a cookie read on the server |
| Light and dark switch, theme picker | `Shell::appearance(Appearance::new(Theme::OCEAN, Mode::System))`, `Shell::theme_menu()` | see "Themes and modes" below |

## Layout

| Design element | navree-ui |
|---|---|
| Vertical rhythm | `Stack::gap(n).align(..).children(..)` |
| A row of things, toolbars | `Row::gap(n).align(..).justify(..).wrap()` |
| Columns, card grids, dashboards | `Grid::cols(1..=6).gap(n)`; a child spans with `.span(2)` (cards and other root-method components) |
| A centred column of fixed width | `Container::new().size(ContainerSize::Sm \| Md \| Lg \| Full)` |
| Right to left | `Direction::rtl(..)`, or the page language; the table mirrors itself |
| Hide below 768 px | `data-hide-mobile` (`TableHead::hide_on_mobile`, `#[col(hide_on_mobile)]`) |
| Two panes side by side | a `Grid` with `data-pane` on the pane that scrolls on its own |
| Resizable panes (a list, a thread, a detail) | `SplitPane::new(id).pane(Pane::new(id, label).size(30).min(22).max(45)).pane(Pane::fill(id, label))`, `.stack_below(SplitWidth::Md)`, `.save_to(url)` for sizes kept on the server |

## Text and display

| Design element | navree-ui |
|---|---|
| Headings | `Heading::h1..h4(text)`: the level is the document's |
| Paragraph, secondary text, lead, code | `Text::new`, `Text::muted`, `.variant(TextVariant::Lead)`, `Text::code`; `.inline()` |
| Panel, card, tile | `Card::new()` / `Card::titled(..)`, `.description(..)`, `.action(..)`, `.body(..)`, `.footer(..)` |
| Status label, tag, chip | `Badge::new(text).variant(BadgeVariant::Neutral \| Outline \| Success \| Warning \| Danger).icon(..)`: 4 px corners, not a pill |
| Inline message, banner | `Alert::new(AlertTone::Neutral \| Success \| Warning \| Danger).title(..).body(..)` |
| Figure with label and change | `Stat::new(label, value).trend(text, good)` |
| Small bar chart | `Bars::new(description, &[values])`: SVG on the server |
| Person picture | `Avatar::image(src, alt)`, `Avatar::placeholder(label)`: no initials, no letter tile |
| List row with media and actions | `Item::new(title).media(..).description(..).actions(..).href(..)`, `ItemVariant` |
| File tile | `Attachment::new(name).meta(..).icon(..).href(..).action(..)` |
| Chat | `Message::new(BubbleSide::Start \| End).avatar(..).header(..).content(..)`, `Bubble`, `Marker` |
| Keyboard key | `Kbd::new("K")` |
| Progress bar, spinner | `Progress::new(value, max, label)`, `Progress::indeterminate(label)`, `Spinner::new(label)` |
| Placeholder while loading | `Skeleton::new(SkeletonShape::Line \| Short \| Block)` |
| Nothing here yet | `Empty::new(title).icon(..).description(..).actions(..)`; for a list, `EmptyState::nothing_yet(what)`, `EmptyState::no_results().clear_filters(url)` and `EmptyState::caught_up()`; in a table, `.row(columns)` |
| A part the user may not see, a record that is gone, a part that needs sign-in | `Forbidden::new().what(..)` (never says whether the thing exists), `NotFound::new().what(..)`, `SignInRequired::new(sign_in_url).what(..)`; each also `.compact()` for one line |
| A whole error page (401, 403, 404, 500, ...) | `UiModule` draws it for every page route; your own is `ErrorPage::not_found()` or `ErrorPage::of(StateKind::..)` |
| Session ended notice | `SessionEnded::new()` |
| Figure with a label and a detail | `FactCard::new(label, value).detail(..)` in a `FactGrid` |
| History of events, in a time zone | `Timeline::new(id, label).time_zone("Asia/Kolkata", 19_800).event(TimelineEvent::new(..).at(secs).tone(Tone::Success).detail(..).meta(Meta::icon(..)))` |
| Recent activity feed | `ActivityFeed::new(id, label).summary(..).item(ActivityItem::new(who, what).object(..).tone(..).icon(..).time_text(..))` |
| Divider | `Separator::horizontal()`, `Separator::vertical()` |
| Picture held to a shape | `AspectRatio::new(Ratio::Square \| Video \| Photo \| Wide, content)` |
| A small static table | `Table::new().caption(..).head(..).rows(..)` with `TableHead`, `TableRowEl`, `TableCell` |
| Page links under a static list | `Pagination::new(page, pages, href_fn)` |
| Icon | `icons::NAME` on a component (`.icon(..)`); `icon(icons::NAME)` as a node, hidden from screen readers |

Icons are lucide's, a fixed set of 145 in `src/icons.rs` (`icons::PLUS`,
`icons::HANDSHAKE`, ...). An icon the design uses that isn't in the set is
a gap: the set grows in the toolkit, not in the app.

## Actions

| Design element | navree-ui |
|---|---|
| Button | `Button::primary`, `Button::outline`, `Button::ghost`, `Button::destructive`, `Button::new(..).variant(ButtonVariant::Secondary \| Link)`; `.size(Size::Xs \| Sm \| Md \| Lg)`, `.icon(..)`, `.icon_end(..)`, `.block()`, `.disabled(..)`, `.pending(..)` |
| Link styled as a button | `Button::...(..).href(url)`; `.prefetch()` for safe links |
| Icon-only button | `Button::icon_only(icons::NAME, "Accessible name")` |
| Button that opens a dialog | `.opens_dialog(id)`, `.closes_dialog(id)` |
| Button group | `ButtonGroup::new(label).children(..)`, `.vertical()` |
| Two-state button | `Toggle::new(name, label).pressed(..)`: a checkbox underneath, so it posts |
| Segmented control, view switch | `ToggleGroup::single(name, label).item(ToggleItem::new(value, label))`, `.value(..)`, `.variant(ToggleVariant::Outline)`, `.joined()`; `ToggleGroup::multiple(..)` for several on. Radios and checkboxes underneath, so it posts |
| Menu from a button | `DropdownMenu::new(id, label).item(MenuItem::link(..) \| action(..) \| post(..))`, `.separator()`, `.group(MenuGroup::new(..))`, `.checkbox(..)`, `.radios(..)`, `.submenu(MenuSub::new(..))`; `DropdownMenu::icon(id, icon, label)` for a row's menu, `.align(PopupAlign::End)` |
| Behaviour with no request (show, hide, copy, focus, toast) | `.on_click(Cmd::toggle(id))` and the rest of `Cmd`: a closed list, no script |
| Answer from a server action | `Reply::redirect(..)`, `Reply::ops(..)`, `.morph(target, node)`, `.toast(ToastTone, text)`, `.toast_with(Toast::new(tone, text).title(..).action(ToastAction::post(label, url)))` |

## Overlays

| Design element | navree-ui |
|---|---|
| Modal | `Dialog::new(id, title).description(..).body(..).footer(..)` |
| Confirmation | `AlertDialog::new(id, title, description).cancel(..).action(..)` |
| Side panel | `Sheet::new(id, title).side(SheetSide::Right \| Left \| Top \| Bottom)` |
| Bottom drawer on phones | `Drawer::new(id, title)` |
| Popover | `Popover::new(id, trigger).side(..).align(..).body(..)` |
| Tooltip | `Tooltip::new(id, trigger, text).side(..)` |
| Hover card | `HoverCard::new(id, trigger).body(..)` |
| Tabs | `Tabs::new(id, label).tab(Tab::new(key, label).panel(..))`, `.active_from(query.tab)`, `.variant(TabsVariant::Segmented)`, `.vertical()`; `Tab::lazy(src)` loads a panel when first shown |
| Toast | `Toast`, `Toaster` and `Reply::toast`; four tones, a title, one action, a time, a stack |
| Accordion, disclosure | `Accordion::new().item(..).open_item(..).exclusive(name)`, `Collapsible::new(title).open(..).body(..)` |
| Scrolling panel | `ScrollArea::new(label).children(..)` |
| Command palette (Ctrl or Cmd and K) | `Command::new(id).placeholder(..).recent(n).group(CommandGroup::new(label).item(..))`; items are `CommandItem::link`, `post`, `run` and `disabled`, each with `.icon(..)`, `.shortcut("mod+shift+n")` and `.keywords(..)`; `Command::trigger(id, label)` is its button; `.source(url)` adds results from the server; `.csrf(token)` for posted items with script off |

## Forms

A design's form is one struct with `#[derive(Deserialize, Validate,
Form)]`, drawn by `FormView` and read back by `Submit<T>`
(`building-pages.md`). Don't place inputs one by one unless the design's
form can't be a struct.

| Data the field holds | Control | From |
|---|---|---|
| Short text | text input | `String`; `#[form(widget = email \| url \| tel \| password \| search)]` |
| Long text | textarea | `#[form(widget = textarea, rows = 5)]` |
| Whole number, decimal | number input | an integer or float type; `range` rules set `min`, `max` |
| Money | amount input, typed `12,500.50` | `Money` (minor units); its currency in an `addon` |
| Calendar day | date picker: a typed date or a calendar, a native date field with JavaScript off | `NaiveDate` with `widget = date_picker` (the plain `date` widget is the browser's own) |
| Time of day | time picker: typed or from a list, a native time field with JavaScript off | `NaiveTime` with `widget = time_picker` |
| Day and time together | a date picker and a time picker side by side, or the browser's own input | two fields; `widget = datetime` for the native one |
| A window of days, for a report | date range picker with presets, its two dates in the address | `DateRangePicker` in a `GET` form, read back with `DateRangeQuery` |
| A month grid to browse, or to pick days | calendar | `Calendar::new(id)`, `.mode(CalendarMode::Multiple \| Range)`, `.rules(DateRules::new()..)` for days that can't be chosen |
| A phone number | phone input: a country list and the number, formatted as typed | `PhoneInput::new(name).country("IN")`; a `Phone` or `PhoneNumber` field, or `widget = phone` on a `String` (E.164) |
| A number on a scale, a range | slider | `Slider::new(name).max(100).step(5)`, `Slider::range(name)`; `SliderSpec` holds the post to the same grid |
| Yes or no | checkbox, or switch | `bool`; `widget = switch` |
| One of a small fixed set | radio buttons when all of them are worth seeing, else a select | an enum with `#[derive(Options)]`: `widget = radio`; with no hint, the browser's select (`NativeSelect`); `widget = listbox`, the styled `Select` |
| One of a short list the server holds (a few dozen at most) | select | `String` with `widget = select` or `listbox`; choices from `FormView::options(field, ..)` |
| One of a large or growing set | combobox, searched on the server | `String` with `widget = combobox`; `FormView::search(field, url)`; the route answers `OptionQuery` with a `ChoiceList` built by `OptionSource` (below) |
| Several of a set | multi-select, shown as chips | `Many<T>` (a multi-select with no hint); `#[form(max = n)]` limits it; options and search as for a combobox |
| Something with a prefix or unit | input group | `#[form(addon = "other_field")]`, or `InputGroup::new(input).start(..).end(..)` |
| A file | file input | `Input::new(InputKind::File, name)` in a form of your own that posts `multipart/form-data` to an upload route (`navree-storage`): `Submit<T>` reads only urlencoded forms |
| A hidden value | hidden input | `widget = hidden`, or `#[form(skip)]` to keep it off the client |

By hand, when a form isn't a struct: `Field::new(id, label).help(..)
.error_opt(..).required().control(..)`, `FieldSet::new(legend)`, `Label`,
`Input::text(name)` (and `email`, `password`, `search`, `number`, `date`,
`Input::new(InputKind::..)`), `Textarea`, `NativeSelect::new(name)
.option(..).options(..).selected(..).prompt(..)`, `Select::new(name)
.option(..).placeholder(..).selected(..).clearable()`, `Combobox::new(name)
.options(..).selected(..)`, `MultiSelect::new(name).options(..)
.selected_values(..).max(n)`, `Checkbox`, `Switch`, `RadioGroup`.

### Pickers over a large or growing set

A picker over records the server holds is searched on the server, never
filled with every row. Four parts, with the code in `building-pages.md`:

- **The field:** `widget = combobox` (or a `Many<T>` multi-select) and
  `FormView::search(field, "/companies/options")`. An edit form also gives
  the chosen record's label with `FormView::options(field, [(id, name)])`;
  without it the picker shows the id.
- **The route:** it takes `OptionQuery` (the text, a limit of at most 50
  and an offset; it never fails) and answers `ChoiceList`.
  `OptionSource::new(value_column, label_column).search_in(&[..])` runs the
  search on the caller's own scoped query (`Accessible<M, Read>`), so
  workspace scoping and row-level security hold. The columns come from the
  code, never from the request.
- **The check on save:** a form reads the picked value as text and doesn't
  know it is one of the options. The handler asks
  `OptionSource::all_pickable(scoped_query, values, &uow, OptionSource::as_uuid)`
  (`as_int` and `as_text` too) with the same scoped query, and answers a
  failure with `Rejected::after_decode(&value, errors, &parts)`. For a
  fixed list in the page, `Choice::all_among(values, &choices)` is the same
  check. Never trust what the page showed.
- **Without script:** a searched combobox shows its chosen option and can't
  change it, because there is nothing to ask. A page that must work with
  script off uses a select, which holds a short list, and says so in its
  page spec.

Options in the page (a `Select`, or a `Combobox` with `.options(..)`) suit
lists up to about a thousand; past that, search on the server. A `Choice`
can carry `.description(..)`, `.icon(..)`, `.avatar(src)` (a URL on this
site) and `.keywords(..)`. `.creatable()` and `.create_url(url)` let the
person add an option the list lacks; the route behind `create_url` must
check that the caller may create.

## Lists

A design's data table is `#[derive(TableRow)]` and `DataTable`
(`building-pages.md`; the toolkit's README has every option). Search,
filters, sort, pages, selection, bulk actions and export run on the server.

| Design element | `DataTable` |
|---|---|
| Column headers that sort | `#[col(sortable)]`; several at once in the URL (`sort=-amount,title`) |
| Search box over the table | `#[col(search)]` on the searched columns |
| Filter per column | `#[col(filter = text \| range \| date \| select \| multi \| boolean \| relation)]`; choices `options = LIST` or `.options(col, Vec<FilterOption>)` from the server |
| A filter row under the headers | `.filter_row()` |
| Filters in a bar over the table | `.filter_bar()`, or `#[derive(FilterBar)]` for a toolbar of your own |
| Pages and page size | `#[table(per = 25, per_pages(10, 25, 50, 100))]`; `#[table(cursor)]` for huge tables |
| Status column | `#[col(badge = tone_fn)]` |
| Link in a cell, whole-row link | `#[col(link = "/deals/{id}")]`, `.row_href(fn)` |
| Row menu | `.row_actions(fn)` with `RowAction::link`, `RowAction::post`, `.destructive()` |
| Select rows, bulk actions | `.bulk(url, [BulkAction::new(..).confirm(..)])`, `BulkRequest` on the server |
| Expand a row | `.detail(fn)` |
| Columns shown, hidden, pinned, resized | `#[col(hidden, locked, sticky, sticky_end, width = 240)]`; the user's choice is in the URL |
| Density | the user's choice, in the URL |
| Export | `.export(url)`; Excel through `navree-sheets` |
| Saved views | `.views(&views, url)` with `SavedViewModel` |
| Cells drawn from the whole row | `#[col(cell = draw)]`, `.cell(name, fn)` |
| Edit in the cell | `EditCell::text`, `number`, `date`, `select` |
| Live data | `.live(every)`, `.live_from(url)`, `.refreshable()` |
| Empty, loading, failed | `.empty(Empty::..)`, `.loading()`, `.failed(msg)` |

One table per URL: its state is in the query string. A second table on
the page gets its own route or a `Region`.

Other lists and moves:

| Design element | navree-ui |
|---|---|
| An inbox, a conversation, a feed: a stream read in one order | `LazyList::new(id, label, src).items(..).more_after(url)`; the source reads `LazyQuery` and answers `LazyPage` |
| Drag to reorder, a board of columns | `Sortable::new(id, label).board().post(url).list(SortList::new(key, title).item(SortItem::new(key, title)))`; the server holds each `Move` to the lists with `Move::check` |

## Steps and trees

| Design element | navree-ui |
|---|---|
| Step bar | `Stepper::new(label).step(Step::new(..).state(StepState::Error)).current(n)`, `.vertical()`, a step that is a link: `Step::new(..).link(href)` |
| A form in steps, with a review | `Wizard::new(id).base(url).step(WizardStep::of::<T>(id, label)).review()`; each step is a `Form` struct, progress is a `Draft` on the server (`DraftStore`) |
| Tree of folders, links or choices | `Tree::new(id, label).node(TreeNode::branch(..) \| leaf(..) \| lazy(id, label, src))`, `.select(TreeSelect::Single \| Multiple)`, `.cascade()` |
| Row of tools with a More menu | `Toolbar::new(id, label).item(ToolbarItem::button(..) \| icon_toggle(..) \| radios(..) \| link(..) \| group(..) \| separator())` |

## Parts that load later

`Region::new(id).src(url).lazy()` fetches a part after the page paints;
`.poll(every)` refreshes it while visible; `.feed(..)` follows a stream;
`.live_from(url)` refetches when a server-sent event stream speaks.
The handler returns a `View` of just that part.

## Themes and modes

| Design element | navree-ui |
|---|---|
| A light design, a dark design, "follow the device" | `Mode::Light`, `Mode::Dark`, `Mode::System` in `Appearance::new(theme, mode)` |
| Brand colours, other corners, density or font | a built-in theme (`Theme::NAVREE`, `NEUTRAL`, `OCEAN`, `VIOLET`, `SUNSET`, `ROSE`, `CONTRAST`), or a theme of the app's own from the theme builder (`Theme::custom`); see `tokens.md` |
| A theme per workspace or per person | `shell.workspace(ThemeSetting)` (`.locked()`), `shell.user(ThemeSetting)`, `Shell::theme_menu()` |

## Errors and states

A request for a page that ends in a 401, 403, 404, 405, 409, 410, 413, 419,
422, 429 or a 5xx gets a page in the shell, with the next step and a
reference to quote. `UiModule` draws it; a design needs nothing more than
the sign-in address (`[ui] sign_in`) and, for its own words,
`PagesOptions::labels`. A route marked `#[page]` (`RouteDef::page()`) always
answers a browser with a page; a route marked `#[json_only]`
(`RouteDef::json_only()`) always answers with Navree's JSON error, and so
does a route under `http.api_prefix` that is not marked. The states inside
a page (`Forbidden`, `NotFound`, `SignInRequired`, `EmptyState`) are in
"Text and display".

## Coming in the next release

| Module | Covers in a design | Interim form, if the owner accepts one |
|---|---|---|
| Charts | line, bar, area and similar charts with a shared palette, and reports with a date window | `Stat` and `Bars` |

## Coming

What navree-ui doesn't offer yet. A design element that needs one of these
is a `gap` row in `GAPS.md` with an interim form the owner picks, or it
waits.

| Module | Covers in a design | Interim form, if the owner accepts one |
|---|---|---|
| Rich text | a formatted text editor | a textarea |
| Islands | a client-side widget of the app's own | none |

Also not there yet:

- A workspace switcher in the shell's sidebar header:
  `navree_workspace::WorkspaceSwitcher` goes in the page or in the shell's
  search slot.
- A date and a time in one control: use a date picker and a time picker.
- An app's own stylesheet or static files: the toolkit serves one
  stylesheet, and the content security policy loads styles, scripts,
  fonts and images only from the app's own origin.
- Web fonts other than Geist: a theme picks Geist or the System, Humanist,
  Geometric or Serif stack, and monospace text uses the system stack.

Check `crates/navree-ui/README.md`, `src/components/mod.rs` and
`src/select/mod.rs` before naming a gap: the list shrinks as modules land.
