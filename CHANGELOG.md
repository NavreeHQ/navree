# Changelog

Every change to Navree is listed here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Navree follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every crate and
every `@navree/*` package carries the same version.

## [Unreleased]

### Upgrading

- To see before upgrading whether the single sign-on change below affects your app, list the
  connections that have `allow_unverified_emails` on (on MySQL and MariaDB, select
  `HEX(owner_id)`):

  ```sql
  SELECT slug, owner_type, owner_id FROM sso_connections
  WHERE provisioning LIKE '%"allow_unverified_emails":true%';
  ```

  Those owned by a workspace or an organization stop creating accounts outside their verified
  domains unless you set `sso.tenants.unverified_emails = true`. Those owned by the app keep
  working, and the boot names them in an `info` line: check that your own staff made each one.

### Security

- A single sign-on connection owned by a workspace or an organization creates accounts only for
  addresses in its verified domains: a customer's connection with `allow_unverified_emails` no
  longer creates accounts outside its verified domains; set `sso.tenants.unverified_emails = true`
  to keep 1.2.1's behaviour. Connections owned by the app are unchanged, and people already linked
  through a connection keep signing in. The boot names every connection of a workspace or an
  organization that has `allow_unverified_emails` on, and a first sign-in it refuses answers 403
  `unverified_domain`, is audited as `sso.login_refused` and is logged (one line an hour per
  connection in each process). The `sso.tenants` section is new in this patch release so that an
  app can keep the earlier behaviour, and it is read also when `SsoModule::for_root` is used. A
  page that lets a workspace's administrators manage their connection sets the connection's
  `owner` itself and checks it before every change (see Single sign-on).

### Fixed

- A module given its options in code (`for_root`) is no longer reported for keys of a section
  nested in its own that is registered and read on its own, such as `auth.apps`, `notify.push`,
  `observe.tracing` or `sso.tenants`; `app.strict_config` no longer stops the boot on them. Other
  keys of the module's section are still reported.

## [1.2.1] - 2026-11-14

### Upgrading

Most apps upgrade with no change. These are the changes that can ask something of an app:

- A route that answers a region, a lazy tab or a lazy sidebar group answers with a `View` or with
  `pages::region_answer(node)`; an answer written another way is no longer drawn. The source of
  one is a path of the site: a whole address is refused, with a warning in the log.
- The route a form's file field sends its file to answers with
  `Received::into_keyed_response_for(&signer, owner)`, with `UploadOwner` as the owner; a key made
  with `file_key` is no longer taken by a form's file field.
- Rich text shown from the database sits inside an element marked `data-ui-stored`, and an
  `HtmlPolicy` that allows an attribute the page acts on fails the boot.
- A test that posts a Navree UI form with a cookie, in an app with no session layer, adds
  `sec-fetch-site: same-origin` to its request.
- A table's CSV export stops at 100,000 rows unless the query asks for more with `export_limit`.
- Social sign-in no longer links to a user whose own address is not verified: it answers 409
  `email_taken` until the address is verified.
- `AuthService::confirm_action` approves its action only and no longer counts as a recent sign-in
  for `#[recent_auth]`.
- An app with no configuration shows an error page's development details only with
  `PagesOptions::debug(true)`.
- Hand-over codes and call tickets issued before the upgrade are refused; a code lives a minute
  and a ticket a few minutes.

### Security

- Social sign-in links a provider account to an existing user by email address only when the
  provider verified the address and the user's own address is verified too. A user whose address
  was never verified is refused as an app set to `link_by_email = "never"` refuses it (409
  `email_taken`), and the refusal is audited as `social.link_refused`; once the address is
  verified, the next sign-in with the provider links. A user that a social sign-in created has a
  verified address when the provider verified it, and connecting a provider from a signed-in
  session works as before.
- The app sign-in hand-over hands over only a sign-in that came back with the hand-over's own
  `attempt`, for every callback: a sign-in made in that browser for anything else while the
  hand-over waits ends it with `400 invalid_hand_over`. A browser that was already signed in when
  the hand-over began gets a small page of the framework's own first ("<app> wants to sign in as
  <person>", Continue or Cancel), answered with `POST /auth/app/confirm`; Cancel sends the app's
  callback `error=access_denied`. Opening `GET /auth/app/sign-in` no longer signs the browser out:
  for an app with a custom-scheme callback, the browser is signed out only once the person
  continues on that page. The `attempt` and the confirmation's value work once, also for requests
  sent at the same time. One client starts 30 hand-overs a minute (`429` after that).
