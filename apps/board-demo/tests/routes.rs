//! The board's routes, through the app's router: a column's next page starts
//! right after the last card drawn wherever that card is now, a card's move form
//! under a filter counts in the whole column and goes back to the filtered board,
//! the board is shown as a member until someone chooses otherwise, and choosing
//! takes a post from this site.

use board_demo::data::{Card, Person};
use board_demo::{Settings, app};
use navree::db::Db;
use navree::orm::Model as _;
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};

/// The app on a database of its own, written afresh, with no teammate moving cards.
async fn boot() -> (TestApp, TempDirGuard) {
    let (url, dir) = temp_sqlite_url();
    let settings = Settings {
        simulate_every: None,
        test_hooks: false,
    };
    (TestApp::from_builder(app(0, url, settings)).await, dir)
}

/// The value of every `name="value"` attribute in `html`, in order.
fn values<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let marker = format!("{name}=\"");
    html.match_indices(&marker)
        .filter_map(|(at, _)| {
            let rest = &html[at + marker.len()..];
            rest.find('"').map(|end| &rest[..end])
        })
        .collect()
}

/// The part of the page that draws list `list`.
fn list<'a>(html: &'a str, list: &str) -> &'a str {
    let start = html
        .find(&format!("id=\"columns-list-{list}\""))
        .unwrap_or_else(|| panic!("no list {list}"));
    let rest = &html[start..];
    let end = rest[1..]
        .find("data-slot=\"sortable-list\"")
        .map_or(rest.len(), |end| end + 1);
    &rest[..end]
}

/// The cards `list` draws, by id.
fn keys(html: &str, name: &str) -> Vec<i64> {
    values(list(html, name), "data-key")
        .into_iter()
        .map(|key| key.parse().expect("a card's key is its id"))
        .collect()
}

/// The value of the field `field` in the element `id`: a form, or a card, whose
/// own move form holds it.
fn field(html: &str, id: &str, field: &str) -> String {
    let start = html
        .find(&format!("id=\"{id}\""))
        .unwrap_or_else(|| panic!("no {id}"));
    let rest = &html[start..];
    // A card ends where the next card, or its list, begins; a form at its end.
    let end = if id.starts_with("columns-item-") {
        ["data-slot=\"sortable-item\"", "</ol>"]
            .iter()
            .filter_map(|mark| rest.find(mark))
            .min()
    } else {
        rest.find("</form>")
    };
    let form = &rest[..end.expect("its end")];
    let at = form
        .find(&format!("name=\"{field}\""))
        .unwrap_or_else(|| panic!("no {field} in {id}"));
    values(&form[at..], "value")[0].to_owned()
}

/// The ids of `list`'s cards, in order, as the database has them.
async fn order(db: &Db, list: &str) -> Vec<i64> {
    Card::query()
        .where_(Card::LIST.eq(list))
        .order_by("rank")
        .order_by("id")
        .get(db)
        .await
        .unwrap()
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner().id)
        .collect()
}

async fn rank(db: &Db, id: i64) -> i64 {
    Card::query()
        .find(db, id)
        .await
        .unwrap()
        .unwrap()
        .into_inner()
        .rank
}

async fn lead(db: &Db) -> i64 {
    Person::query()
        .where_(Person::ROLE.eq("lead"))
        .first(db)
        .await
        .unwrap()
        .expect("a lead")
        .into_inner()
        .id
}

/// Moves card `item` with the form a page posts, with JavaScript off: the answer's
/// status and where it sends the browser.
async fn post_move(
    client: &navree::testing::TestClient,
    action: &str,
    fields: &[(&str, String)],
) -> (u16, String) {
    let response = client.post(action).form(fields).send().await;
    let location = response.header("location").unwrap_or_default().to_owned();
    (response.status().as_u16(), location)
}

