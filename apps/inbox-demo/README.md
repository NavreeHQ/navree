# inbox-demo

An example app on Navree UI: the support desk of an invented software company in Mohali, whose customers are clinics, studios and academies across India. Three panes you resize (the conversations, the open conversation and its customer), new messages live over server-sent events, and a desk of 100,000 conversations and about 5.5 million messages, read as it is read.

```sh
cargo run --release -p inbox-demo          # http://127.0.0.1:8096/inbox, 100,000 conversations
INBOX_SCALE=2000 cargo run -p inbox-demo   # a small desk, quicker to start
INBOX_SIMULATOR=off cargo run -p inbox-demo
```

Press `?` in the app for the keys. Each agent can turn single keys off from that dialog.

## No sign-in

The example has no sign-in. A visitor works as one of the team, named by a cookie the server signs, and can switch to another from the account menu. The signature only stops a visitor naming someone they didn't choose in the app; it is not a sign-in.

A real app puts the inbox behind [navree-auth](https://navree.com/docs/security/authentication/) and checks, on every route, that the person may see and change the conversation it names.

## What to read

- `src/live.rs`: every write in one transaction, with its revision and its ids taken where it commits, so a live reader can't skip one.
- `src/queries.rs`: a page by keyset, what changed since a revision, a window of messages.
- `src/views/`: the panes, the list, the thread and the customer, as plain functions of the data.
- `e2e/ui/tests/inbox.test.mjs` and `inbox-scale.test.mjs` in the repository: the app in Chromium, Firefox and WebKit, and measured at full size.