- The session an app gets from `POST /auth/app/exchange` keeps the time the person last
  authenticated in the browser session the code came from. After a sign-in made for the hand-over
  it ends when a "remember me" sign-in made at that time would; after a browser that was already
  signed in was confirmed, it ends when that browser session was due to end as the hand-over
  finished, at the latest (signing the browser out afterwards does not end the app's session). A
  confirmed hand-over's code is refused once that browser session has ended. The exchange asks
  the sign-in policies about the new `SignInMethod::AppHandOver { platform, app }`, and a
  connection that enforces single sign-on lets it through. A code issued before the upgrade is
  refused; codes live a minute.
- With an `https` `app.url` set, an `https` app callback (`auth.apps.*.*.callback`) on another host
  fails the boot. A callback's scheme is read in any case, so `HTTPS://` is an `https` callback.
  With `auth.apps.callback_page = true` (off by default) the site answers `GET` on the path of
  each `https` callback with an empty page (`Cache-Control: no-store`,
  `Referrer-Policy: no-referrer`) for a browser the system did not hand to the app.
- `AuthService::confirm_action` approves its action only: it no longer records the confirmation as
  the session's `authenticated_at`, so `#[recent_auth]` routes and refreshed tokens keep the
  sign-in time they had. `POST /auth/confirm` and `POST /auth/confirm/passkey` still step up.
- A request authenticated with a bearer access token carries the token's session (`sid`) as the
  principal's `session_id`, so call tickets and push devices are bound to that session as they are
  for a browser's.
- Call tickets (`SealedTickets`) are sealed with a key derived from the application key for tickets
  alone, so no other value sealed with the application key opens as a ticket. Tickets issued
  before the upgrade no longer redeem; they live minutes.
- A page carries the session's CSRF token masked afresh in every answer
  (`navree_http::csrf::page_token`): Navree UI's forms and post form, the auth pages, Helm's pages
  and native screens write a different string each time, and the session's check takes the token
  as it is or any mask of it, in the `_csrf` field and in both headers. The `XSRF-TOKEN` cookie
  still holds the token as it is. A form whose method is `get` writes no token.
- The UI toolkit's brotli for pages and ops answers follows the same opt-outs as the HTTP
  pipeline's gzip: a route with `no_compression`, a response with `Cache-Control: no-transform` and
  `http.compression = false` leave the body uncompressed (`navree_http::may_compress` is the rule
  both read). A `Reply` whose handler says `no-transform` keeps it when it is answered as ops or
  as a native envelope.
- A form's `Submit` (and a table's bulk form, a move, a grid order) refuses a post that carries a
  cookie, sends neither `Sec-Fetch-Site` nor `Origin`, and has no CSRF token the session's check
  compared and took (`navree_http::csrf::TokenChecked`); the appearance route, which takes no
  token, refuses one with a cookie other than its own. A post with no cookie is still taken, and
  so is one whose token the session's check took, as a test client or a native app sends it. A
  form compares the `Origin` as the session layer does (host and port) and, when the request
  carries its own scheme, the scheme too. In an app with no session layer, a test that posts a
  form with a cookie adds `sec-fetch-site: same-origin` to its request, as a browser sends it:
  the test client does not add it.
- A form's file field takes a file key only from the person it was made for, for a file directly
  in the folder of its own `Uploads`, as long as the file is still within that upload's size and
  kind limits. The route a field's upload sends its file to answers with
  `Received::into_keyed_response_for(&signer, owner)`, where `UploadOwner` (an extractor) is the
  signed-in person's id and nothing for a guest. `file_key_for` and `file_path_for` make and read
  these keys; a key from `file_key` is no longer taken by a form's file field, and the person
  chooses the file again.
- A form's file field stores the first file posted under its name; a later part under the same
  name is not stored. A part of a chunked upload sent while its upload is being finished waits for
  that, and the finished file starts with the first part as it was checked.
- `TableQuery::without_columns` and `only_columns` (and the same two on `BulkRequest` and
  `ViewPost`) take columns away from the person asking, for one request. A column taken away is
  not shown, not in the column menu, not filtered on, sorted by or searched, not in a saved view
  that is applied, not in a row's cells and not in the CSV export; a full-text search with a
  searched column taken away reads the columns left as plain text. `#[col(hidden)]` still only
  says how a column starts: the docs now say so.
- A table's CSV export writes at most 100,000 rows (`DEFAULT_EXPORT_ROWS`) unless the query raises
  the limit with `TableQuery::export_limit`, up to `MAX_EXPORT_ROWS`. A result cut at the limit says
  so in `x-navree-truncated`, and a result of exactly the limit does not. An export always goes on
  from the position of its last row and never pages by `OFFSET`, so a sort by a column of a joined
  table writes the file in the order of the key. The docs say to throttle the route that serves it.
- A sortable's list lock and rank renewal can be scoped to the caller's own rows: a `ListScope` of
  columns that must hold the caller's workspace or board, given to `Move::transaction_scoped`,
  `sortable::transaction_scoped`, `lock_lists_scoped` and `renew_ranks_scoped`. A key of another
  scope is then a list that isn't there, a renewal leaves the ranks of a same-named list of
  another scope alone, and a scope whose value is `None` holds the rows whose column is NULL. The
  existing calls are unchanged and take no scope. The drag and drop page says to make list keys
  unique per workspace or to use the scoped forms.
- An owner keeps at most 50 saved views of one table (`MAX_SAVED_VIEWS`): `save_view` refuses
  another with `TooManyViews` and writes nothing, and `ViewPost::refused` answers the form with the
  reason. The data table page says that an app with workspaces puts the workspace into the owner
  string, and shows it.
- An `HtmlPolicy` that allows an attribute the page acts on (`data-ui-*`, `data-genui*`, `form`,
  `formaction`, `formmethod`, `formenctype`, `formtarget`, `popover`, `popovertarget`,
  `popovertargetaction`, `command`, `commandfor`, `autofocus`, `accesskey`, `contenteditable`) is
  refused at boot, and the sanitizer drops these from rich text whatever the policy says.
