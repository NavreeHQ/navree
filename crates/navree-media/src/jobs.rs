//! [`GenerateConversions`]: the job that generates queued conversions.

use navree_contracts::{BoxError, Job, JobContext, JobOptions};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Media, MediaError};

/// The job `media.conversions`: generates conversions of one media item
/// and saves them on its row.
///
/// [`PendingMedia::save`](crate::PendingMedia::save) dispatches one for
/// the queued conversions of a new file, and
/// [`Media::queue_regenerate`] (and `media:regenerate --queue`) one per
/// item in a batch, all on `media.queue`. An item deleted before the job
/// runs is skipped. A failed attempt is retried (three attempts, with
/// backoff); each attempt may take 15 minutes, enough for a transcode
/// within `media.ffmpeg.timeout_secs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerateConversions {
    /// The media item.
    pub media_id: Uuid,
    /// The conversions to generate, by name (all of the collection's when
    /// empty).
    pub conversions: Vec<String>,
}

impl Job for GenerateConversions {
    const NAME: &'static str = "media.conversions";

    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: 900,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let media = ctx.app.get::<Media>().await?;
        let only = (!self.conversions.is_empty()).then_some(self.conversions.as_slice());
        match media.convert(self.media_id, only).await {
            Ok(_) | Err(MediaError::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
