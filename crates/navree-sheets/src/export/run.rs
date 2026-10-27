//! Writing a registered export page by page: CSV as a stream of chunks,
//! XLSX as a file built in constant memory.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use futures_util::stream::{self, Stream};
use navree_contracts::BoxError;
use navree_kernel::BoxFuture;

use super::registry::Source;
use super::{ExportContext, ExportPage};
use crate::write::csv::CsvEncoder;
use crate::write::xlsx::XlsxBuilder;
use crate::{Column, SheetsError, XLSX_MAX_ROWS};

/// Told the rows written so far (and the total when known) after every
/// page; an error stops the export (a cancelled operation).
pub(crate) type Progress =
    Arc<dyn Fn(u64, Option<u64>) -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync>;

/// How an export is paged.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) page_size: u64,
    pub(crate) max_rows: u64,
}

/// One export being written.
pub(crate) struct Paging {
    pub(crate) source: Arc<dyn Source>,
    pub(crate) ctx: ExportContext,
    pub(crate) limits: Limits,
    pub(crate) total: Option<u64>,
    pub(crate) progress: Option<Progress>,
    /// Data rows written so far.
    pub(crate) written: Arc<AtomicU64>,
}

impl Paging {
    /// The next page, with the max-rows check and the progress report.
    async fn next_page(&self, offset: u64, max_rows: u64) -> Result<Vec<Vec<crate::Cell>>, BoxError> {
        let page = ExportPage {
            offset,
            limit: self.limits.page_size,
        };
        let rows = self.source.page(&self.ctx, page).await?;
        let done = offset + rows.len() as u64;
        if done > max_rows {
            return Err(SheetsError::TooManyRows { limit: max_rows }.into());
        }
        self.written.store(done, Ordering::Relaxed);
        if let Some(progress) = &self.progress
            && !rows.is_empty()
        {
            progress(done, self.total).await?;
        }
        Ok(rows)
    }
}

enum Step {
    Header,
    Page(u64),
    Done,
}

struct CsvState {
    paging: Paging,
    encoder: Arc<CsvEncoder>,
    headers: Vec<String>,
    step: Step,
}

/// The CSV file as chunks: the byte-order mark and header row first, then
/// one chunk per page. An error ends the stream after it.
pub(crate) fn csv_stream(
    paging: Paging,
    encoder: Arc<CsvEncoder>,
    headers: Vec<String>,
) -> impl Stream<Item = Result<Bytes, BoxError>> + Send + 'static {
    let state = CsvState {
        paging,
        encoder,
        headers,
        step: Step::Header,
    };
    stream::unfold(state, |mut state| async move {
        match state.step {
            Step::Header => {
                state.step = Step::Page(0);
                let head = state.encoder.header(&state.headers).map_err(BoxError::from);
                Some((head, state))
            }
            Step::Page(offset) => {
                let max_rows = state.paging.limits.max_rows;
                match state.paging.next_page(offset, max_rows).await {
                    Err(error) => {
                        state.step = Step::Done;
                        Some((Err(error), state))
                    }
                    Ok(rows) if rows.is_empty() => None,
                    Ok(rows) => {
                        let n = rows.len() as u64;
                        state.step = if n < state.paging.limits.page_size {
                            Step::Done
                        } else {
                            Step::Page(offset + n)
                        };
                        let chunk = state.encoder.rows(&rows).map_err(BoxError::from);
                        Some((chunk, state))
                    }
                }
            }
            Step::Done => None,
        }
    })
}

/// The XLSX file: one sheet, written in constant memory; returns it and
/// its data rows.
pub(crate) async fn xlsx_file(
    paging: &Paging,
    sheet: &str,
    columns: &[Column],
    headers: &[String],
) -> Result<(Vec<u8>, u64), BoxError> {
    let max_rows = paging.limits.max_rows.min(XLSX_MAX_ROWS);
    let mut builder = XlsxBuilder::new(sheet, columns, headers, true)?;
    let mut offset = 0;
    loop {
        let rows = paging.next_page(offset, max_rows).await?;
        let n = rows.len() as u64;
        builder.append(&rows)?;
        offset += n;
        if n < paging.limits.page_size {
            break;
        }
    }
    Ok((builder.finish()?, offset))
}