- The UI runtime acts on no hook inside an element marked `data-ui-stored`, and an op, a command
  or a button that names an id never finds an element there, nor submits through a form there.
  `Markdown` carries the mark, and so does a record's HTML in Helm; wrap rich text you show from
  the database in it too. A code block's copy button, in a part marked `data-ui-own`, still works
  inside.
- The page's post form (`ui-post`) posts only for a button the toolkit drew for it, which the
  renderer marks `data-ui-post`, and a button in stored content sends no form.
- Every request of the UI runtime goes through one check: an address on another origin is refused
  before anything is sent, and an answer redirected to another origin is a failure (a page loads
  its own address for real). A region, a lazy tab or a lazy sidebar group whose source is not a
  `LocalHref` (or is `#`) has no source: it is drawn once and never fetched, and a poll, feed or
  stream given with it is left out. A whole address of the site's own
  (`https://example.com/fragments/kpis`) is refused like any other, so write the path; a source
  that is a whole address, of this site or another, is named in the log as a warning, once,
  without its query. A region is polled once a second at the most, whatever its `poll` says.
- Every `View` answers with `X-Navree-Page: 1`, and the UI runtime takes an answer into the page
  only when it carries it (the build is checked as before): a page without it loads for real, and
  a region's answer without it is not drawn. A region's source answers with a `View`, or with
  `pages::region_answer(node)` for a part of a page alone; Helm's menu counts and Lighthouse's
  overview region answer that way.
