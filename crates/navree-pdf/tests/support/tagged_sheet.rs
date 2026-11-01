//! A tagged spreadsheet as an office suite writes it: twelve columns of
//! numbers, a page for each fifty-three rows, and a structure element for each
//! cell (a `TD` that holds a `P` that holds the cell's marked content), for
//! each row (a `TR`) and for each page (a `Table`), with the parent tree that
//! leads from the marked content back to its elements. Of every ten objects,
//! nine are structure elements.

use navree_pdf::{Dictionary, Object, ObjectId, Objects, Stream, dictionary};

const COLUMNS: usize = 12;
const ROWS_PER_PAGE: usize = 53;

fn element(kind: &str, parent: ObjectId, page: ObjectId, kids: Vec<Object>) -> Dictionary {
    dictionary! {
        "Type" => "StructElem", "S" => kind, "P" => parent, "Pg" => page, "K" => kids,
    }
}

/// The objects of a sheet of `rows` rows. Saved plain (`save`) or in object
/// streams (`save_with(true)`), after `compress`, it is the file such a writer
/// makes, and `rows / 53` pages long.
pub fn tagged_sheet(rows: usize) -> Objects {
    let mut objects = Objects::new();
    let pages = objects.reserve();
    let workbook = objects.reserve();
    let parent_tree = objects.reserve();
    let font = objects.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = objects.add(dictionary! {
        "Font" => dictionary! { "F1" => font },
    });
    let mut kids = Vec::new();
    let mut tables = Vec::new();
    let mut numbers: Vec<Object> = Vec::new();
    let mut seed = 1u64;
    for (index, first) in (0..rows).step_by(ROWS_PER_PAGE).enumerate() {
        let page = objects.reserve();
        let table = objects.reserve();
        let mut content =
            String::from("0.1 w\n/Artifact BMC\nq 0 0.028 595.276 841.861 re\nW* n\nQ EMC\n");
        let mut marked: Vec<Object> = Vec::new();
        let mut trs: Vec<Object> = Vec::new();
        let last = (first + ROWS_PER_PAGE).min(rows);
        for row in first..last {
            let tr = objects.reserve();
            let mut tds: Vec<Object> = Vec::new();
            for column in 0..COLUMNS {
                let id = marked.len();
                let td = objects.reserve();
                let p = objects.add(element("P", td, page, vec![Object::Integer(id as i64)]));
                objects.insert(td, element("TD", tr, page, vec![p.into()]));
                tds.push(td.into());
                marked.push(p.into());
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                content.push_str(&format!(
                    "/P<</MCID {id}>>BDC\nq 0 0 0 rg\nBT\n{:.3} {:.3} Td /F1 10.006 Tf<{:02x}{:02x}{:02x}{:02x}{:02x}>Tj\nET\nQ\nEMC\n",
                    63.496 + 40.0 * column as f32,
                    754.582 - 14.1 * (row - first) as f32,
                    (seed >> 33) as u8,
                    (seed >> 41) as u8,
                    (seed >> 49) as u8,
                    (seed >> 25) as u8,
                    (seed >> 17) as u8,
                ));
            }
            objects.insert(tr, element("TR", table, page, tds));
            trs.push(tr.into());
        }
        objects.insert(table, element("Table", workbook, page, trs));
        tables.push(Object::from(table));
        let contents = objects.add(Stream::new(Dictionary::new(), content.into_bytes()));
        objects.insert(
            page,
            dictionary! {
                "Type" => "Page", "Parent" => pages, "Contents" => contents,
                "MediaBox" => vec![
                    0.into(), 0.into(), Object::Real(595.304), Object::Real(841.89),
                ],
                "Resources" => resources, "StructParents" => index as i64, "Tabs" => "S",
            },
        );
        kids.push(Object::from(page));
        numbers.push((index as i64).into());
        numbers.push(Object::Array(marked));
    }
    let count = kids.len() as i64;
    objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count },
    );
    let structure = objects.reserve();
    objects.insert(
        workbook,
        dictionary! {
            "Type" => "StructElem", "S" => "Workbook", "P" => structure, "K" => tables,
        },
    );
    objects.insert(parent_tree, dictionary! { "Nums" => numbers });
    objects.insert(
        structure,
        dictionary! {
            "Type" => "StructTreeRoot", "K" => vec![workbook.into()], "ParentTree" => parent_tree,
        },
    );
    let catalog = objects.add(dictionary! {
        "Type" => "Catalog", "Pages" => pages, "StructTreeRoot" => structure,
        "MarkInfo" => dictionary! { "Marked" => true }, "Lang" => Object::string_literal("en-US"),
    });
    objects.trailer.set("Root", catalog);
    objects
}
