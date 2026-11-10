# Security policy

## Supported versions

Every crate and every `@navree/*` package shares one version number, so a
line is supported as a whole.

| Line | Bug fixes | Security fixes |
|---|---|---|
| The current major (1.x until 2.0 ships) | the latest minor | the latest minor |
| The previous major, after the next one ships | 6 months | 12 months |
| Long-term support | none at launch | none at launch |

To get a fix on the current major, upgrade to the latest minor: minors keep
the public API, so an upgrade is safe. Fixes are published as a patch release.
When a fix applies to more than one supported line, the patch releases for
every line go out together, within the same hour. The
[versioning and support](https://navree.com/docs/prologue/versioning-and-support/)
page has the details.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: open this repository's
**Security** tab and choose **Report a vulnerability**. Or email
**security@navree.com**. Never open a public issue, pull request or
discussion for a suspected vulnerability, and do not post details anywhere
public before a fix is released.

Please include:

- the affected crate or package and version (or commit),
- what an attacker can do and under what conditions,
- a minimal reproduction or proof of concept,
- whether you plan to disclose publicly, and when.

## What to expect

| Step | Target |
|---|---|
| Acknowledgement of your report | within 2 business days |
| Fix for a **critical** issue | within 24 hours of confirmation |
| Fix for a **high** issue | within 72 hours of confirmation |
| Fix for medium and low issues | in the next regular release |

Severity follows CVSS v4 and is set during triage; we tell you the rating and
the reasoning. We keep you updated on progress and credit you in the advisory
unless you ask us not to.

## How advisories are published

When a fix is ready, we publish the patch releases and, at the same time:

- a [GitHub Security Advisory](https://github.com/NavreeHQ/navree/security/advisories)
  with the affected and patched versions and credit to the reporter, and a CVE
  where one applies;
- for the crates, an entry in the
  [RustSec advisory database](https://github.com/rustsec/advisory-db), so
  `cargo audit`, `cargo deny` and OSV-Scanner pick it up;
- for the `@navree/*` npm packages, the same GitHub advisory, which npm audit
  reads.

Details stay private until the patched releases are available. We do not yank
a release for a vulnerability by default: the advisory warns users, and the
fix is the next patch.

## Supply chain

Dependencies are gated in CI with cargo-deny (advisories, licenses, bans and
sources), cargo-vet (reviewed or exempted crates) and a daily OSV scan; every
GitHub Action is pinned to a commit SHA, and each release carries a CycloneDX
SBOM. See [`supply-chain/README.md`](supply-chain/README.md) and the
"Dependencies and security" section of [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Static analysis

Every push to `main`, every pull request and a weekly run scan the code with
Semgrep (the registry rulesets for Rust, TypeScript, JavaScript, React, GitHub
Actions and Dockerfiles, plus Navree's own rules in [`.semgrep/`](.semgrep/))
and the git history and working tree with Gitleaks. A finding fails the build;
false positives are suppressed at the line or in [`.gitleaks.toml`](.gitleaks.toml)
with a reason. The reports go to code scanning and are kept as workflow
artifacts. See "Static
analysis" in [`CONTRIBUTING.md`](CONTRIBUTING.md) for what the rules check, how
to run them locally and how to suppress a false positive.
