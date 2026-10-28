# Supply-chain review (cargo-vet)

Every third-party crate in `Cargo.lock` must be either **audited** (by us or
by an organisation we import) or **exempted** (listed in `config.toml` as
not yet reviewed). CI runs `cargo vet --locked` on every pull request, on
pushes to main and daily (`.github/workflows/supply-chain.yml`); it fails when
a crate is neither. `cargo deny check` (see `deny.toml` at the root) covers
advisories, licenses, banned crates and sources alongside it.

Files here:

- `config.toml`: the imported audit sets (Mozilla, Google, Bytecode Alliance,
  ISRG, Zcash, Embark) and the exemptions.
- `audits.toml`: audits we wrote ourselves.
- `imports.lock`: the pinned copy of the imported audits, so a run does not
  depend on what those organisations publish that day. Do not edit it by hand.

Tool: `cargo install --locked cargo-vet`. Comments in these files are dropped
by `cargo vet fmt`, so notes live here.

- The exemptions are crates nobody we import has audited. They are debt, not
  approvals; do not add one by hand just to get CI green.
- `docx-rs` is a pinned commit of `bokuweb/docx-rs` (a git dependency); the
  `[policy.docx-rs] audit-as-crates-io = true` entry in `config.toml` makes
  cargo-vet treat it as the crates.io release of the same name.

## Adding or updating a dependency

1. Add the dependency and update `Cargo.lock` in the pull request.
2. Run `cargo deny check` and `cargo vet`. If both pass, you are done.
3. If `cargo vet` reports unvetted crates, **look at them before exempting
   them**:
   - `cargo vet suggest` lists what is missing and who already audited a
     nearby version.
   - `cargo vet diff <crate> <old> <new>` shows what changed since an audited
     version; `cargo vet inspect <crate> <version>` shows a whole crate.
   - If an imported audit covers a nearby version, a diff review is enough.
4. Review for the `safe-to-deploy` criteria: no unexpected network, file or
   process access; no `build.rs` or proc macro that does more than build;
   `unsafe` that is sound and explained; no obfuscated code; the maintainer
   and repository match what the crate claims. Pay extra attention to crates
   with one maintainer, a recent ownership change, or a `build.rs`.
5. Record the result:
   - Reviewed and fine: `cargo vet certify <crate> <version>` (adds to
     `audits.toml` with your notes). Use `--criteria safe-to-run` for
     dev-only crates that never ship.
   - Not reviewed yet and the risk is acceptable for now: `cargo vet
     add-exemption <crate> <version>`, and say why in the pull request.
     Exemptions are debt; the aim is for the list to shrink.
6. Commit `supply-chain/` with the dependency change. Changes under
   `supply-chain/` need a review from the code owners (`.github/CODEOWNERS`).

Dependabot bumps go through the same steps: a bump that changes a crate's
version needs `cargo vet` to pass, usually through a diff audit
(`cargo vet diff`) or by refreshing the exemption for the new version
(`cargo vet regenerate exemptions` drops entries the audits now cover and
rewrites the rest to match `Cargo.lock`).

## Refreshing the imported audits

`cargo vet` (without `--locked`) pulls the latest imported audits and updates
`imports.lock`. Do this now and then in its own pull request, followed by
`cargo vet regenerate exemptions` to drop exemptions that are now covered.

## Reducing the exemptions

Start with crates that run in production and have no imported audit, ordered
by `cargo vet suggest`. Small crates that ship in the binaries are the best
place to start.
