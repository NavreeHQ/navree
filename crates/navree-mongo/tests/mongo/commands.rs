//! What queries compile to, without a server: the filters, pipelines and
//! counts the driver sends, as `to_mongo()` shows them (`{:#}`: a stage
//! per line).

use bson::doc;
use chrono::NaiveDate;
use navree_mongo::{MongoFilter, MongoQuery, ObjectId, ToMongo, compile};
use navree_orm::ir::{Aggregate, AggregateFunction, FullText, Operator, Order};
use navree_orm::{Expr, Filter, Model, col, table};

use crate::models::{Company, Deal, Note, Stage, WORKSPACE};

fn show(query: &impl ToMongo) -> String {
    format!("{:#}", query.to_mongo().unwrap())
}

fn acme() -> ObjectId {
    ObjectId::parse("65f1a2b3c4d5e6f708091a2b").unwrap()
}

#[test]
fn a_filter_a_sort_a_page_and_a_projection_are_a_find() {
    insta::assert_snapshot!(show(
        &Deal::query()
            .select([Deal::TITLE.expr(), Deal::AMOUNT.expr()])
            .add_select([(Deal::STAGE, "status")])
            .where_(Deal::STAGE.eq(Stage::Won))
            .where_(Deal::COMPANY_ID.eq(acme()))
            .order_by_desc(Deal::AMOUNT)
            .order_by(Deal::ID)
            .for_page(3, 25)
    ));
    // Every field of the documents; soft deletes hide the trashed ones.
    insta::assert_snapshot!(show(&Deal::query()));
    insta::assert_snapshot!(show(&Deal::query().with_trashed()));
}

#[test]
fn conditions_become_query_operators() {
    insta::assert_snapshot!(show(
        &table("deals")
            .where_(col("amount").gt(1_000))
            .where_(col("amount").ne(5_000))
            .where_in("stage", ["won", "lost"])
            .where_not_in("stage", ["lead"])
            .where_between("amount", 10, 20)
            .where_null("deleted_at")
            .where_not_null("closes_on")
            .where_(col("meta->source").eq("web"))
            .where_(col("meta->scores->0").gte(5))
    ));
    insta::assert_snapshot!(show(
        &table("deals")
            .where_like("title", "50%_off\\%%")
            .where_not_like("title", "%test%")
            .where_like_case_sensitive("code", "AB_")
            .where_(col("title").op("regexp", "^E"))
    ));
    insta::assert_snapshot!(show(
        &table("deals")
            .where_json_contains("tags", ["vip", "renewal"])
            .where_json_contains("meta", serde_json::json!({ "source": "web", "flags": { "hot": true } }))
            .where_json_length("tags", Operator::Gte, 2)
            .where_(col("meta").op("?", "labels"))
    ));
    insta::assert_snapshot!(show(
        &table("deals")
            .where_date("closes_on", "=", NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())
            .where_month("closes_on", ">=", 6)
            .where_year("closes_on", "=", 2026)
            .where_day("closes_on", "<", 15)
    ));
    insta::assert_snapshot!(show(
        &table("deals")
            .where_column("amount", ">", "budget")
            .where_group(|g| g.where_(col("stage").eq("won")).or_where(col("amount").gt(100)))
            .or_where(col("pinned").eq(true))
    ));
    insta::assert_snapshot!(show(
        &table("deals")
            .where_not(col("stage").eq("won"))
            .where_not(col("amount").gt(10))
    ));
}

#[test]
fn global_scopes_join_the_filter() {
    let workspace = acme();
    let scoped = WORKSPACE.sync_scope(workspace, || show(&Note::query().where_(Note::BODY.eq("hi"))));
    insta::assert_snapshot!(scoped);
}

#[test]
fn orders_put_nulls_where_asked_and_shuffle_with_rand() {
    insta::assert_snapshot!(show(
        &Company::query()
            .order(Order::asc(Company::COUNTRY.expr()).nulls_last())
            .order(Order::desc(Expr::col("employees")).nulls_first())
            .limit(10)
    ));
    insta::assert_snapshot!(show(&Company::query().in_random_order().limit(3)));
    insta::assert_snapshot!(show(
        &table("deals")
            .select(["title"])
            .select_expr(Expr::col("amount").mul(Expr::val(2)), "doubled")
            .order_by_desc("doubled")
    ));
}