- A boosted navigation fetched before a write, a sign-out or another tab's change is fetched again
  rather than shown (twice at the most; then a page loads for real, and a link that redraws its
  region alone, such as a table's sort, draws what came); a navigation that began before a
  form's post never replaces the form's answer; a region's fetch begun on one page
  never lands in the next page's region of the same id; and a page dropped as overtaken adds no
  history entry.
- A table's selection kept in the tab is cleared when another person signs in, a palette's recent
  items are kept for the person signed in, so the next person in the same browser starts with none,
  and a page restored from the back-forward cache for someone else loads again.
- `Href::parse`, and `LocalHref::parse` with it, refuses an address that starts with white space
  of any kind, a control character or an invisible format character (`\u{3000}`, `\u{a0}`,
  `\u{2028}`, `\u{85}`, `\u{200b}`, `\u{feff}` and the like), which some clients trim and others
  don't, so every client reads the same site in an address. It refused a plain space there
  already. After its first character an address is kept as it is written, as before.
- `.attr(..)` on `Select`, `Combobox` and `MultiSelect` refuses `style`, `srcdoc` and `on*` and
  checks URL attributes, as it does on any element.
- The host shown beside a link to another site, in rendered Markdown and in an assistant's
  `Sources`, is read the way a browser reads the address: a backslash is a slash, any run of
  slashes follows the scheme, and a user name, a password and a port are left out, so
  `https:/\other.example` shows `other.example`. A link that leaves the site with no host to read
  still counts as off-site and shows no host in Markdown; a source like it is shown as its title,
  not as a link.
- A chart picture served with `ChartImage::respond` answers `Cache-Control: private, no-cache`
  unless the route sets another with `.cache_control(...)`, so a shared cache keeps no copy of a
  picture of someone's own data. It was `no-cache`.
- An app with no configuration shows the "Development only" details of an error page only when
  the code turns them on with `PagesOptions::debug(true)`: a build with debug assertions alone no
  longer does, since such an app cannot say it is not production. An app with a configuration is
  unchanged: on in a debug build, off in a release build, always off in `production` and
  `staging`.

### Added

- `pages::region_answer(node)` answers a region with a part of a page alone, and
  `pages::region_asked(&headers)` reads the region a request names.
- Navree Native: the Android client's `NavreeConfig` gains `also`, `pictureHosts`, `webHosts`,
  `screen` (`hidden_in_recents`, `no_capture`) and `filterObscuredTouches`;
  `navree_notification_lock_screen` (`nothing`) keeps the notifications the library draws off a
  secure locked screen; the library leaves its web view folder out of backups and device
  transfers; the client hands an answer's `cache-control` to the core.
- Navree Native: `native::Answer::cache` sets the `Cache-Control` of a written screen, and
  `navree_native::KEEP_HEADER` and `PERSON_HEADER` name the headers `x-navree-keep` and
  `x-navree-person`. Every native answer from a signed-in session carries `x-navree-person`.

### Changed

- `@navree/nuxt` asks for Nuxt 4.6.4 or newer (`nuxt` peer `^4.6.4`, the module's own
  compatibility check too): 4.6.3 has two advisories against `@nuxt/nitro-server` and
  `@nuxt/schema` that 4.6.4 fixes.
- The UI runtime sends a region's id percent-encoded in the `x-navree-region` header, whatever the
  id. A handler that reads the header itself reads it with `pages::region_asked`.
- Social sign-in fetches a provider's key set with no lock held on the cached key sets: a token
  signed with a key the cached set has is verified without waiting for a fetch that another token
  started, and the tokens that need a fetch share one, still at most once every 30 seconds for a
  key id the set lacks. How long a key set is kept is unchanged.
- Navree Native: a native app keeps a page on the device between launches only when the page was
  read with GET, answered with success, and its `Cache-Control` does not say `no-store`. Pages are
  `no-store` by default, so nothing is kept unless a page allows it with `View::cache("private,
  no-cache")`, or `native::Answer::cache("private, no-cache")` for a written screen. What is kept
  belongs to the person it was answered for (by id) or to a visitor, and is shown only after the
  app's shell has answered and named that same person. An app whose sign-in does not give the
  framework its principal keeps nothing for signed-in people.

### Fixed

- A chart picture put in a page with `ChartImage::svg_node` marked the ids of its drawing with the
  picture's tag, which holds the toolkit's version: the same drawing got other ids, and the page
  other HTML, with every release. The ids now come from the drawing and its size alone. A picture's
  `ETag` still changes with a release, since its pixels may.
- A region whose id is not in Latin letters (Gurmukhi, Devanagari) can refresh: a browser refused
  to send such an id in a header as it was.
- A table names its own columns with their table (the key, the order, the filters, the search,
  the version column, a bulk request's keys), so a table over a join with another table that has
  a column of the same name is not ambiguous, on a page, in an export and in a bulk action.
  `TableQuery::apply`, which is given no database, still writes each column as its `source` says.
  An export sorted by a column that holds no value in some rows writes every row once.
- Navree Native: the runtime core reads an address as it is written and never trims or repairs it.
  An address with white space around it, a control character, a backslash, a name before its
  host, a host that is not ASCII, or a path that starts with `//` once its dot segments are taken
  out leads nowhere.
- Navree Native: every address a client is asked to fetch, to stream from or to show in the web
  view is a whole address on one of the app's origins.
  `crates/navree-native-core/vectors/addresses.json` holds the addresses every client must read
  the same way.
- Navree Native: an address opens outside the app only when it is a whole `http` or `https`
  address or a `mailto`, `tel`, `sms` or `geo` one, and a newer app is offered only at an `https`
  address.
- Navree Native: the id of a region or a list may be in any script. The core writes it into
  `x-navree-region` percent-encoded and the server reads it back; a plain ASCII id is unchanged.
- Navree Native: the Android client checks every address it is handed against the app's origins,
  uses modern TLS only, and caps answer bodies, event-stream lines and pictures. A request it
  cannot build is answered as a failed request, and an effect that fails is reported while the
  rest are carried out.
- Navree Native: the Android web view keeps its page on the app's origins and loads what a page
  holds only from them and from `NavreeConfig.webHosts`; where the system's web view supports it,
  a document of any other origin is emptied before it runs; a page taken off the origins is shown
  as a failed screen; the web view recovers when its process ends.
- Navree Native: signing out on Android removes, in order, the page cache, the launch snapshot,
  the web view's data, cache, history and form data, the app's notifications, the cookies and the
  push token; the cookie store's cookies go only to the app's own origins.
- Navree Native: an Android app that shrinks its code keeps the core's native methods and the
  protocol's serializers.
- Navree Native: what an app keeps between launches is emptied on any write, a new build and a
  change of person; an error, a redirect or an answer to a post removes the kept page of its
  address; after a page in the web view or a return to the front nothing kept is shown until the
  shell has answered again.
- Navree Native: a whole address leaves the core percent-encoded as a browser sends it; every
  picture source reaches a client whole, as a `data:image/` source, or empty; an address that does
  not read is reported by its kind, scheme and host.
- Navree Native: a sign-in whose return says the person refused, with the sign-in's own state,
  ends quietly; a return with another state changes nothing; the shell's sign-in callback is used
  only when it is the app's own `https` link or scheme.
- Navree Native: a client descriptor value that is not a token is written percent-encoded in
  `X-Navree-Client` and read back; plain values are unchanged.

## [1.2.0] - 2026-11-13

### Added

- Navree Native, a preview behind the `ui-native-preview` feature: a `ui` app's pages answer a
  native client as well as a browser. A request that sends `X-Navree-Native: 1` and describes
  itself in `X-Navree-Client` gets the page as a tree of parts (a screen with a title and actions
  by role, lists, tables, forms, sheets, charts) for the system's own toolkit to draw, and the
  app's shell at `/_navree/native/shell`. A browser's pages are the same bytes with the feature on
  and off. A page with a part that has no native form yet is shown in the client's web view, and
  the answer says which part.
- Words a page says what its parts are with, each a field that writes no HTML: `Button::role`
  (`Role`), `Button::presented` and `Item::presented` (`Presented`), `Button::undoable`,
  `Button::standard` (`Standard`), `Button::reason`, `AppPage::actions`, `Sheet::footer` and
  `Dialog::footer`, `DataTable::row_opens`, a column's `role` (`title`, `subtitle`, `status`,
  `trailing`) on `#[col]`, and `native::only` and `native::web_only` for a part of one side.
- Navree Native UI (`navree::native::ui`): a screen written once in Rust for native clients, with
  builders for every part and every control of a form. Structure the system places (`Screen`,
  `Section`, `List`, `Row`, `Form`, `Field`, `Sheet`, `Dialog`, `Region`), content it styles
  (`Figure`, `Ring`, `Rings`, `Chart`, `DayClock`, `Timeline`, `Progress`, `Facts`) and
  composition (`Stack`, `Grid`, `Card`, `Layers`, `Shape`, `Carousel`, `Adaptive`, `Text`,
  `Icon`) in the system's text styles, colours by name (`Hue`) and steps of its spacing.
  `navree::ui::native::Answer` answers one route with a page for a browser and a screen for a
  native client; `native::form` says a page's own form natively; `Reply::each` goes on with a
  reply for the client that asked; `Reply::signed_out` tells a native client to forget its
  session. A route with only a native side answers a browser with a small page and the status
  406.
- `navree native:coverage` grades every page route for a native client (`idiom`, `designed`,
  `layout`, `web`) and says what keeps a page from its system, with `--platform`, `--json` and
  `--fail-on` for a check in CI and a `native.toml` of example values; `navree native:doctor`
  checks an app's setup for native clients. `TestNative` drives a page as a native client in a
  plain test.
- The crate `navree-native`: the protocol's vocabulary, the client's descriptor, a JSON Schema,
  the example answers every client decodes, and the builders above. It is a preview and is used
  through `navree`.
- Signing in from native apps (`auth`): the association files Apple's and Android's systems ask
  for, from the apps listed under `auth.apps`; passkeys made in an Android app; a hand-over that
  signs an app in through the system's sign-in session (`GET /auth/app/sign-in`, then
  `POST /api/auth/app/exchange` with a PKCE proof); and `ConfirmAction`, which holds an action
  until the person confirms it with a passkey (`POST /api/auth/confirm/options`,
  `POST /api/auth/confirm/passkey`).
