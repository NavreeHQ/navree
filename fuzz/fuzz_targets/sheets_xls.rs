//! `navree_sheets`: XLS workbooks, by mode (byte 0): the rest as a whole
//! file (a compound file and the workbook stream in it), or as the
//! workbook stream alone, put in a compound file first so the BIFF records
//! are reached.
//!
//! Invariants: a workbook that opens reads every worksheet's cells to the
//! end without an error (each sheet is read once as the workbook opens);
//! nothing holds more than its limit.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    if mode % 2 == 0 {
        navree_sheets::fuzzing::xls(rest);
    } else {
        navree_sheets::fuzzing::xls_stream(rest);
    }
});
