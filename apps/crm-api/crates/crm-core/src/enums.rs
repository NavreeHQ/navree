//! The CRM's closed value sets. Each is stored as a string column (the
//! models declare `#[model(cast = "string_enum")]`, which stores the serde
//! name) and sent over the API as the same string, and parses
//! back with [`FromStr`].

use std::fmt;
use std::str::FromStr;

use navree::openapi::ToSchema;
use serde::{Deserialize, Serialize};

/// A value that is not one of an enum's strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownValue {
    /// The enum, e.g. `stage`.
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
}

impl fmt::Display for UnknownValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} `{}`", self.kind, self.value)
    }
}

impl std::error::Error for UnknownValue {}

/// Declares a string enum: serde and OpenAPI with the contract strings
/// (which the ORM stores), `ALL` in declaration order, `as_str`, `Display`
/// and `FromStr`. With `schema = Name` the OpenAPI component is called
/// `Name` instead of the enum's name.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $name:ident, $kind:literal $(, schema = $schema:ident)? {
            $( $(#[$vmeta:meta])* $variant:ident => $value:tt ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize, ToSchema,
        )]
        $(#[schema(as = $schema)])?
        pub enum $name {
            $(
                $(#[$vmeta])*
                #[serde(rename = $value)]
                $variant,
            )+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The string stored and sent for this value.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $value,)+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = UnknownValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                $name::ALL
                    .iter()
                    .copied()
                    .find(|candidate| candidate.as_str() == value)
                    .ok_or_else(|| UnknownValue {
                        kind: $kind,
                        value: value.to_owned(),
                    })
            }
        }
    };
}

string_enum! {
    /// A company's head count (`companies.size`).
    CompanySize, "company size" {
        /// 1 to 10 people.
        Tiny => "1-10",
        /// 11 to 50 people.
        Small => "11-50",
        /// 51 to 200 people.
        Medium => "51-200",
        /// 201 to 1000 people.
        Large => "201-1000",
        /// More than 1000 people.
        Huge => "1000+",
    }
}

string_enum! {
    /// Where a contact is in the customer life cycle (`contacts.status`).
    #[derive(Default)]
    ContactStatus, "contact status" {
        /// Not qualified yet (the default).
        #[default]
        Lead => "lead",
        /// Qualified, talking to us.
        Prospect => "prospect",
        /// Buys from us.
        Customer => "customer",
        /// Stopped buying.
        Churned => "churned",
    }
}

string_enum! {
    /// A deal's pipeline stage (`deals.stage`), in board order; new deals
    /// start as `lead`.
    #[derive(Default)]
    Stage, "stage", schema = DealStage {
        /// 10% (the default).
        #[default]
        Lead => "lead",
        /// 25%.
        Qualified => "qualified",
        /// 50%.
        Proposal => "proposal",
        /// 75%.
        Negotiation => "negotiation",
        /// Closed won, 100%.
        Won => "won",
        /// Closed lost, 0%.
        Lost => "lost",
    }
}

impl Stage {
    /// The chance of winning a deal in this stage, in percent.
    pub const fn probability(self) -> u8 {
        match self {
            Stage::Lead => 10,
            Stage::Qualified => 25,
            Stage::Proposal => 50,
            Stage::Negotiation => 75,
            Stage::Won => 100,
            Stage::Lost => 0,
        }
    }

    /// The column title on the board.
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Lead => "Lead",
            Stage::Qualified => "Qualified",
            Stage::Proposal => "Proposal",
            Stage::Negotiation => "Negotiation",
            Stage::Won => "Won",
            Stage::Lost => "Lost",
        }
    }

    /// Won or lost: the deal is closed (`closed_at` is set).
    pub const fn is_closed(self) -> bool {
        matches!(self, Stage::Won | Stage::Lost)
    }
}

string_enum! {
    /// What an activity records (`activities.kind`); only tasks have due
    /// dates that matter and can be completed.
    #[derive(Default)]
    ActivityKind, "activity kind" {
        /// A note (the default: the plainest kind).
        #[default]
        Note => "note",
        /// A phone call.
        Call => "call",
        /// An email.
        Email => "email",
        /// A meeting.
        Meeting => "meeting",
        /// A to-do with a due date.
        Task => "task",
    }
}