- Sign in with Apple and with Google from native apps (`social`): the app posts the identity
  token the system's own sheet gave it (`POST /api/auth/social/{provider}/native/nonce`, then
  `POST /api/auth/social/{provider}/native`), and the server signs the person in by the rules of
  the web callback. Native Sign in with Apple needs an Apple app under `auth.apps`, and native
  Sign in with Google an Android app there. Apple's server-to-server notifications are taken at
  `POST /api/auth/social/apple/notifications`. `Social::revoke_tokens` revokes a user's stored
  tokens at the provider, and offboarding a user does it too.
- Push to native apps (`notify`): a `push` channel through Apple's push service and Firebase
  Cloud Messaging, configured under `notify.push`. An app registers its device with
  `POST /api/notifications/push-devices` and removes it with `DELETE`; a device belongs to the
  session that registered it and gets nothing more once that session ends. The routes exist only
  where the app can push (`notify.push.apns` or `notify.push.fcm` configured, or a push driver
  registered), and a person keeps at most 20 devices.
- Agent skills for native apps: `navree-native`, `navree-native-design`, `navree-native-apple`
  and `navree-native-android`.
- `SortableGrid`, `GridTile` and `GridOrder` in `navree::ui`: a grid of tiles that people put in their
  own order by mouse, touch or keys, and take away. The page changes at once, and a moment after the
  last change the whole order is posted to your server, which checks and saves it.
- Charts drawn as PNG pictures on the server, for places that show only an image such as a
  home-screen widget, an email or a link preview: `ChartImage`, `Tile` and `Block` in
  `navree::ui::image`, with colours from the theme, light or dark, and the toolkit's typeface built
  in. It is behind the `image` feature of `navree-ui` and the `ui-image` feature of `navree`, off by
  default.

### Changed

- Disconnecting a provider (`Social::unlink`, `DELETE /api/auth/social/accounts/{provider}`) now
  revokes the stored tokens at the provider first. A provider that cannot be reached answers 502
  and leaves the account linked, to try again. A token the provider refuses for good is audited
  (`social.token_revocation_refused`) and forgotten, and the account is disconnected. An account
  with no stored tokens is disconnected as before.
- An app with an Apple app under `auth.apps` keeps Sign in with Apple's tokens (encrypted)
  whatever `social.store_tokens` says, because deleting an account must revoke them at Apple. An
  app with no Apple app there stores what it stored before.
- The `social` and `notify` modules each add a migration (the client a provider's token was issued
  to, and push devices). Run `navree migrate` after upgrading.
- The Rust library of `navree-cli` (the crate `navree_cli`) is not public API: the commands and
  options of `navree` stay compatible across 1.x, and the library's items, which the tool and its
  tests share, may change in any release. The versioning page says so.

### Fixed

- A small `Donut` drew the figure in its middle at one size whatever the ring's, so at
  `size(96)` a total of five characters reached the ring. The middle is now a box as wide as
  the hole and its words are sized against it: a donut of the default size and larger looks
  as before, and a smaller one, or one with a longer figure, draws it smaller. A figure too
  long for the hole at any size ends in an ellipsis.
- The `openapi.json` of the API and SaaS kits and the web kit's `web/lib/api/schema.d.ts` were
  behind the routes a new app serves since 1.1.0, so a new app's own CI check of its contract
  failed at once. They match now. In an app generated by 1.1.0, run
  `cargo run -- openapi:export openapi.json` and commit the file, and in a SaaS app run
  `pnpm --dir web gen:api` instead, which also writes the web app's types.

### Known limits

