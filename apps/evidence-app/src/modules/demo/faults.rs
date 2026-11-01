//! The errors the failure routes raise. Each is a typed error with a cause
//! below it, worded like what an application reports when a payment, a
//! forecast, a domain, a reminder, a digest or an import goes wrong: a
//! summary of what the app was doing, then the reason.

use std::error::Error;
use std::fmt;

use navree::view::ViewError;

/// Why the payment gateway refused a charge.
#[derive(Debug)]
pub struct GatewayDecline {
    reason: &'static str,
    code: u16,
}

impl fmt::Display for GatewayDecline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "payment gateway declined the charge: {} (decline code {})",
            self.reason, self.code
        )
    }
}

impl Error for GatewayDecline {}

/// A stage whose win probability is outside 0 to 100.
#[derive(Debug)]
pub struct StageRange {
    stage: &'static str,
    probability: i32,
}

impl fmt::Display for StageRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stage \"{}\" has a win probability of {}%, expected 0 to 100",
            self.stage, self.probability
        )
    }
}

impl Error for StageRange {}

/// A value that breaks a unique index.
#[derive(Debug)]
pub struct UniqueViolation {
    constraint: &'static str,
}

impl fmt::Display for UniqueViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "duplicate key value violates unique constraint \"{}\"",
            self.constraint
        )
    }
}

impl Error for UniqueViolation {}

/// What the app was doing when it failed, with the cause.
#[derive(Debug)]
pub enum Fault {
    /// A card charge the gateway refused.
    ChargeDeclined(GatewayDecline),
    /// A forecast over a stage table with an impossible probability.
    ForecastRange(StageRange),
    /// A company domain that another company already holds.
    DomainTaken(UniqueViolation),
    /// A reminder the mail server turned away.
    ReminderRefused(std::io::Error),
    /// A digest whose template does not exist.
    DigestTemplate(ViewError),
    /// An imported invoice line with an amount that is not a number.
    InvoiceLine(std::num::ParseFloatError),
}

impl Fault {
    /// The charge the gateway declined for lack of funds.
    pub fn charge_declined() -> Self {
        Fault::ChargeDeclined(GatewayDecline {
            reason: "insufficient funds",
            code: 51,
        })
    }

    /// A stage with a probability of 140%.
    pub fn forecast_range() -> Self {
        Fault::ForecastRange(StageRange {
            stage: "negotiation",
            probability: 140,
        })
    }

    /// A second company claiming a domain.
    pub fn domain_taken() -> Self {
        Fault::DomainTaken(UniqueViolation {
            constraint: "companies_domain_key",
        })
    }

    /// An outgoing mail server that refuses the connection.
    pub fn reminder_refused() -> Self {
        Fault::ReminderRefused(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused (smtp.internal:587)",
        ))
    }

    /// A template that cannot be found: the error the view layer returns.
    pub fn digest_template(source: ViewError) -> Self {
        Fault::DigestTemplate(source)
    }

    /// An amount typed with a decimal comma and a currency code.
    pub fn invoice_line() -> Self {
        let source = "12,50 EUR"
            .parse::<f64>()
            .expect_err("a decimal comma and a currency code are not a number");
        Fault::InvoiceLine(source)
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Fault::ChargeDeclined(_) => "cannot charge invoice: card declined",
            Fault::ForecastRange(_) => "cannot recalculate the pipeline forecast",
            Fault::DomainTaken(_) => {
                "cannot attach the domain to the company: domain already registered"
            }
            Fault::ReminderRefused(_) => {
                "cannot send the quote reminder: mail server refused the message"
            }
            Fault::DigestTemplate(_) => "cannot render the weekly digest email",
            Fault::InvoiceLine(_) => "cannot import the invoice line: amount is not a number",
        })
    }
}

impl Error for Fault {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Fault::ChargeDeclined(source) => source,
            Fault::ForecastRange(source) => source,
            Fault::DomainTaken(source) => source,
            Fault::ReminderRefused(source) => source,
            Fault::DigestTemplate(source) => source,
            Fault::InvoiceLine(source) => source,
        })
    }
}

/// The error of an outgoing call that answered with a failure status.
#[derive(Debug)]
pub struct UpstreamStatus {
    /// What the app was calling.
    pub service: &'static str,
    /// The status it answered with.
    pub status: u16,
}

impl fmt::Display for UpstreamStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} answered with status {}", self.service, self.status)
    }
}

impl Error for UpstreamStatus {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fault_has_a_cause_below_its_summary() {
        let faults = [
            Fault::charge_declined(),
            Fault::forecast_range(),
            Fault::domain_taken(),
            Fault::reminder_refused(),
            Fault::invoice_line(),
        ];
        let mut summaries = std::collections::BTreeSet::new();
        for fault in &faults {
            let cause = fault.source().expect("a cause");
            assert!(!cause.to_string().is_empty());
            assert!(summaries.insert(fault.to_string()), "{fault} is repeated");
        }
    }
}
