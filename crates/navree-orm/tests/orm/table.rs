//! `TableBuilder`: rows of any table.

use navree_orm::{Value, col, col_as, table};
use serde_json::json;

use crate::support::sqlite;

#[tokio::test]
async fn inserts_reads_updates_and_deletes_rows() {
    let t = sqlite().await;
    let inserted = table("tags")
        .insert(
            &t.db,
            [
                vec![("name", Value::from("vip")), ("color", Value::from("gold"))],
                vec![("color", Value::from("red")), ("name", Value::from("hot"))],
            ],
        )
        .await
        .unwrap();
    assert_eq!(inserted, 2);

    let rows = table("tags").order_by("name").get(&t.db).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<String>("name").unwrap(), "hot");
    assert_eq!(rows[1].get::<String>("color").unwrap(), "gold");
    assert_eq!(rows[0].columns(), ["id", "name", "color"]);
    assert_eq!(
        serde_json::to_value(&rows[1]).unwrap(),
        json!({"id": 1, "name": "vip", "color": "gold"})
    );

    let colors: Vec<String> = table("tags")
        .order_by("id")
        .pluck(&t.db, col_as::<String>("color"))
        .await
        .unwrap();
    assert_eq!(colors, ["gold", "red"]);
    let first: Option<Value> = table("tags")
        .where_(col("name").eq("vip"))
        .value(&t.db, "color")
        .await
        .unwrap();
    assert_eq!(first, Some(Value::from("gold")));
    let keyed = table("tags")
        .pluck_keyed(&t.db, col_as::<String>("color"), col_as::<String>("name"))
        .await
        .unwrap();
    assert_eq!(keyed["hot"], "red");

    let changed = table("tags")
        .where_(col("name").eq("hot"))
        .update(&t.db, [navree_orm::Assignment::value("color", "orange")])
        .await
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(
        table("tags")
            .where_(col("color").eq("orange"))
            .count(&t.db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(table("tags").increment(&t.db, "id", 10).await.unwrap(), 2);
    assert_eq!(
        table("tags").max::<i64>(&t.db, "id").await.unwrap(),
        Some(12)
    );
    assert_eq!(
        table("tags")
            .where_(col("name").eq("vip"))
            .delete(&t.db)
            .await
            .unwrap(),
        1
    );
    assert!(
        table("tags")
            .where_(col("name").eq("vip"))
            .doesnt_exist(&t.db)
            .await
            .unwrap()
    );
    assert_eq!(
        table("tags")
            .insert(&t.db, Vec::<Vec<(&str, Value)>>::new())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn rows_of_unequal_shape_are_refused() {
    let t = sqlite().await;
    let error = table("tags")
        .insert(
            &t.db,
            [
                vec![("name", Value::from("a")), ("color", Value::from("b"))],
                vec![("name", Value::from("c"))],
            ],
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: row 2 of an insert into `tags` has 1 columns, the first has 2"
    );
    let error = table("tags")
        .join("companies", "companies.id", "=", "tags.id")
        .delete(&t.db)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("a table delete takes where conditions only"),
        "{error}"
    );
}

#[tokio::test]
async fn pages_of_rows() {
    let t = sqlite().await;
    for n in 0..4 {
        table("tags")
            .insert(
                &t.db,
                [vec![
                    ("name", Value::from(format!("t{n}"))),
                    ("color", Value::from("x")),
                ]],
            )
            .await
            .unwrap();
    }
    let page = table("tags")
        .order_by("name")
        .paginate(&t.db, (2, 3))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 4);
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].get::<String>("name").unwrap(), "t3");
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(json["data"][0]["name"], "t3");
    assert_eq!(table("tags").sum::<i64>(&t.db, "id").await.unwrap(), 10);
    assert_eq!(
        table("tags").avg::<f64>(&t.db, "id").await.unwrap(),
        Some(2.5)
    );
    assert_eq!(
        table("tags").min::<i64>(&t.db, "id").await.unwrap(),
        Some(1)
    );
}