- Navree Native (preview): reading the deepest tree in a debug build on Windows. An answer may be up
  to 64 parts deep. Reading one at that limit with `serde_json` in a debug build on Windows takes
  between 1.75 and 2 MiB of stack, which is what a default thread has, so it can overflow there (it
  does with Rust 1.100). The pages of the example apps are 10 to 20 parts deep, a release build needs
  under 512 KiB, and a debug build on Linux is not affected. If a test of yours reads a tree that deep
  on Windows, give its thread 8 MiB. Making each level cheaper is planned.

## [1.1.0] - 2026-11-13

### Added

- Radial charts in `navree::ui::charts`: `Ring` (one value from twelve o'clock, with a second lap
  past its goal), `Rings` (up to six values inside each other), `Gauge` (an open arc on any scale,
  with zones) and `DayClock` (24 wedges by value, category and level, with moments and an hour
  band). Each has its table and summary, and a size per screen width.
- Linear charts: `Meter` (segments of a whole, with shares, a legend and detail lines),
  `MiniColumns` (one stacked column per item), `ColorStrip` (a cell per item in its category's or
  band's colour) and `RangeScale` (a value on a gradient track). `Trend` draws an arrow and a
  signed change, and a `Stat` can carry one.
- Grid charts: `CalendarHeatmap` (a day per cell in weeks, a five-step scale, month labels and a
  "Less … More" legend), `MatrixHeatmap` (a diverging scale over pairs, focusable cells),
  `Scatter` (translucent points, with an identity line) and `RelationList` (pairs ranked by a
  signed value).
- Options for every chart: gradients, theme inks and glow (`Gradient`, `Ink`); colour by value
  or threshold (`ColorScale`); motion that is off by default and off under reduced motion
  (`Motion`, `CountUp`); axis, gridline and tick switches and padded or fixed domains (`Axis`,
  `Gridlines`, `Ticks`, `Domain`); reference levels and bands (`Reference`, `Zone`); point dots,
  an active dot, step curves over named levels and big-value tooltips (`Dots`, `ActiveDot`,
  `Curve`, `TooltipStyle`); `Chart::baseline`, `Chart::lanes` and `NumberFormat::clock`.
  `Sparkline`, `Progress` and `Bars` take the same paint, and draw what they drew before when
  none is set. Each chart family links a stylesheet of its own.
- App blocks: `AppSheet`, `AppFont` and `AppImage` (an app's own stylesheet, font and picture,
  compiled in and served by content hash), a `Stat` that grows into a dashboard tile (icon,
  unit, status, a small chart beside or under the figure, a link), `InfoTip`, `ChartSkeleton`,
  `BreathingGuide`, `Dock` (with `Dock::below`), `DayPager`, `CopyField`, `Grid::ratio` and
  `Grid::fit`. They share one stylesheet and load no script.
- Marketing blocks for a public site, drawn on the server with no script: `SitePage`,
  `SiteHeader`, `Hero`, `Section`, `LogoCloud`, `FeatureGrid`, `FeatureRows`, `StatBand`,
  `BrowserFrame` and `PhoneFrame`, `Testimonials`, `Pricing`, `Faq`, `CtaBand`, `Newsletter`,
  `LegalPage` and `SiteFooter` with its theme toggle. `Picture` writes `srcset`, `sizes`, AVIF and
  WebP sources and light and dark versions.
- `View::seo` writes a page's description, robots, canonical and language links, social cards
  and JSON-LD (escaped so no text can close the block), and `Seo::problems` lists what a search
  result would hold against the page. `Sitemap` and `RobotsTxt` answer as files.
- `AuthPages` draws the sign-in pages in the app's look (its shell's theme, sheets, fonts and
  title template, or `appearance`, `stylesheet`, `font`, `aside` and `view` without one), on
  `AuthLayout`. `AuthPages::sign_up()` adds a sign-up page when `auth.registration` is on, and
  `AuthPages::after_sign_out` sets where a signed-out browser goes.
- `AppPage::topbar` puts a page's own part in the middle of the top bar, and
  `Shell::title_template` with `AppPage::document_title` titles the browser tab apart from the
  heading. `ThemeMenu` shows only the modes when one theme is on offer.
- `RouteDef::on_guest_or`: one address that answers a guest with one handler and a signed-in
  reader with another, such as a public home page and the app's home.
- `csrf::SameOriginOnly`: a module names a path whose posts are checked by their origin in place
  of the CSRF token. `Session::is_resumed` says whether the request came with the session's
  cookie.

### Changed

- Every form of `AuthPages` needs the session's cookie: a state-changing request to those pages
  without it is refused with a 403 before anything runs. A form left open until its session
  expired is drawn again with "Your session expired. Please try again.", keeping nothing that was
  typed.
- `POST /auth/register` takes five requests a minute per client, as the sign-up page does.
- navree-ui's appearance route checks the request's origin in place of the CSRF token, so a
  theme toggle on a page cached for everyone works for a reader with a session too, and it goes
  back only to a path of this site.
- An `on_guest_or` answer to a signed-in reader goes out as `Cache-Control: private, no-store`
  unless its handler kept it private, without the headers a CDN reads in place of
  `Cache-Control`, and with `Vary: Cookie, Authorization`.
- A `ColorScale` on a `Gauge` or a `Ring` reads its thresholds as values in the chart's own units,
  as every other chart does.