#[test]
fn groups_become_group_stages() {
    insta::assert_snapshot!(show(
        &table("deals")
            .select(["stage"])
            .select_expr(Expr::count_all(), "deals")
            .select_expr(Expr::sum(Expr::col("amount")), "total")
            .select_expr(Expr::avg(Expr::col("amount")), "average")
            .group_by(["stage"])
            .having(Expr::count_all().gt(1))
            .order_by_desc("total")
    ));
    // TimescaleDB's buckets and first/last.
    insta::assert_snapshot!(show(
        &table("readings")
            .select(["sensor"])
            .select_time_bucket("15 minutes", "taken_at", "bucket")
            .select_first("celsius", "taken_at", "opening")
            .select_last("celsius", "taken_at", "closing")
            .select_expr(Expr::max(Expr::col("celsius")), "high")
            .group_by(["sensor", "bucket"])
            .order_by("bucket")
    ));
    insta::assert_snapshot!(show(
        &table("readings")
            .select_time_bucket("1 month", "taken_at", "month")
            .select_expr(Expr::count_all(), "readings")
            .group_by(["month"])
    ));
    // HAVING over group keys: LIKE, and the comparisons' NULL rule.
    insta::assert_snapshot!(show(
        &table("deals")
            .select(["owner"])
            .select_expr(Expr::max(Expr::col("amount")), "top")
            .group_by(["owner"])
            .having(col("owner").like("a%"))
            .having(Expr::max(Expr::col("amount")).lt(Expr::min(Expr::col("budget"))))
    ));
    let refused = table("deals")
        .select(["owner"])
        .group_by(["owner"])
        .having(Filter::exists(table("owners")))
        .to_mongo()
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "EXISTS (`where_exists`, `where_has`) in HAVING (HAVING compares group keys and aggregates; filter the documents with `where`) is not supported on MongoDB"
    );
}

#[test]
fn distinct_values_group_on_what_is_selected() {
    insta::assert_snapshot!(show(&table("deals").select(["stage"]).distinct().order_by("stage")));
    insta::assert_snapshot!(show(
        &Deal::query()
            .distinct_on([Deal::COMPANY_ID])
            .order_by(Deal::COMPANY_ID)
            .order_by_desc(Deal::AMOUNT)
    ));
}

#[test]
fn joins_are_lookups() {
    insta::assert_snapshot!(show(
        &table("deals")
            .join("companies", "companies._id", "=", "deals.company_id")
            .select(["deals.title", "companies.name"])
            .where_(col("companies.country").eq("US"))
    ));
    insta::assert_snapshot!(show(
        &table("companies")
            .left_join("deals", "deals.company_id", "=", "companies._id")
            .where_null("deals._id")
            .select(["companies.name"])
    ));
    insta::assert_snapshot!(show(
        &table("deals")
            .join_on("companies", |on| {
                on.on("companies._id", "=", "deals.company_id")
                    .where_(col("companies.employees").gt(100))
            })
            .select(["deals.title"])
    ));
    // A self-join: the alias hides the name, and NULL keys join nothing.
    insta::assert_snapshot!(show(
        &table("companies")
            .left_join("companies as twins", "twins.country", "=", "companies.country")
            .where_null("twins._id")
            .select(["companies.name"])
    ));
    // No equality to match by: the conditions run in the pipeline.
    insta::assert_snapshot!(show(
        &table("deals")
            .join_on("companies", |on| {
                on.on("companies.founded_on", "<", "deals.closes_on")
                    .or_on("companies.employees", ">", "deals.amount")
            })
            .select(["deals.title"])
    ));
}

#[test]
fn subqueries_are_correlated_lookups() {
    insta::assert_snapshot!(show(
        &Company::query().where_exists(
            Deal::query()
                .where_column(Deal::COMPANY_ID, "=", Company::ID)
                .where_(Deal::STAGE.eq(Stage::Won)),
        )
    ));
    insta::assert_snapshot!(show(
        &Deal::query().where_in_sub(
            Deal::COMPANY_ID,
            Company::query().select([Company::ID]).where_(Company::COUNTRY.eq("NO")),
        )
    ));
    insta::assert_snapshot!(show(
        &Company::query().select_sub(
            Deal::query()
                .select([(Expr::count_all(), "n")])
                .where_column(Deal::COMPANY_ID, "=", Company::ID),
            "deals_count",
        )
    ));
}

