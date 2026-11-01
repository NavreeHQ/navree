//! The cells of workbooks that openpyxl and LibreOffice wrote, against the
//! cells recorded for them (`<file>.cells`): every non-empty position of
//! every sheet, as the table keeps its value.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::*;

/// The cells of every sheet of `bytes`: `sheet <i> <name>`, then a line
/// `<row> <column> <cell>` for each position with a value (a position given
/// twice keeps its last cell), or `error` when the sheet cannot be read.
pub(super) fn dump(bytes: &[u8]) -> String {
    let limits = Limits {
        max_part_bytes: 1 << 30,
        max_text_bytes: 1 << 28,
    };
    let Ok(workbook) = Workbook::open(bytes, limits) else {
        return "open-error\n".into();
    };
    let mut out = String::new();
    for (index, name) in workbook.sheet_names().iter().enumerate() {
        let _ = writeln!(out, "sheet {index} {name}");
        let cells = workbook.read_cells(index, |cells| {
            let mut all = BTreeMap::new();
            if let Some(cells) = cells {
                while let Some(cell) = cells.next_cell()? {
                    let kept = match cell.value {
                        Value::Text("") => Cell::Empty,
                        Value::Text(text) => Cell::Text(text.to_owned()),
                        Value::Typed(typed) => typed,
                    };
                    for column in cell.column..cell.column + cell.columns {
                        all.insert((cell.row, column), kept.clone());
                    }
                }
            }
            Ok(all)
        });
        match cells {
            Ok(cells) => {
                for ((row, column), cell) in cells {
                    if !cell.is_empty() {
                        let _ = writeln!(out, "{row} {column} {cell:?}");
                    }
                }
            }
            Err(_) => out.push_str("error\n"),
        }
    }
    out
}

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            &include_bytes!(concat!("../../../tests/fixtures/workbooks/", $name))[..],
            include_str!(concat!(
                "../../../tests/fixtures/workbooks/",
                $name,
                ".cells"
            )),
        )
    };
}

#[test]
fn workbooks_are_read_cell_for_cell_as_recorded() {
    let fixtures = [
        fixture!("types-openpyxl.xlsx"),
        fixture!("types-libreoffice.xlsx"),
        fixture!("types-libreoffice.ods"),
        fixture!("types-1904-openpyxl.xlsx"),
        fixture!("types-1904-libreoffice.xlsx"),
        fixture!("types-1904-libreoffice.ods"),
        fixture!("repeats-openpyxl.xlsx"),
        fixture!("repeats-libreoffice.xlsx"),
        fixture!("repeats-libreoffice.ods"),
        fixture!("chart-openpyxl.xlsx"),
        fixture!("chart-libreoffice.xlsx"),
        fixture!("chart-libreoffice.ods"),
    ];
    for (name, bytes, recorded) in fixtures {
        let read = dump(bytes);
        if read != recorded {
            let first = read
                .lines()
                .zip(recorded.lines())
                .position(|(read, recorded)| read != recorded);
            panic!(
                "{name}: first difference at line {first:?}\nread:\n{read}\nrecorded:\n{recorded}"
            );
        }
    }
}

#[test]
fn xlsb_workbooks_are_read_cell_for_cell_as_recorded() {
    let recorded = [
        (
            "xlsb-types",
            include_str!("../../../tests/fixtures/workbooks/xlsb-types.cells"),
        ),
        (
            "xlsb-1904",
            include_str!("../../../tests/fixtures/workbooks/xlsb-1904.cells"),
        ),
        (
            "xlsb-sheets",
            include_str!("../../../tests/fixtures/workbooks/xlsb-sheets.cells"),
        ),
        (
            "xlsb-unordered",
            include_str!("../../../tests/fixtures/workbooks/xlsb-unordered.cells"),
        ),
        (
            "xlsb-columns",
            include_str!("../../../tests/fixtures/workbooks/xlsb-columns.cells"),
        ),
        (
            "xlsb-rows",
            include_str!("../../../tests/fixtures/workbooks/xlsb-rows.cells"),
        ),
    ];
    let samples = super::xlsb_writer::samples();
    assert_eq!(samples.len(), recorded.len());
    for ((name, bytes), (recorded_name, recorded)) in samples.iter().zip(recorded) {
        assert_eq!(*name, recorded_name);
        assert_eq!(dump(bytes), recorded, "{name}");
    }
}