#[tokio::test]
async fn the_next_page_starts_right_after_the_last_card_drawn_even_after_fresh_ranks() {
    let (app, _dir) = boot().await;
    let db = app.get::<Db>().await;
    let client = app.client();
    let lead = lead(&db).await;
    assert_eq!(
        client
            .post(&format!("/as/{lead}"))
            .form(&[("go", "")])
            .send()
            .await
            .status(),
        303
    );
    let page = client.get("/").send().await.text();
    let first = keys(&page, "backlog");
    assert_eq!(first.len(), 20);
    let cursor = field(&page, "columns-list-backlog-more", "after");
    let last = first[19];
    let drawn_at = rank(&db, last).await;

    // Four cards above the last one drawn leave the column.
    for id in &first[..4] {
        let fields = [
            ("item", id.to_string()),
            ("from_list", "backlog".to_owned()),
            ("to_list", "done".to_owned()),
        ];
        assert_eq!(post_move(&client, "/board/moves", &fields).await.0, 303);
    }
    // Two more pass each other until no rank is left between them and the column
    // gets fresh ones.
    let (a, b) = (first[5], first[6]);
    for step in 0..18 {
        let (moved, before) = if step % 2 == 0 { (b, a) } else { (a, b) };
        let fields = [
            ("item", moved.to_string()),
            ("from_list", "backlog".to_owned()),
            ("to_list", "backlog".to_owned()),
            ("before", before.to_string()),
        ];
        assert_eq!(post_move(&client, "/board/moves", &fields).await.0, 303);
    }
    assert!(
        rank(&db, last).await < drawn_at,
        "the column got fresh ranks, lower than when the page was drawn"
    );

    let next = client
        .get(&format!("/columns/backlog?after={cursor}&show=40"))
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert_eq!(next.status(), 200);
    let ops = next.json_value().to_string().replace("\\\"", "\"");
    let second: Vec<i64> = values(&ops, "data-key")
        .into_iter()
        .map(|key| key.parse().unwrap())
        .collect();
    let now = order(&db, "backlog").await;
    let at = now.iter().position(|id| *id == last).unwrap();
    assert_eq!(
        second,
        now[at + 1..at + 21],
        "the 20 cards right after the last one drawn"
    );
}

#[tokio::test]
async fn under_a_filter_a_cards_move_form_counts_in_the_whole_column_and_goes_back_filtered() {
    let (app, _dir) = boot().await;
    let db = app.get::<Db>().await;
    let client = app.client();
    let page = client.get("/?label=bug").send().await.text();
    let shown = keys(&page, "backlog");
    assert!(shown.len() > 3, "{shown:?}");
    let whole = order(&db, "backlog").await;
    let place = |id: i64| whole.iter().position(|other| *other == id).unwrap() + 1;
    let mut skipped = false;
    for (at, id) in shown.iter().enumerate() {
        let position: usize = field(&page, &format!("columns-item-{id}"), "position")
            .parse()
            .unwrap();
        assert_eq!(position, place(*id), "card {id}");
        skipped |= position != at + 1;
    }
    assert!(skipped, "the filter leaves cards of the column out");
    // The forms post to the moves route with the filter, and go back to it.
    let action = values(list(&page, "backlog"), "action")
        .into_iter()
        .find(|action| action.starts_with("/board/moves"))
        .unwrap()
        .to_owned();
    assert_eq!(action, "/board/moves?label=bug");
    let card = shown[1];
    let to = place(card) + 2;
    let fields = [
        ("item", card.to_string()),
        ("from_list", "backlog".to_owned()),
        ("to_list", "backlog".to_owned()),
        ("position", to.to_string()),
    ];
    let (status, location) = post_move(&client, &action, &fields).await;
    assert_eq!((status, location.as_str()), (303, "/?label=bug"));
    assert_eq!(
        place_now(&db, card).await,
        to,
        "it lands at that place in the whole column"
    );
}

async fn place_now(db: &Db, id: i64) -> usize {
    order(db, "backlog")
        .await
        .iter()
        .position(|other| *other == id)
        .unwrap()
        + 1
}

#[tokio::test]
async fn the_board_is_shown_as_a_member_until_someone_chooses_otherwise() {
    let (app, _dir) = boot().await;
    let db = app.get::<Db>().await;
    let client = app.client();
    let page = client.get("/").send().await.text();
    assert!(
        list(&page, "done").contains("data-refuse="),
        "Done takes no card from a member"
    );
    let review = order(&db, "review").await;
    let refused = client
        .post("/board/moves")
        .header("x-navree-ui", "1")
        .form(&[
            ("item", review[0].to_string()),
            ("from_list", "review".to_owned()),
            ("to_list", "done".to_owned()),
        ])
        .send()
        .await;
    assert_eq!(refused.status(), 409);
    assert_eq!(order(&db, "review").await, review);
}

