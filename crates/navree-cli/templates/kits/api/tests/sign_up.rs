//! Sign-up, email verification, the first workspace and joining one by
//! invitation, over the API as a client does it.

mod support;

use navree::workspace::WorkspaceRole;
use serde_json::{Value, json};
use support::{boot, first_url, in_workspace, sent_mail, sign_in, user, workspace};

#[tokio::test]
async fn people_sign_up_verify_their_address_and_create_a_workspace() {
    let (app, _guard) = boot().await;
    let client = app.client();

    // Sign up: the account and a token pair.
    let body = client
        .post("/api/auth/register")
        .json(&json!({
            "name": "Anu Lakhotia",
            "email": "anu@example.com",
            "password": "a long enough password",
        }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let token = body["access_token"].as_str().expect("a token").to_owned();
    assert_eq!(body["user"]["email_verified_at"], Value::Null);

    // The verification mail's signed link verifies the address.
    client
        .post("/api/auth/email/verification")
        .bearer(&token)
        .send()
        .await
        .assert_status(202);
    let text = sent_mail(&app)
        .await
        .last()
        .and_then(|mail| mail.text.clone())
        .expect("a verification mail");
    let link = first_url(&text).expect("a link in the mail");
    let path = &link[link.find("/api/").expect("a link to the API")..];
    client.get(path).send().await.assert_status(200);
    let me = client
        .get("/api/auth/me")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(me["email_verified_at"].is_string(), "{me}");

    // No workspace yet: the first one is theirs.
    client
        .get("/api/workspaces")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data", json!([]));
    let created = client
        .post("/api/workspaces")
        .bearer(&token)
        .json(&json!({ "name": "Analytical Engines" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("role", json!("owner"))
        .json_value();
    let slug = created["slug"].as_str().expect("a slug").to_owned();
    assert_eq!(slug, "analytical-engines");
    in_workspace(app.client().get("/api/v1/projects"), &token, &slug)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(0));
}

#[tokio::test]
async fn sign_up_checks_its_input() {
    let (app, _guard) = boot().await;
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Anu", "email": "not an address", "password": "a long enough password" }))
        .send()
        .await
        .assert_validation_error("email");
}

#[tokio::test]
async fn invited_people_join_with_their_role() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Agra").await;
    let owner_token = sign_in(&app, "olivia@example.com").await;

    app.client()
        .post(&format!("/api/workspaces/{}/invitations", team.slug))
        .bearer(&owner_token)
        .json(&json!({ "email": "gauri@example.com", "role": "member" }))
        .send()
        .await
        .assert_status(201);
    let text = sent_mail(&app)
        .await
        .last()
        .and_then(|mail| mail.text.clone())
        .expect("an invitation mail");
    let token = invitation_token(&text);

    // Gauri signs up with the invited address and accepts.
    user(&app, "Gauri Hundal", "gauri@example.com").await;
    let grace = sign_in(&app, "gauri@example.com").await;
    app.client()
        .post("/api/invitations/accept")
        .bearer(&grace)
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("member.role", json!(WorkspaceRole::Member.as_str()));

    // A member reads and creates projects, but can't manage billing.
    in_workspace(app.client().get("/api/v1/projects"), &grace, &team.slug)
        .send()
        .await
        .assert_status(200);
    in_workspace(app.client().get("/api/billing"), &grace, &team.slug)
        .send()
        .await
        .assert_status(403);
}

/// The invitation token in a mail: the `token` parameter of its link, or the
/// token itself when `workspace.invitations.url` is unset.
fn invitation_token(text: &str) -> String {
    let start = match text.find("token=") {
        Some(at) => at + "token=".len(),
        None => {
            let label = "Your invitation token: ";
            text.find(label).expect("a token in the mail") + label.len()
        }
    };
    text[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '&')
        .collect()
}
