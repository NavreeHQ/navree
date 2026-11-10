//! The SCIM server driven by a SCIM client Navree did not write: the
//! `scim2` command of scim2-cli (Python, `pip install scim2-cli`), whose `test`
//! command reads the server's discovery documents and then runs a compliance
//! check as a provisioning client does: creates, reads, searches, replaces,
//! patches and deletes users and groups with values it generates, and reports
//! every deviation from RFC 7643 and RFC 7644.
//!
//! One report of the client is wrong and does not count: it requires a
//! group member's read-only `display` and a randomly picked `type` to come
//! back as sent (see `Check::is_a_false_report`). Every other result must be
//! a success.
//!
//! The other SCIM tests send the requests Microsoft Entra ID and Okta are
//! known to send, written from their documentation. This one sends what an
//! independent implementation of the standard sends. (Entra ID's own SCIM
//! validator and Okta's SCIM tester are hosted tools that need a public
//! address and an account: they stay the owner's to run.)
//!
//! This runs in the integration workflow, which installs scim2-cli and sets
//! `NAVREE_TEST_SCIM2_CLI` to the command; it is `#[ignore]`d in the other
//! suites. Run it with `-- --ignored`. A missing variable fails the test when
//! `NAVREE_TEST_REQUIRE_SERVICES=1` and prints `SKIPPED` otherwise.

use std::process::Command;

use navree_directory::Owner;

use crate::support::harness;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs scim2-cli, installed by the integrations workflow"]
async fn a_scim_compliance_client_finds_no_deviation() {
    let test = "a_scim_compliance_client_finds_no_deviation";
    let cli = match std::env::var("NAVREE_TEST_SCIM2_CLI") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_SCIM2_CLI is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set NAVREE_TEST_SCIM2_CLI");
            return;
        }
    };

    // The address first: the `location` of every resource the server writes
    // is built from it, and the client follows those.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    let h = harness(|builder| {
        builder
            .config("app.url", format!("http://127.0.0.1:{port}"))
            // A compliance run sends hundreds of requests from one address.
            .config("http.per_ip_rate_limit", serde_json::Value::Null)
    })
    .await;
    let token = h
        .scim(Owner::App)
        .await
        .scim_token
        .expect("a SCIM token")
        .expose()
        .clone();
    let router = navree_http::build_router(h.app.ctx())
        .await
        .expect("the router");
    let server = tokio::spawn(async move {
        let _ = navree_http::serve(listener, router).await;
    });

    let url = format!("http://127.0.0.1:{port}/scim/v2");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&cli)
            .arg("--url")
            .arg(&url)
            .arg("--header")
            .arg(format!("Authorization: Bearer {token}"))
            .args(["test", "-v"])
            .output()
    })
    .await
    .expect("the task ends")
    .expect("scim2 starts (pip install scim2-cli)");
    server.abort();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let checks = checks(&stdout);
    let deviations: Vec<&Check> = checks
        .iter()
        .filter(|check| check.status != "SUCCESS" && !check.is_a_false_report())
        .collect();
    assert!(
        deviations.is_empty(),
        "scim2 test reported deviations ({}):\n{stdout}\n{stderr}",
        output.status
    );
    // The client ran: its exit status is failure only for the false reports
    // above, and it checked the whole surface (a run of a few checks would
    // pass anything).
    let passed = checks
        .iter()
        .filter(|check| check.status == "SUCCESS")
        .count();
    assert!(passed >= 50, "{passed} checks passed:\n{stdout}\n{stderr}");
    assert!(
        output.status.success() || checks.iter().any(Check::is_a_false_report),
        "scim2 failed without a report ({}):\n{stdout}\n{stderr}",
        output.status
    );
}

/// One result the client prints: a status line such as `ERROR
/// check_add_attribute`, and the lines under it up to the next result.
struct Check<'a> {
    status: &'a str,
    title: &'a str,
    lines: Vec<&'a str>,
}

fn checks(stdout: &str) -> Vec<Check<'_>> {
    let mut checks: Vec<Check> = Vec::new();
    for line in stdout.lines() {
        let mut words = line.split(' ');
        match (words.next(), words.next(), words.next()) {
            (Some(status), Some(title), None)
                if !status.is_empty()
                    && status.chars().all(|c| c.is_ascii_uppercase())
                    && title.chars().all(|c| c.is_ascii_lowercase() || c == '_') =>
            {
                checks.push(Check {
                    status,
                    title,
                    lines: Vec::new(),
                });
            }
            _ => {
                if let Some(check) = checks.last_mut() {
                    check.lines.push(line);
                }
            }
        }
    }
    checks
}

