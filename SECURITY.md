# Security policy

## Supported versions

Only the **latest 1.x release** receives security fixes. Fixes are published as a new patch release of that line;
older releases are not patched, so upgrade to the latest release to pick them up.
When more than one release line is supported, this section will list them.

## Reporting a vulnerability

Email **security@navree.com**. Never open a public issue, pull request or
discussion for a suspected vulnerability, and do not post details anywhere
public before a fix is released.

You can also use GitHub's private vulnerability reporting (the repository's
Security tab, "Report a vulnerability") once it is enabled.

Please include:

- the affected crate and version (or commit),
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

When a fix is ready, we publish a patch release and, at the same time, a
[GitHub Security Advisory](https://github.com/NavreeHQ/navree/security/advisories)
on the repository with the affected and patched versions, a CVE where one
applies, and credit to the reporter. For issues in published crates we also
submit the advisory to the [RustSec advisory database](https://github.com/rustsec/advisory-db)
so `cargo audit`, `cargo deny` and OSV-Scanner pick it up. Details stay private
until the patched release is available.

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
with a reason. The reports are kept as workflow artifacts. See "Static
analysis" in [`CONTRIBUTING.md`](CONTRIBUTING.md) for what the rules check, how
to run them locally and how to suppress a false positive.
