//! What a stage change does to a deal's closing columns: won and lost set `closed_at`, lost needs a `lost_reason`, and
//! moving back to an open stage reopens the deal.

use chrono::{DateTime, Utc};
use crm_core::Stage;
use navree::validation::ValidationErrors;

/// The message for a lost deal without a reason.
const REASON_REQUIRED: &str = "lost_reason should not be empty";

/// A deal's closing columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Closing {
    /// When it was won or lost.
    pub(crate) closed_at: Option<DateTime<Utc>>,
    /// Why it was lost.
    pub(crate) lost_reason: Option<String>,
}

impl Closing {
    /// An open deal.
    pub(crate) const OPEN: Closing = Closing {
        closed_at: None,
        lost_reason: None,
    };
}

/// The closing columns of a deal going from `from` (its stage and closing
/// columns; `None` for a new deal) to `stage` at `now`, with the
/// `lost_reason` the request sent.
///
/// - open stages: `closed_at` and `lost_reason` are cleared (reopened);
/// - `won`: closed now, unless it was already won (then it keeps its
///   date); no reason;
/// - `lost`: needs a non-blank reason, sent now or kept from before when it
///   was already lost (a 400 on `lost_reason` otherwise); closed now unless
///   it was already lost.
pub(crate) fn close(
    from: Option<(Stage, &Closing)>,
    stage: Stage,
    lost_reason: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Closing, ValidationErrors> {
    let stays = from.filter(|(previous, _)| *previous == stage);
    let closed_at = || stays.map_or(Some(now), |(_, closing)| closing.closed_at.or(Some(now)));
    match stage {
        Stage::Won => Ok(Closing {
            closed_at: closed_at(),
            lost_reason: None,
        }),
        Stage::Lost => {
            let sent = lost_reason
                .map(str::trim)
                .filter(|reason| !reason.is_empty());
            let kept = stays.and_then(|(_, closing)| closing.lost_reason.clone());
            match sent.map(str::to_owned).or(kept) {
                Some(reason) => Ok(Closing {
                    closed_at: closed_at(),
                    lost_reason: Some(reason),
                }),
                None => Err(reason_required()),
            }
        }
        _ => Ok(Closing::OPEN),
    }
}

/// The 400 for a lost deal without a reason.
pub(crate) fn reason_required() -> ValidationErrors {
    let mut errors = ValidationErrors::new();
    errors.add("lost_reason", REASON_REQUIRED);
    errors
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, hour, 0, 0).unwrap()
    }

    fn closed(hour: u32, reason: Option<&str>) -> Closing {
        Closing {
            closed_at: Some(at(hour)),
            lost_reason: reason.map(str::to_owned),
        }
    }

    #[test]
    fn open_stages_reopen_the_deal() {
        let lost = closed(9, Some("No budget"));
        for stage in [
            Stage::Lead,
            Stage::Qualified,
            Stage::Proposal,
            Stage::Negotiation,
        ] {
            assert_eq!(
                close(Some((Stage::Lost, &lost)), stage, Some("ignored"), at(10)),
                Ok(Closing::OPEN)
            );
        }
        assert_eq!(close(None, Stage::Lead, None, at(10)), Ok(Closing::OPEN));
    }

    #[test]
    fn winning_closes_now_unless_already_won() {
        assert_eq!(
            close(
                Some((Stage::Negotiation, &Closing::OPEN)),
                Stage::Won,
                None,
                at(10)
            ),
            Ok(closed(10, None))
        );
        assert_eq!(
            close(
                Some((Stage::Won, &closed(9, None))),
                Stage::Won,
                None,
                at(10)
            ),
            Ok(closed(9, None))
        );
        // Lost, then won after all: closed again now, the reason goes.
        assert_eq!(
            close(
                Some((Stage::Lost, &closed(9, Some("No budget")))),
                Stage::Won,
                None,
                at(10)
            ),
            Ok(closed(10, None))
        );
        assert_eq!(close(None, Stage::Won, None, at(10)), Ok(closed(10, None)));
    }

    #[test]
    fn losing_needs_a_reason() {
        for reason in [None, Some(""), Some("   ")] {
            let error = close(
                Some((Stage::Proposal, &Closing::OPEN)),
                Stage::Lost,
                reason,
                at(10),
            )
            .expect_err("a reason is required");
            assert_eq!(
                error.get("lost_reason"),
                Some(&["lost_reason should not be empty".to_owned()][..])
            );
        }
        assert_eq!(
            close(
                Some((Stage::Won, &closed(9, None))),
                Stage::Lost,
                Some(" Went dark "),
                at(10)
            ),
            Ok(closed(10, Some("Went dark")))
        );
    }

    #[test]
    fn a_lost_deal_keeps_its_date_and_reason_unless_given_a_new_one() {
        let lost = closed(9, Some("No budget"));
        assert_eq!(
            close(Some((Stage::Lost, &lost)), Stage::Lost, None, at(10)),
            Ok(closed(9, Some("No budget")))
        );
        assert_eq!(
            close(
                Some((Stage::Lost, &lost)),
                Stage::Lost,
                Some("Chose a rival"),
                at(10)
            ),
            Ok(closed(9, Some("Chose a rival")))
        );
    }
}