- A `RelationList` is a named group of list rows a screen reader walks, not a picture; a `Gauge`
  on a scale that starts above zero says in its summary and table when the value is below or above
  the scale; a list cut at 50 says so under `only_significant()` too.
- Warm chart colours keep their hue in light mode: gradients and vivid stops take a deeper end
  per theme (`--chart-N-vivid`), so orange and amber no longer mix toward brown.

### Fixed

- A region whose answer held a lazy part of its own (a lazy tab inside a lazy tab) replaced the
  whole page with that answer. A page the toolkit draws for a region's fetch is now marked as
  the region's fragment.
- A light and dark toggle, or a theme menu without its themes, turned a page of another theme
  into the default theme: the appearance cookie now keeps only what was chosen.

### Security

- Login CSRF from a sibling subdomain, or from a browser that doesn't say where it posts from,
  is closed by refusing a sign-in page post without the session's cookie. Every such refusal is
  logged on `navree::security` and audited as `csrf.mismatch`.
- A signed-in reader's page at an `on_guest_or` address is kept out of shared caches whatever its
  handler set. A CDN or proxy in front must still skip its cache for requests that carry the
  session cookie, since several ignore `Vary: Cookie`; the docs say so.
- The security docs describe a risk that remains: someone can register an account with another
  person's address before that person signs up, and the account keeps their password after the
  owner verifies it. Keep registration closed, or refuse password sign-in for an unverified
  address with a sign-in policy, until a setting for it ships.

## [1.0.2] - 2026-11-12

### Security

- Navigator's `query_database` and Lighthouse's tools could be made to run more than the one
  read-only statement they checked, and so to change the development database or write files:
  through hex strings (`X'..'`), custom type modifiers and, on SQLite, variables (`$a::..`), which
  the guard and the database read differently; on PostgreSQL through functions called in `FROM`
  (`pg_read_file`, `dblink`, `pg_terminate_backend`) and functions that run SQL text of their own
  (`ts_stat`, `query_to_xml`); and on a PostgreSQL server with `standard_conforming_strings` off,
  through a backslash before a quote. The database engine now holds the line. On SQLite the file
  is opened read-only, SQLite itself prepares the text and refuses anything after the first
  statement, refuses a statement it says writes (which covers `VACUUM`), and an authorizer allows
  only reading (no `PRAGMA` but the `data_version` FTS5 reads, no `ATTACH`, no transactions). On
  PostgreSQL each query is a single prepared statement in a read-only transaction, in a session
  that pins `standard_conforming_strings = on`, checks it, and pins it again inside the
  transaction (behind a pooler in transaction mode another client's `SET` could otherwise reach
  it). The statement guard also refuses those literals, types, parameters and functions, sends a
  statement only when it reads back unchanged, and refuses statements over 64 KB or nested over
  1,000 expressions deep, which could crash Navigator.
