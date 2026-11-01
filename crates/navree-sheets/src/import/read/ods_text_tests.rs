//! The text of an ODS cell: whitespace that ends it is trimmed without being
//! made, however many spaces `text:s` says; whitespace inside it is kept as
//! the file says it; and the spaces a sheet makes count together.

use std::time::{Duration, Instant};

use super::opening_tests::{MIB, ods, ods_text_cell, opts, text};
use super::*;
use crate::import::workbook::{Limits, Workbook};

fn table_of(rows: &str) -> String {
    format!(r#"<table:table table:name="S">{rows}</table:table>"#)
}

/// A cell of text whose paragraphs are `paragraphs`, each made of markup.
fn cell(paragraphs: &[&str]) -> String {
    let paragraphs: String = paragraphs
        .iter()
        .map(|paragraph| format!("<text:p>{paragraph}</text:p>"))
        .collect();
    format!(r#"<table:table-cell office:value-type="string">{paragraphs}</table:table-cell>"#)
}

fn spaces(count: usize) -> String {
    format!(r#"<text:s text:c="{count}"/>"#)
}

/// The text the table keeps for a cell that is the markup `paragraphs`, in the
/// row under a header; none when the cell is empty and the row dropped.
fn kept(paragraphs: &[&str]) -> Option<String> {
    let file = ods(&table_of(&format!(
        "<table:table-row>{}</table:table-row><table:table-row>{}</table:table-row>",
        ods_text_cell("h"),
        cell(paragraphs)
    )));
    let table = read_table(&file, "x.ods", &opts(1 << 28)).unwrap();
    table
        .rows
        .first()
        .map(|row| row.cells[0].to_text().into_owned())
}

#[test]
fn spaces_inside_a_cell_are_kept_and_the_ones_that_end_it_are_not() {
    let s3 = spaces(3);
    let s2 = spaces(2);
    for (paragraphs, expected) in [
        (vec![format!("a{s3}b")], Some("a   b")),
        (vec![format!("a{s3}")], Some("a")),
        (vec![format!("a{s3} ")], Some("a")),
        (vec![format!("{s2}a")], Some("a")),
        (vec![format!("{s2} a")], Some("a")),
        (vec![s2.clone()], None),
        (vec![format!("a{s3}<text:tab/>{s2}b")], Some("a   \t  b")),
        (
            vec![format!("a<text:tab/>{s2}<text:line-break/>{s2}b")],
            Some("a\t  \n  b"),
        ),
        (vec![format!("a<text:tab/>{s2}<text:tab/>")], Some("a")),
        (
            vec![format!("a{s2}<text:tab/>{s2}<text:tab/>{s2}b")],
            Some("a  \t  \t  b"),
        ),
        (vec!["a".into(), " ".into(), "b".into()], Some("a\n \nb")),
        (vec!["a".into(), String::new()], Some("a")),
        (vec![String::new(), "a".into()], Some("a")),
        (vec!["a&#160;".to_owned()], Some("a")),
        (vec![format!("a{s2}&#160;b")], Some("a  \u{a0}b")),
    ] {
        let markup: Vec<&str> = paragraphs.iter().map(String::as_str).collect();
        assert_eq!(kept(&markup).as_deref(), expected, "{paragraphs:?}");
    }
}

/// Rows that each make `spaces` spaces that trim away; the file is a few
/// hundred bytes a row, and what the spaces would cost is gigabytes.
fn trailing_spaces_cost_nothing(ending: &str) {
    let count = 4 * MIB - 64;
    let row = format!(
        "<table:table-row>{}</table:table-row>",
        cell(&[&format!("a{}{ending}", spaces(count))])
    );
    let header = format!("<table:table-row>{}</table:table-row>", ods_text_cell("h"));
    let file = ods(&table_of(&format!("{header}{}", row.repeat(300))));
    let at = Instant::now();
    let table = read_table(&file, "x.ods", &opts(4 * MIB as u64)).unwrap();
    let took = at.elapsed();
    assert_eq!(table.rows.len(), 300);
    assert!(table.rows.iter().all(|row| row.cells == [text("a")]));
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn spaces_that_trim_away_after_a_space_are_not_made() {
    trailing_spaces_cost_nothing(" ");
}

#[test]
fn spaces_that_trim_away_before_a_tab_are_not_made() {
    trailing_spaces_cost_nothing("<text:tab/>");
}

#[test]
fn spaces_that_trim_away_before_a_line_break_or_a_paragraph_are_not_made() {
    trailing_spaces_cost_nothing("<text:line-break/>");
}

#[test]
fn spaces_made_across_a_sheet_count_together_against_the_limit() {
    // Each row's cell is within the limit of a row, and the cells together
    // are not; read with no table to count them.
    let row = format!(
        "<table:table-row>{}</table:table-row>",
        cell(&[&format!("a{}b", spaces(MIB))])
    );
    let file = ods(&table_of(&row.repeat(100)));
    let book = Workbook::open(
        &file,
        Limits {
            max_part_bytes: 1 << 30,
            max_text_bytes: 8 * MIB as u64,
        },
    )
    .unwrap();
    let result = book.read_cells(0, None, |cells| {
        let cells = cells.unwrap();
        let mut count = 0;
        while cells.next_cell()?.is_some() {
            count += 1;
        }
        Ok(count)
    });
    assert!(
        matches!(&result, Err(SheetsError::UnsupportedFile(message)) if message.contains("more than 8388608 bytes of text")),
        "{result:?}"
    );
    // The same cells, a few of them: 6 MiB of spaces.
    let few = ods(&table_of(&row.repeat(6)));
    let table = read_table(&few, "x.ods", &opts(8 * MIB as u64));
    assert!(table.is_ok(), "{table:?}");
}