#[tokio::test]
async fn choosing_who_to_show_the_board_as_takes_a_post_from_this_site() {
    let (app, _dir) = boot().await;
    let db = app.get::<Db>().await;
    let lead = lead(&db).await;
    let client = app.client();
    let forged = client
        .post(&format!("/as/{lead}"))
        .header("origin", "https://elsewhere.example")
        .form(&[("go", "")])
        .send()
        .await;
    assert_eq!(forged.status(), 403);
    assert!(forged.set_cookie("board_as").is_none());
    let page = client.get("/").send().await.text();
    assert!(
        list(&page, "done").contains("data-refuse="),
        "still a member"
    );

    let chosen = client
        .post(&format!("/as/{lead}"))
        .form(&[("go", "")])
        .send()
        .await;
    assert_eq!(chosen.status(), 303);
    assert_eq!(
        chosen.set_cookie("board_as").map(|c| c.value().to_owned()),
        Some(lead.to_string())
    );
    let page = client.get("/").send().await.text();
    assert!(
        !list(&page, "done").contains("data-refuse="),
        "a lead closes cards"
    );
}

/// The cards of a column's next page, from the ops `GET /columns/{column}` answers.
async fn next_page(client: &navree::testing::TestClient, cursor: &str, show: usize) -> Vec<i64> {
    let next = client
        .get(&format!("/columns/backlog?after={cursor}&show={show}"))
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert_eq!(next.status(), 200);
    let ops = next.json_value().to_string().replace("\\\"", "\"");
    values(&ops, "data-key")
        .into_iter()
        .map(|key| key.parse().unwrap())
        .collect()
}

#[tokio::test]
async fn when_the_last_card_drawn_has_left_the_column_the_next_page_skips_nothing() {
    let (app, _dir) = boot().await;
    let db = app.get::<Db>().await;
    let client = app.client();
    let lead = lead(&db).await;
    client
        .post(&format!("/as/{lead}"))
        .form(&[("go", "")])
        .send()
        .await;
    let page = client.get("/").send().await.text();
    let first = keys(&page, "backlog");
    let cursor = field(&page, "columns-list-backlog-more", "after");
    let last = first[19];
    // The cursor's card leaves the column, and no redraw follows.
    let fields = [
        ("item", last.to_string()),
        ("from_list", "backlog".to_owned()),
        ("to_list", "done".to_owned()),
    ];
    assert_eq!(post_move(&client, "/board/moves", &fields).await.0, 303);
    let second = next_page(&client, &cursor, 40).await;
    let now = order(&db, "backlog").await;
    // The card that moved up into place 20 is the next page's first: counting from
    // one card early, the page can repeat a card (which the page joins once) but
    // never miss one until the next redraw.
    assert_eq!(second[0], now[19], "starts at the card now at place 20");
    assert_eq!(second, now[19..39]);
    assert!(first[..19].iter().all(|id| *id != second[0]));
}

#[tokio::test]
async fn signing_out_takes_a_post_from_this_site() {
    let (app, _dir) = boot().await;
    let client = app.client();
    let forged = client
        .post("/sign-out")
        .header("origin", "https://elsewhere.example")
        .form(&[("go", "")])
        .send()
        .await;
    assert_eq!(forged.status(), 403);
    let own = client.post("/sign-out").form(&[("go", "")]).send().await;
    assert_eq!(own.status(), 303);
}

#[tokio::test]
async fn comments_and_history_take_ids_the_database_gives() {
    let (app, _dir) = boot().await;
    let db = app.get::<Db>().await;
    let client = app.client();
    let card = order(&db, "review").await[0];
    let before = board_demo::data::Comment::query().count(&db).await.unwrap();
    for body in ["First look done.", "Second look done."] {
        let posted = client
            .post(&format!("/cards/{card}/comments"))
            .form(&[("body", body)])
            .send()
            .await;
        assert_eq!(posted.status(), 303, "{}", posted.text());
    }
    let comments = board_demo::data::Comment::query()
        .order_by_desc("id")
        .limit(2)
        .get(&db)
        .await
        .unwrap()
        .into_vec();
    assert_eq!(
        board_demo::data::Comment::query().count(&db).await.unwrap(),
        before + 2
    );
    let ids: Vec<i64> = comments.iter().map(|c| c.clone().into_inner().id).collect();
    assert!(ids[0] > ids[1] && ids[1] > 0, "{ids:?}");
}
