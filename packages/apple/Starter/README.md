# Dukaan for iPhone, iPad and Mac

The Apple client of Dukaan: its screens come from http://127.0.0.1:8095 and are drawn natively, by the Navree Native client in `..`. Written by `navree native:init`.

## Run it

1. XcodeGen, once: `brew install xcodegen`.
2. In this folder: `xcodegen generate`, then `open Dukaan.xcodeproj`.
3. Run the scheme `Dukaan` in an iPhone or iPad simulator, or `DukaanMac` on this Mac.

The app talks to http://127.0.0.1:8095: plain http to this machine, for development, so start the server there first (`navree dev`). A build for a device talks to the site over https: change `NavreeOrigin` in `project.yml`, for both apps.

The server draws its pages for the app when its `navree` dependency has the native preview on: `navree native:doctor --platform ios` says what the server is missing, and `navree native:coverage --platform ios` how each page is drawn.

## Signing in

The app hands a sign-in to the system's browser, and the server hands it back to the app. The server names the app in `config/app.toml`:

```toml
[auth.apps]
# The site answers the callback link with an empty page in a browser.
callback_page = true

[auth.apps.apple.dukaan]
team_id = "ABCDE12345"
bundle_id = "com.navree.dukaan"
# Back on a link of the site, which no other app can claim.
callback = "https://shop.example/app/signed-in"
```

That link needs both apps signed by a team with Associated Domains (below). A build with no team can come back only on the app's own scheme, `callback = "dukaan:/signed-in"` with `custom_scheme = true`; any app can claim a scheme, so the site then asks the person, on a page that names the app, before the sign-in leaves for it. Either way the sign-in ends at `/auth/app/finish`, which only the browser's own navigation finishes: Navree's sign-in pages send the browser there themselves, and a sign-in page of your own that signs a person in by script does the same (`location.assign(next)`).

The app opens the system's browser for the pages in `ShellConfiguration.signInPaths`, by default a social sign-in's only (`/auth/*/redirect`). For the site's own sign-in page as well, add `signInPaths: ["/sign-in", "/auth/*/redirect"]` to the `ShellConfiguration` in `Dukaan/App.swift`.

## Signing

`Signing/Signing.xcconfig` signs both apps with the bundle id `com.navree.dukaan`, and no entitlements. Sign in with Apple, passkeys, push and a sign-in that comes back on a link of the site need a team: copy `Signing/Signing.local.xcconfig.example` to `Signing/Signing.local.xcconfig`, fill it in (it says what to register with Apple and on the server), and run `xcodegen generate` again. The iPhone and iPad app and the Mac app share the bundle id, so one App ID and one `auth.apps.apple` entry on the server cover both.

## The client

`project.yml` takes the client from `..`: a Navree checkout's `packages/apple`, with its core built by `scripts/build-core.sh`. An app for people takes it from the package instead: `url: https://github.com/NavreeHQ/navree-swift` and `minorVersion:` the version of Navree the server runs.
Its runtime core is built for Apple silicon only: the Mac app is built for `arm64`, and a simulator build leaves out `x86_64`.

`Dukaan/App.swift` is the whole app: a `ShellConfiguration` with the origin, and `NavreeScenes`. What else it can set (other hosts a web page loads from, covering the app in the app switcher, opening at login on a Mac) is in the client's documentation.
