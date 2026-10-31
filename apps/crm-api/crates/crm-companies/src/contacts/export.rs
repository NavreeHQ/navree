//! The contacts CSV export (`GET /api/v1/contacts/export`).

use chrono::SecondsFormat;
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use navree::http::response::{IntoResponse, Response};
use navree::prelude::*;

use super::resource::ContactResource;
use crate::common::csv_safe;

/// The export's columns, in order.
pub const CSV_HEADER: [&str; 10] = [
    "id",
    "first_name",
    "last_name",
    "email",
    "phone",
    "title",
    "status",
    "company",
    "owner",
    "created_at",
];

/// The export's content type.
pub const CSV_CONTENT_TYPE: &str = "text/csv; charset=utf-8";

/// `contacts` as RFC 4180 CSV (CRLF line ends, a header row), one row per
/// contact; text cells cannot start a spreadsheet formula.
pub fn contacts_csv(contacts: &[ContactResource]) -> Result<Vec<u8>, csv::Error> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(Vec::new());
    writer.write_record(CSV_HEADER)?;
    for contact in contacts {
        let text =
            |value: &Option<String>| csv_safe(value.as_deref().unwrap_or_default()).into_owned();
        writer.write_record([
            contact.id.to_string(),
            csv_safe(&contact.first_name).into_owned(),
            csv_safe(&contact.last_name).into_owned(),
            text(&contact.email),
            text(&contact.phone),
            text(&contact.title),
            contact.status.to_string(),
            text(&contact.company.as_ref().map(|company| company.name.clone())),
            csv_safe(&contact.owner.name).into_owned(),
            contact
                .created_at
                .to_rfc3339_opts(SecondsFormat::AutoSi, true),
        ])?;
    }
    writer
        .into_inner()
        .map_err(|error| csv::Error::from(error.into_error()))
}

/// The CSV download response (`contacts.csv`).
pub fn csv_response(contacts: &[ContactResource]) -> Result<Response> {
    let body = contacts_csv(contacts).map_err(AppError::internal)?;
    Ok((
        [
            (CONTENT_TYPE, CSV_CONTENT_TYPE),
            (CONTENT_DISPOSITION, "attachment; filename=\"contacts.csv\""),
        ],
        body,
    )
        .into_response())
}