- On PostgreSQL, `query_database` and Lighthouse's tools now refuse to run as a superuser, as a
  role that can become one (a member of a superuser role, or a superuser session with a lesser
  `role`), or as a member of `pg_read_server_files`, `pg_write_server_files`,
  `pg_execute_server_program`, `pg_signal_backend` or `pg_signal_autovacuum_worker`, since such a
  role reaches the server's files, programs and other sessions from a query. They also refuse a
  role that may execute, whatever its privilege on the schema, a `SECURITY DEFINER` function of
  the database's own (not an extension's), a function in an untrusted language (`plpython3u`,
  `plperlu`, `pltclu`, or C outside an extension), or a function of an extension that reaches
  files, programs, other servers or the network (any extension with a foreign-data wrapper,
  `dblink`, `adminpack`, `http`, `pg_net`, `pg_background`, `pg_cron`, `pgagent`, `plproxy`,
  `orafce`, `aws_s3`, `aws_lambda`, `aws_commons`, `azure_storage`, `azure_ai`,
  `google_ml_integration`, `pgml`); the support functions of an aggregate it may execute count
  too (PostgreSQL checks `EXECUTE` on them against the aggregate's owner), and so does a foreign
  table it may `SELECT` from, directly or through views. The refusal names what it found. What
  an ordinary role can still reach is what the other extensions installed in the database give
  it (their C functions, and their own `SECURITY DEFINER` functions: PostGIS, pgcrypto,
  TimescaleDB, pgvector), a table read by a view it may read (the view reads its tables with
  its owner's rights; a foreign table there is refused, as above), and any extension Navigator
  doesn't know that adds a function reaching the network or a file.
- A query could make Navigator hold gigabytes: `printf('%.*c', 400000000, 'x')`, a recursive
  `zeroblob(..)` or `json_group_array` on SQLite, `repeat('x', 900000000)` or an `EXPLAIN` plan
  on PostgreSQL. SQLite now refuses to make a string or blob over 1 MB, and `navree mcp` caps
  SQLite's heap at 256 MB for its process (code that embeds Navigator sets its own cap with
  `database::limit_sqlite_heap`). PostgreSQL cuts each value to 4 KB before sending it (its own
  work ends with the 5 second timeout), and `EXPLAIN` is refused there. A query may answer at
  most 100 columns; every value over 4 KB comes back cut and ending with `… [cut]`, and an
  answer stops at about 1 MB of JSON, with `truncated` set (a single row that large keeps its
  first values and cuts the rest). Error messages are cut to 2 KB in what the tools return
  (PostgreSQL still sends the whole message, up to its 1 GB limit, and Navigator reads it once
  before cutting it). Lighthouse's tools read their own tables whole, up to 8 MB a value, so
  histograms and backtraces stay complete.
- `describe_schema` on PostgreSQL could list every table instead of the one asked for, given a
  table name with a backslash and a quote on a server with `standard_conforming_strings` off.
- The pgvector store wrote the field name of a comparison filter (`Filter::gt`, `gte`, `lt`,
  `lte`) into its SQL unescaped. Field names are bound parameters now.
- The UI runtime refuses a `data-ui-form` whose action is on another origin before sending it,
  and applies nothing from an answer that was redirected to another origin. One case is left:
  the browser follows a redirect before the runtime sees it, so on a page served without
  Navree's content security policy, a same-origin action that redirects to another origin
  (an open redirect in the app) sends the request there, with the session's `x-xsrf-token`
  header and, after a 307 or 308, the form's body, if that origin answers the browser's
  preflight. Navree's policy (`connect-src 'self'`) stops that request in the browser.

### Added

- `navree dock up` keeps a read-only role, `navree_reader`, in the PostgreSQL database, for
  Navigator. When Dock creates it, it may connect, use `public` and read its tables, now and
  later; Dock never grants it anything again, so a `REVOKE` stays. On every `up` it is made a
  plain login role whose sessions start read-only with a 5 second statement timeout (role
  defaults a session may change) and may write at most 64 MB of temporary files. Its password
  is its own, derived from the service's, and Dock sends the server only its SCRAM verifier.
  Like any role, it also has what PostgreSQL gives `PUBLIC`: `CONNECT`, `TEMP`, and `EXECUTE` on
  functions, extension C functions included.
- `NAVREE_MCP_DATABASE_URL`: the database `navree mcp` reads instead of the app's
  `database.url`, such as the same database through a read-only role.
- `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`: lets `navree mcp` query as a refused
  PostgreSQL role, with a warning each time it starts.
- `navree_navigator::DatabaseOptions`, `Navigator::with_database` and `run_stdio_with`, which
  carry those two settings for code that embeds Navigator, and
  `navree_navigator::database::limit_sqlite_heap` with `SQLITE_HEAP_LIMIT`, `MAX_VALUE_BYTES`,
  `MAX_ANSWER_BYTES`, `MAX_COLUMNS`, `MAX_MESSAGE_BYTES` and `CUT_MARK`.

### Changed

- Upgrading with Dock: run `navree dock up` once. It creates `navree_reader` in your running
  database (nothing is restarted), and from then on Navigator's `query_database`,
  `describe_schema` and Lighthouse's tools read the Dock database as that role. Until you do,
  they fail to connect and say that `navree dock up` creates the role. Dock's own user,
  `navree`, is a superuser, which `query_database` now refuses.
- Upgrading without Dock: on PostgreSQL, `navree mcp`'s `query_database` and Lighthouse's tools
  refuse a privileged role, and a development database often connects as a superuser. Create a
  read-only role (the refusal prints the SQL; the Navigator docs have it too) and set
  `NAVREE_MCP_DATABASE_URL` to its URL in your agent's MCP configuration, or set
  `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1` to keep querying as before. A role that may
  execute `SECURITY DEFINER`, untrusted-language or network and file extension functions is
  refused too: revoke `EXECUTE` on the functions the refusal names. `describe_schema` is not
  affected.
- On SQLite, `query_database` no longer reads pragma table functions (`pragma_table_info`):
  `describe_schema` answers what they would. R*Tree tables can't be queried through
  `query_database` either: opening one prepares a write. FTS5 tables can.
- On PostgreSQL, `query_database` refuses `EXPLAIN`: the plan holds the query's constants at any
  size. Run it in `psql`, or ask `describe_schema` about the tables and indexes. SQLite's
  `EXPLAIN QUERY PLAN` still works.
- Dock refuses a `dock.toml` whose PostgreSQL `user` is `navree_reader`.
- Navigator's docs and tool descriptions say that hiding columns with secret-looking names in
  query results is a convenience, not a protection: a query can rename or wrap a column.

## [1.0.1] - 2026-11-11

### Fixed

- `navree dev` on Linux restarted the server again and again with nothing edited: it took a file
  being opened or read (the server reading `.env` as it starts, `cargo build` reading `Cargo.toml`)
  for an edit. Only edits start a rebuild now, and a server busy writing `storage/` no longer delays one.

### Security

- The development install of `@navree/nuxt` takes node-forge 1.4.3, for the four advisories fixed in it.
  Only Nuxt's development HTTPS certificates use it; the published package does not depend on it.
- The fuzzing workspace takes rustix 1.1.6, for RUSTSEC-2026-0486; the main workspace already had it.

### Changed

- Dependencies: the minor and patch updates of the Rust crates and of the front-end packages.

## [1.0.0] - 2026-11-09

First release. Read what it includes in the [release notes](https://navree.com/docs/prologue/release-notes/).

[Unreleased]: https://github.com/NavreeHQ/navree/compare/v1.2.0...HEAD
[1.2.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.2.0
[1.1.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.1.0
[1.0.2]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.2
[1.0.1]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.1
[1.0.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.0
