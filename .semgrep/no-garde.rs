// `semgrep --test` ignores `paths`, so the template rule (the same pattern,
// for `*.rs.j2` files) runs on this fixture too and is annotated with it.
// ruleid: navree-no-garde, navree-no-garde-in-templates
use garde::Validate;
// ruleid: navree-no-garde, navree-no-garde-in-templates
use garde::{Report, Validate as _};
// ruleid: navree-no-garde, navree-no-garde-in-templates
extern crate garde;
// ok: navree-no-garde, navree-no-garde-in-templates
use navree::validation::Validate;
// ok: navree-no-garde, navree-no-garde-in-templates
use navree_validation::{Violation, Violations};

// A comment that mentions garde::Validate is not a use.
/// Doc text about garde::Report is not a use either.
// ok: navree-no-garde, navree-no-garde-in-templates
const NAME: &str = "garde::Validate";
// ok: navree-no-garde, navree-no-garde-in-templates
const GARDEN: &str = "garden";

// ruleid: navree-no-garde, navree-no-garde-in-templates
#[derive(garde::Validate)]
struct Old {
    // ruleid: navree-no-garde, navree-no-garde-in-templates
    #[garde(length(min = 1))]
    name: String,
}

// ok: navree-no-garde, navree-no-garde-in-templates
#[derive(Validate)]
struct New {
    // ok: navree-no-garde, navree-no-garde-in-templates
    #[validate(length(min = 1))]
    name: String,
}

// ruleid: navree-no-garde, navree-no-garde-in-templates
fn rule(value: &str, _: &()) -> garde::Result {
    // ruleid: navree-no-garde, navree-no-garde-in-templates
    Err(garde::Error::new("no"))
}

// ok: navree-no-garde, navree-no-garde-in-templates
fn gardener(garden_size: u32) -> u32 {
    garden_size
}
