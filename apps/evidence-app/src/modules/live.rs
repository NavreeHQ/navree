//! Live: `POST /api/v1/live/publish` broadcasts an event on the workspace's
//! private channel (`private-workspace.{id}`), the same path a model change
//! takes. The load profiles publish through it while WebSocket clients
//! listen on `/app/{key}`.

use garde::Validate;
use navree::contracts::Broadcaster;
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The body of `POST /api/v1/live/publish`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct Publish {
    /// The event name, `client-` events excluded: 1 to 64 characters.
    #[garde(length(min = 1, max = 64))]
    pub event: String,
    /// Any JSON payload.
    #[garde(skip)]
    #[serde(default)]
    pub data: serde_json::Value,
}

/// What a publish answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct Published {
    /// The channel the event went to.
    pub channel: String,
    /// The event name.
    pub event: String,
}

/// The publish route, behind `InWorkspace`.
#[injectable]
pub struct LiveController {
    broadcaster: Arc<dyn Broadcaster>,
}

#[controller("/live", version = "1")]
#[guard(InWorkspace::guard())]
impl LiveController {
    /// `POST /api/v1/live/publish`
    #[post("/publish")]
    #[authenticated]
    #[api(
        summary = "Publish a realtime event",
        tag = "Live",
        request = Publish,
        response = Published,
        responses(400, 401, 403)
    )]
    async fn publish(
        &self,
        ws: CurrentWorkspace,
        Valid(Json(input)): Valid<Json<Publish>>,
    ) -> Result<Json<Published>> {
        let channel = format!("private-workspace.{}", ws.id());
        self.broadcaster
            .broadcast(vec![channel.clone()], input.event.clone(), input.data)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(Json(Published {
            channel,
            event: input.event,
        }))
    }
}

/// The live routes.
#[module(controllers = [LiveController])]
pub struct LiveModule;