// Each value is the morph name of a model (`#[model(morph_name = ..)]`):
// the one an activity's `subject_record` relation loads. The doc comment
// below is the OpenAPI description, which the API contract keeps.
string_enum! {
    /// The kind of record an activity is attached to
    /// (`activities.subject_type`).
    #[derive(Default)]
    SubjectType, "subject type", schema = ActivitySubjectType {
        /// A company.
        Company => "company",
        /// A contact.
        Contact => "contact",
        /// A deal (the default: most activities are about deals).
        #[default]
        Deal => "deal",
    }
}

impl SubjectType {
    /// The access subject (CASL subject type) of records of this type.
    pub const fn subject(self) -> &'static str {
        match self {
            SubjectType::Company => crate::subjects::COMPANY,
            SubjectType::Contact => crate::subjects::CONTACT,
            SubjectType::Deal => crate::subjects::DEAL,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn stages_run_in_board_order_with_their_probabilities_and_labels() {
        let stages: Vec<(&str, &str, u8)> = Stage::ALL
            .iter()
            .map(|stage| (stage.as_str(), stage.label(), stage.probability()))
            .collect();
        assert_eq!(
            stages,
            [
                ("lead", "Lead", 10),
                ("qualified", "Qualified", 25),
                ("proposal", "Proposal", 50),
                ("negotiation", "Negotiation", 75),
                ("won", "Won", 100),
                ("lost", "Lost", 0),
            ]
        );
        assert!(Stage::Won.is_closed() && Stage::Lost.is_closed());
        assert!(Stage::ALL[..4].iter().all(|stage| !stage.is_closed()));
    }

    #[test]
    fn values_serialize_as_the_contract_strings() {
        assert_eq!(json!(Stage::Negotiation), json!("negotiation"));
        assert_eq!(json!(CompanySize::Huge), json!("1000+"));
        assert_eq!(json!(CompanySize::Small), json!("11-50"));
        assert_eq!(json!(ContactStatus::default()), json!("lead"));
        assert_eq!(json!(ActivityKind::Meeting), json!("meeting"));
        assert_eq!(json!(SubjectType::Deal), json!("deal"));
        let size: CompanySize = serde_json::from_value(json!("201-1000")).unwrap();
        assert_eq!(size, CompanySize::Large);
        assert!(serde_json::from_value::<Stage>(json!("closed")).is_err());
    }

    #[test]
    fn values_parse_from_their_strings() {
        assert_eq!("won".parse::<Stage>(), Ok(Stage::Won));
        assert_eq!("1-10".parse::<CompanySize>(), Ok(CompanySize::Tiny));
        assert_eq!(
            "churned".parse::<ContactStatus>(),
            Ok(ContactStatus::Churned)
        );
        assert_eq!("task".parse::<ActivityKind>(), Ok(ActivityKind::Task));
        assert_eq!("company".parse::<SubjectType>(), Ok(SubjectType::Company));
        assert!("Deal".parse::<SubjectType>().is_err());
        assert_eq!(Stage::Proposal.to_string(), "proposal");
    }

    /// The OpenAPI document must list the strings the API sends, not the
    /// Rust variant names (crm-web generates its types from it).
    #[test]
    fn openapi_schemas_list_the_contract_strings() {
        fn values<T: navree::openapi::ToSchema>() -> serde_json::Value {
            serde_json::to_value(T::schema()).unwrap()["enum"].clone()
        }
        assert_eq!(
            values::<CompanySize>(),
            json!(["1-10", "11-50", "51-200", "201-1000", "1000+"])
        );
        assert_eq!(
            values::<ContactStatus>(),
            json!(["lead", "prospect", "customer", "churned"])
        );
        assert_eq!(
            values::<Stage>(),
            json!([
                "lead",
                "qualified",
                "proposal",
                "negotiation",
                "won",
                "lost"
            ])
        );
        assert_eq!(
            values::<ActivityKind>(),
            json!(["note", "call", "email", "meeting", "task"])
        );
        assert_eq!(
            values::<SubjectType>(),
            json!(["company", "contact", "deal"])
        );
    }

    #[test]
    fn activity_subject_types_name_their_access_subjects() {
        assert_eq!(SubjectType::Company.subject(), "Company");
        assert_eq!(SubjectType::Contact.subject(), "Contact");
        assert_eq!(SubjectType::Deal.subject(), "Deal");
    }
}