#[test]
fn unions_append_pipelines() {
    insta::assert_snapshot!(show(
        &table("companies")
            .select(["name"])
            .where_(col("country").eq("NO"))
            .union_all(table("deals").select(["title"]).where_(col("amount").lt(10_000)))
            .union(table("companies").select(["name"]).where_(col("country").eq("US")))
            .order_by("name")
            .limit(5)
    ));
}

#[test]
fn full_text_runs_first() {
    insta::assert_snapshot!(show(
        &Deal::query()
            .where_(Deal::STAGE.eq(Stage::Won))
            .where_full_text([Deal::TITLE.expr()], "enterprise plan")
    ));
    insta::assert_snapshot!(show(&Deal::query().where_full_text_with(
        [Deal::TITLE.expr()],
        "ACME support",
        FullText::phrase().language("english"),
    )));
}

#[test]
fn aggregates_counts_and_existence() {
    let won = Deal::query().where_(Deal::STAGE.eq(Stage::Won)).into_ir();
    let aggregate = |function, arg: Option<&'static str>, distinct| Aggregate {
        function,
        arg: arg.map(Expr::col),
        distinct,
    };
    let count = compile::aggregate(&won, &aggregate(AggregateFunction::Count, None, false)).unwrap();
    insta::assert_snapshot!(format!("{count:#}"));
    let sum = compile::aggregate(&won, &aggregate(AggregateFunction::Sum, Some("amount"), false)).unwrap();
    insta::assert_snapshot!(format!("{sum:#}"));
    let distinct = compile::aggregate(&won, &aggregate(AggregateFunction::Count, Some("stage"), true)).unwrap();
    insta::assert_snapshot!(format!("{distinct:#}"));
    let limited = Deal::query().order_by_desc(Deal::AMOUNT).limit(3).into_ir();
    let top = compile::aggregate(&limited, &aggregate(AggregateFunction::Avg, Some("amount"), false)).unwrap();
    insta::assert_snapshot!(format!("{top:#}"));
    let exists = compile::exists(&won).unwrap();
    insta::assert_snapshot!(format!("{exists:#}"));
}

#[test]
fn native_documents_and_stages() {
    insta::assert_snapshot!(show(
        &Deal::query()
            .where_(Filter::document(doc! { "tags": { "$elemMatch": { "$eq": "vip" } } }))
            .or_where(Deal::AMOUNT.gt(1_000_000))
    ));
    let pipeline = Deal::query()
        .where_(Deal::STAGE.eq(Stage::Won))
        .pipeline([
            doc! { "$lookup": { "from": "companies", "localField": "company_id", "foreignField": "_id", "as": "company" } },
            doc! { "$set": { "company_name": { "$first": "$company.name" } } },
        ])
        .to_mongo()
        .unwrap();
    insta::assert_snapshot!(format!("{pipeline:#}"));
}

#[test]
fn shapes_keep_the_structure_and_drop_the_values() {
    let command = Deal::query()
        .where_(Deal::TITLE.eq("Secret deal"))
        .where_in(Deal::STAGE, [Stage::Won, Stage::Lost])
        .order_by_desc(Deal::AMOUNT)
        .limit(5)
        .to_mongo()
        .unwrap();
    let shape = command.shape();
    assert!(!shape.contains("Secret"), "{shape}");
    insta::assert_snapshot!(shape);
}

#[test]
fn what_mongodb_lacks_is_refused() {
    let refused = |result: Result<navree_mongo::MongoCommand, navree_orm::OrmError>| {
        result.unwrap_err().to_string()
    };
    insta::assert_snapshot!(
        [
            refused(Deal::query().where_raw("amount > ?", [5]).to_mongo()),
            refused(Deal::query().lock_for_update().to_mongo()),
            refused(table("deals").right_join("companies", "companies._id", "=", "deals.company_id").to_mongo()),
            refused(Deal::query().order_by_raw("amount DESC", Vec::<navree_orm::Value>::new()).to_mongo()),
            refused(table("deals").select_expr(Expr::raw("now()", Vec::<navree_orm::Value>::new()), "now").to_mongo()),
            refused(table("deals").select([Expr::col("amount").mul(Expr::val(2))]).to_mongo()),
            refused(table("deals").select(["title"]).union(table("companies").select(["name", "country"])).to_mongo()),
        ]
        .join("\n")
    );
}
