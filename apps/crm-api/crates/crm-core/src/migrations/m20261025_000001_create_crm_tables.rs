//! Creates the CRM's tables, `companies`, `contacts`, `deals` and
//! `activities`, with their indexes and foreign keys. Owners are
//! navree-auth's users: a user who owns records cannot be deleted.

use navree::schema::prelude::*;

/// `m20261025_000001_create_crm_tables`.
pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("companies", |t| {
                t.uuid_primary();
                t.string("name").index();
                t.string("domain").nullable();
                t.string("industry").nullable();
                t.string("size").length(16).nullable();
                t.string("phone").nullable();
                t.string("website").nullable();
                t.string("city").nullable();
                t.string("country").nullable();
                owner(t);
                timestamps(t);
                t.soft_deletes_tz().index();
            })
            .await?;

        schema
            .create("contacts", |t| {
                t.uuid_primary();
                t.string("first_name");
                t.string("last_name");
                t.string("email").nullable().index();
                t.string("phone").nullable();
                t.string("title").nullable();
                t.string("status").length(16).default("lead").index();
                t.foreign_uuid("company_id")
                    .nullable()
                    .index()
                    .constrained_on("companies", "id")
                    .null_on_delete();
                owner(t);
                timestamps(t);
                t.soft_deletes_tz().index();
            })
            .await?;

        schema
            .create("deals", |t| {
                t.uuid_primary();
                t.string("title");
                t.big_integer("amount_cents");
                t.string("currency").length(3).default("INR");
                t.string("stage").length(16).default("lead");
                t.integer("position").default(0);
                t.date("expected_close_date").nullable();
                t.timestamp_tz("closed_at").nullable();
                t.text("lost_reason").nullable();
                t.foreign_uuid("company_id")
                    .index()
                    .constrained_on("companies", "id")
                    .restrict_on_delete();
                t.foreign_uuid("contact_id")
                    .nullable()
                    .index()
                    .constrained_on("contacts", "id")
                    .null_on_delete();
                owner(t);
                timestamps(t);
                // The board reads a stage's column in position order.
                t.index(["stage", "position"]);
            })
            .await?;

        schema
            .create("activities", |t| {
                t.uuid_primary();
                t.string("kind").length(16);
                t.string("subject");
                t.text("body").nullable();
                t.timestamp_tz("due_at").nullable().index();
                t.timestamp_tz("completed_at").nullable();
                // The record it is attached to: a company, contact or deal
                // (no foreign key), with an index on both columns.
                t.uuid_morphs("subject");
                owner(t);
                t.foreign_uuid("assignee_id")
                    .nullable()
                    .index()
                    .constrained_on("users", "id")
                    .null_on_delete();
                timestamps(t);
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        for table in ["activities", "deals", "contacts", "companies"] {
            schema.drop_if_exists(table).await?;
        }
        Ok(())
    }
}

/// `owner_id`, the user who owns the record, indexed; the user cannot be
/// deleted while they own records.
fn owner(t: &mut Blueprint) {
    t.foreign_uuid("owner_id")
        .index()
        .constrained_on("users", "id")
        .restrict_on_delete();
}

/// `created_at` and `updated_at`, always set (the ORM fills them).
fn timestamps(t: &mut Blueprint) {
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
}