impl Check<'_> {
    /// Whether this is the one report of the client that is wrong, and
    /// stays out of the pass criterion.
    ///
    /// `check_add_attribute` and `check_replace_attribute` PATCH a group's
    /// `members` with a value the client generates: a real user's `value` and
    /// `$ref`, a `display` that is a random string, and a `type` picked at
    /// random from `User` and `Group`. They then require the server to
    /// return that value unchanged. The two attributes cannot come back as
    /// sent:
    ///
    /// - `display` is read-only (RFC 7643 §2.4: "A human-readable name,
    ///   primarily used for display purposes. READ-ONLY."), so a server
    ///   shows the member's own name, not what a client sent.
    /// - `type` is the kind of resource the member is (RFC 7643 §4.2). The
    ///   client picks it without looking at the resource `value` names, so
    ///   half of the time it says `Group` of a user whose `$ref` is under
    ///   `/Users/`; the server answers `User`.
    ///
    /// The member itself, `value` and `$ref`, must come back as sent.
    fn is_a_false_report(&self) -> bool {
        if self.status != "ERROR"
            || !matches!(
                self.title,
                "check_add_attribute" | "check_replace_attribute"
            )
        {
            return false;
        }
        let line = |prefix: &str| self.lines.iter().find_map(|line| line.strip_prefix(prefix));
        let (Some(sent), Some(returned)) = (line("Patched value: "), line("Returned value: "))
        else {
            return false;
        };
        let members = members_of(sent);
        !members.is_empty()
            && members == members_of(returned)
            && self
                .lines
                .first()
                .is_some_and(|reason| reason.contains("returned unexpected value for 'members'"))
    }
}

/// The `(value, ref)` of every `GroupMember(...)` in a printed list.
fn members_of(printed: &str) -> Vec<(&str, &str)> {
    printed
        .split("GroupMember(")
        .skip(1)
        .filter_map(|member| Some((quoted(member, "value=")?, quoted(member, "ref=")?)))
        .collect()
}

/// The text of `name='...'` in a printed member.
fn quoted<'a>(member: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}'");
    let rest = &member[member.find(&marker)? + marker.len()..];
    Some(&rest[..rest.find('\'')?])
}

#[cfg(test)]
mod tests {
    use super::checks;

    const REASON: &str = "PATCH modify() returned unexpected value for 'members'.";

    fn member(value: &str, kind: &str, display: &str) -> String {
        format!(
            "[GroupMember(value='{value}', ref='http://h/scim/v2/Users/{value}', type='{kind}', display='{display}')]"
        )
    }

    /// What the client prints around one failed check.
    fn report(title: &str, reason: &str, sent: &str, returned: &str) -> String {
        format!(
            "SUCCESS object_creation\n  Successfully created\nERROR {title}\n  {reason}\nPatched value: {sent}\nReturned value: {returned}\n  {{'urn': 'members'}}\nSUCCESS check_add_attribute\n  Successfully added attribute 'title'\n"
        )
    }

    #[test]
    fn only_a_member_that_differs_in_display_and_type_is_a_false_report() {
        let sent = member("u1", "Group", "a random string");
        let returned = member("u1", "User", "Anu Lakhotia");
        let text = report("check_add_attribute", REASON, &sent, &returned);
        let parsed = checks(&text);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].status, "ERROR");
        assert_eq!(parsed[1].title, "check_add_attribute");
        assert!(parsed[1].is_a_false_report());
        assert!(!parsed[0].is_a_false_report() && !parsed[2].is_a_false_report());
        let replaced = report("check_replace_attribute", REASON, &sent, &returned);
        assert!(checks(&replaced)[1].is_a_false_report());

        // Another member, another attribute and another check are deviations.
        let other = member("u2", "User", "Anu Lakhotia");
        assert!(
            !checks(&report("check_add_attribute", REASON, &sent, &other))[1].is_a_false_report()
        );
        let emails = "PATCH modify() returned unexpected value for 'emails'.";
        assert!(
            !checks(&report("check_add_attribute", emails, &sent, &returned))[1]
                .is_a_false_report()
        );
        let removal = report("check_remove_attribute", REASON, &sent, &returned);
        assert!(!checks(&removal)[1].is_a_false_report());
    }
}
