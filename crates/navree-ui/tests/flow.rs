//! The flow parts: the wizard (its steps, its draft, every way a post can go
//! wrong), the stepper in a wizard, and the slider, tree and toolbar as a page
//! uses them.

use navree_ui::flow::{
    Draft, StepState, Wizard, WizardAction, WizardError, WizardOutcome, WizardRefusal, WizardStep,
};
use navree_ui::{Form, Options, Uses, Validate, measure, render};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    #[option(label = "A new customer")]
    New,
    #[option(label = "An existing customer")]
    Existing,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Basics {
    /// Deal name
    #[validate(length(min = 1, max = 60))]
    #[form(error = "Give the deal a name.")]
    title: String,
    /// Who is it for
    #[validate(skip)]
    #[form(widget = select, prompt = "Choose")]
    kind: Kind,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Contact {
    /// Email
    #[validate(email)]
    #[form(widget = email)]
    email: String,
    /// Password
    #[validate(length(min = 8))]
    #[form(widget = password)]
    password: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Extras {
    /// Notes
    #[validate(length(max = 200))]
    #[form(widget = textarea)]
    notes: String,
    /// Send me updates
    #[validate(skip)]
    news: bool,
}

/// A step with a text and no rule on it, so that what is typed in it always passes.
#[derive(Debug, Deserialize, Validate, Form)]
struct Free {
    /// Notes
    #[validate(skip)]
    #[form(widget = textarea)]
    notes: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Terms {
    /// I accept
    #[validate(custom(accepted))]
    accept: bool,
}

fn accepted(value: &bool) -> Result<(), String> {
    if *value {
        Ok(())
    } else {
        Err("Accept the terms.".to_owned())
    }
}

fn wizard() -> Wizard {
    Wizard::new("deal")
        .base("/deals/new")
        .step(WizardStep::of::<Basics>("basics", "Basics").description("What and for whom"))
        .step(WizardStep::of::<Contact>("contact", "Contact"))
        .step(WizardStep::of::<Extras>("extras", "Extras").optional())
        .step(WizardStep::of::<Terms>("terms", "Terms"))
        .review()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

/// A post of `fields` to `step` with `action`, from the draft as it stands.
fn post(
    wizard: &Wizard,
    draft: &mut Draft,
    step: &str,
    action: &str,
    fields: &[(&str, &str)],
) -> WizardOutcome {
    let rev = draft.revision.to_string();
    let mut all = vec![("_rev", rev.as_str()), ("_wizard", action)];
    all.extend_from_slice(fields);
    wizard.submit(draft, step, &pairs(&all))
}

const BASICS: &[(&str, &str)] = &[("title", "Annual plan"), ("kind", "new")];
const CONTACT: &[(&str, &str)] = &[("email", "anu@example.com"), ("password", "correct horse")];
const TERMS: &[(&str, &str)] = &[("accept", "true")];

/// A draft that has done every step up to the review.
fn finished_steps(wizard: &Wizard) -> Draft {
    let mut draft = wizard.start();
    assert_eq!(
        post(wizard, &mut draft, "basics", "next", BASICS),
        WizardOutcome::Go("contact".into())
    );
    assert_eq!(
        post(wizard, &mut draft, "contact", "next", CONTACT),
        WizardOutcome::Go("extras".into())
    );
    assert_eq!(
        post(wizard, &mut draft, "extras", "next", &[("notes", "")]),
        WizardOutcome::Go("terms".into())
    );
    assert_eq!(
        post(wizard, &mut draft, "terms", "next", TERMS),
        WizardOutcome::Go("review".into())
    );
    draft
}

fn states(wizard: &Wizard, draft: &Draft, current: &str) -> Vec<StepState> {
    wizard
        .states(draft, current)
        .into_iter()
        .map(|(_, s)| s)
        .collect()
}

#[test]
fn a_new_draft_opens_on_the_first_step_and_nothing_else_can_be_reached() {
    let wizard = wizard();
    let draft = wizard.start();
    assert_eq!(draft.step, "basics");
    assert_eq!(
        states(&wizard, &draft, "basics"),
        [
            StepState::Current,
            StepState::Disabled,
            StepState::Disabled,
            StepState::Disabled,
            StepState::Disabled
        ]
    );
    assert!(wizard.page(&draft, "basics").is_ok());
    assert_eq!(
        wizard.page(&draft, "terms").unwrap_err(),
        WizardError::NotReachable
    );
    assert_eq!(
        wizard.page(&draft, "nope").unwrap_err(),
        WizardError::UnknownStep
    );
    assert_eq!(wizard.entry(&draft), "basics");
}

#[test]
fn next_checks_the_step_and_opens_the_next() {
    let wizard = wizard();
    let mut draft = wizard.start();
    let out = post(&wizard, &mut draft, "basics", "next", BASICS);
    assert_eq!(out, WizardOutcome::Go("contact".into()));
    assert!(out.changed());
    assert_eq!(draft.step, "contact");
    assert_eq!(draft.revision, 1);
    assert_eq!(draft.value("basics", "title"), Some("Annual plan"));
    let s = states(&wizard, &draft, "contact");
    assert_eq!(s[0], StepState::Done);
    assert_eq!(s[1], StepState::Current);
    // Only the step after the one just done is open.
    assert_eq!(s[2], StepState::Disabled);
}

#[test]
fn next_with_a_bad_step_keeps_what_was_typed_and_shows_the_errors() {
    let wizard = wizard();
    let mut draft = wizard.start();
    let out = post(
        &wizard,
        &mut draft,
        "basics",
        "next",
        &[("title", ""), ("kind", "new")],
    );
    assert_eq!(out, WizardOutcome::Invalid("basics".into()));
    assert_eq!(draft.step, "basics");
    let html = render(&wizard.page(&draft, "basics").unwrap().into());
    assert!(html.contains("Give the deal a name."), "{html}");
    assert!(html.contains(r#"data-state="current""#), "{html}");
    // The next step stays shut.
    assert_eq!(
        wizard.page(&draft, "contact").unwrap_err(),
        WizardError::NotReachable
    );
    // Typing it right opens it.
    let out = post(&wizard, &mut draft, "basics", "next", BASICS);
    assert_eq!(out, WizardOutcome::Go("contact".into()));
}

#[test]
fn a_step_that_was_not_tried_shows_no_errors() {
    let wizard = wizard();
    let draft = wizard.start();
    let html = render(&wizard.page(&draft, "basics").unwrap().into());
    assert!(!html.contains("Give the deal a name."), "{html}");
    assert!(!html.contains("data-invalid"), "{html}");
}

#[test]
fn back_and_the_stepper_keep_input_without_checking_it() {
    let wizard = wizard();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    // Half a step typed, then Back: kept, though not valid.
    let out = post(&wizard, &mut draft, "contact", "back", &[("email", "anu@")]);
    assert_eq!(out, WizardOutcome::Go("basics".into()));
    assert_eq!(draft.value("contact", "email"), Some("anu@"));
    // The step was not tried, so it shows no error when it opens again.
    let html = render(&wizard.page(&draft, "contact").unwrap().into());
    assert!(html.contains(r#"value="anu@""#), "{html}");
    assert!(!html.contains("data-invalid"), "{html}");
    // The stepper moves to a step the person has been to.
    let out = post(&wizard, &mut draft, "basics", "goto:contact", BASICS);
    assert_eq!(out, WizardOutcome::Go("contact".into()));
    assert_eq!(draft.step, "contact");
}

#[test]
fn a_step_posted_out_of_order_is_refused_and_changes_nothing() {
    let wizard = wizard();
    let mut draft = wizard.start();
    let before = draft.clone();
    let out = post(&wizard, &mut draft, "terms", "next", TERMS);
    assert_eq!(out, WizardOutcome::Refused(WizardRefusal::NotReachable));
    assert!(!out.changed());
    assert_eq!(draft, before);
    // Going to a step that isn't open yet keeps the typing and goes nowhere.
    let out = post(&wizard, &mut draft, "basics", "goto:terms", BASICS);
    assert_eq!(out, WizardOutcome::Go("basics".into()));
    assert_eq!(draft.value("basics", "title"), Some("Annual plan"));
}

#[test]
fn unknown_steps_and_actions_and_hostile_ones_are_refused() {
    let wizard = wizard();
    let mut draft = wizard.start();
    let before = draft.clone();
    for step in ["", "nope", "../etc", "basics ", "review", "BASICS", "\u{0}"] {
        let out = post(&wizard, &mut draft, step, "next", BASICS);
        assert!(
            matches!(
                out,
                WizardOutcome::Refused(WizardRefusal::UnknownStep | WizardRefusal::NotReachable)
            ),
            "{step:?} {out:?}"
        );
    }
    for action in [
        "",
        "NEXT",
        "next ",
        "goto:",
        "goto:../x",
        "goto:nope",
        "finish",
        "run:rm",
        "back\0",
    ] {
        let out = post(&wizard, &mut draft, "basics", action, BASICS);
        assert!(
            matches!(out, WizardOutcome::Refused(_)),
            "{action:?} {out:?}"
        );
    }
    // No action at all, no revision at all.
    assert_eq!(
        wizard.submit(&mut draft, "basics", &pairs(&[("_rev", "0")])),
        WizardOutcome::Refused(WizardRefusal::UnknownAction)
    );
    assert_eq!(
        wizard.submit(&mut draft, "basics", &pairs(&[("_wizard", "next")])),
        WizardOutcome::Stale
    );
    assert_eq!(draft, before);
}

#[test]
fn a_post_from_an_older_revision_is_stale_and_changes_nothing() {
    let wizard = wizard();
    let mut draft = wizard.start();
    let form_in_window_a = draft.revision;
    post(&wizard, &mut draft, "basics", "next", BASICS);
    let before = draft.clone();
    let out = wizard.submit(
        &mut draft,
        "basics",
        &pairs(&[
            ("_rev", &form_in_window_a.to_string()),
            ("_wizard", "next"),
            ("title", "Other"),
            ("kind", "new"),
        ]),
    );
    assert_eq!(out, WizardOutcome::Stale);
    assert!(!out.changed());
    assert_eq!(draft, before);
    let html = render(&wizard.page(&draft, "contact").unwrap().stale().into());
    assert!(html.contains("changed in another window"), "{html}");
    // A revision that is not a number is stale too.
    for rev in ["", "-1", "1.5", "x", "99999999999999999999999"] {
        let out = wizard.submit(
            &mut draft,
            "contact",
            &pairs(&[("_rev", rev), ("_wizard", "next")]),
        );
        assert_eq!(out, WizardOutcome::Stale, "{rev:?}");
    }
}

#[test]
fn finish_checks_every_step_again_and_sends_the_person_to_the_one_that_fails() {
    let wizard = wizard();
    let mut draft = finished_steps(&wizard);
    assert!(
        states(&wizard, &draft, "review")
            .iter()
            .take(4)
            .all(|s| *s == StepState::Done || *s == StepState::Upcoming)
    );
    // A rule that no longer holds: the stored answer goes bad after the person left the step.
    draft.steps.get_mut("basics").unwrap().values = pairs(&[("title", ""), ("kind", "new")]);
    draft.revision += 1;
    // The review can't be reached with a bad step before it.
    assert_eq!(
        wizard.page(&draft, "review").unwrap_err(),
        WizardError::NotReachable
    );
    let out = post(&wizard, &mut draft, "review", "finish", &[]);
    assert_eq!(out, WizardOutcome::Refused(WizardRefusal::NotReachable));
    // From the last form step (terms) the same bad step is found.
    let mut free = Wizard::new("deal")
        .free()
        .step(WizardStep::of::<Basics>("basics", "Basics"))
        .step(WizardStep::of::<Contact>("contact", "Contact"))
        .step(WizardStep::of::<Terms>("terms", "Terms"));
    free = free.base("/x");
    let mut d = free.start();
    post(&free, &mut d, "basics", "next", BASICS);
    let out = post(&free, &mut d, "terms", "finish", TERMS);
    assert_eq!(out, WizardOutcome::Invalid("contact".into()));
    assert_eq!(d.step, "contact");
    assert!(!d.submitted);
    let html = render(&free.page(&d, "contact").unwrap().into());
    assert!(
        html.contains("data-invalid") || html.contains("field-error"),
        "{html}"
    );
    // The stepper shows both the step to fix and the ones that are done.
    let s = states(&free, &d, "contact");
    assert_eq!(s[0], StepState::Done);
    assert_eq!(s[1], StepState::Current);
    assert_eq!(s[2], StepState::Done);
}

#[test]
fn finishing_gives_done_once_and_a_second_post_is_refused() {
    let wizard = wizard();
    let mut draft = finished_steps(&wizard);
    let out = post(&wizard, &mut draft, "review", "finish", &[]);
    assert_eq!(out, WizardOutcome::Done);
    assert!(draft.submitted);
    // Posted again, from the same page: no second Done.
    let again = wizard.submit(
        &mut draft.clone(),
        "review",
        &pairs(&[
            ("_rev", &(draft.revision - 1).to_string()),
            ("_wizard", "finish"),
        ]),
    );
    assert_eq!(
        again,
        WizardOutcome::Refused(WizardRefusal::AlreadySubmitted)
    );
    let again = post(&wizard, &mut draft, "review", "finish", &[]);
    assert_eq!(
        again,
        WizardOutcome::Refused(WizardRefusal::AlreadySubmitted)
    );
    // The typed values read back from the draft.
    let basics: Basics = draft.read("basics").unwrap();
    assert_eq!(basics.title, "Annual plan");
    assert_eq!(basics.kind, Kind::New);
    // If creating the record failed, the app opens the draft again.
    draft.reopen();
    assert_eq!(
        post(&wizard, &mut draft, "review", "finish", &[]),
        WizardOutcome::Done
    );
}

#[test]
fn an_optional_step_can_be_skipped_but_not_left_bad() {
    let wizard = wizard();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    // Skipped: the step after it opens.
    assert_eq!(states(&wizard, &draft, "extras")[3], StepState::Upcoming);
    assert!(wizard.page(&draft, "terms").is_ok());
    post(
        &wizard,
        &mut draft,
        "extras",
        "goto:terms",
        &[("notes", "x")],
    );
    post(&wizard, &mut draft, "terms", "next", TERMS);
    assert_eq!(
        post(&wizard, &mut draft, "review", "finish", &[]),
        WizardOutcome::Done
    );
    // Visited with a bad answer, it blocks.
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    let too_long = "x".repeat(201);
    assert_eq!(
        post(
            &wizard,
            &mut draft,
            "extras",
            "next",
            &[("notes", &too_long)]
        ),
        WizardOutcome::Invalid("extras".into())
    );
    assert_eq!(
        wizard.page(&draft, "terms").unwrap_err(),
        WizardError::NotReachable
    );
}

#[test]
fn only_the_steps_own_fields_are_kept_and_a_password_never_is() {
    let wizard = wizard();
    let mut draft = wizard.start();
    post(
        &wizard,
        &mut draft,
        "basics",
        "next",
        &[
            ("title", "T"),
            ("kind", "new"),
            ("is_admin", "true"),
            ("_csrf", "tok"),
            ("password", "leak"),
        ],
    );
    let record = draft.record("basics").unwrap();
    assert_eq!(record.values, pairs(&[("title", "T"), ("kind", "new")]));
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    let record = draft.record("contact").unwrap();
    assert_eq!(record.values, pairs(&[("email", "anu@example.com")]));
    let json = serde_json::to_string(&draft).unwrap();
    assert!(
        !json.contains("correct horse") && !json.contains("leak") && !json.contains("tok"),
        "{json}"
    );
    // A password step can never be complete from a stored draft: the person types it again.
    assert!(draft.read::<Contact>("contact").is_err());
}

#[test]
fn what_a_password_got_wrong_is_said_again_in_words_and_never_in_value() {
    let wizard = wizard();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    // A short password: the step is not done, and the page says why, once the value is gone.
    let out = post(
        &wizard,
        &mut draft,
        "contact",
        "next",
        &[("email", "anu@example.com"), ("password", "short")],
    );
    assert_eq!(out, WizardOutcome::Invalid("contact".into()));
    let json = serde_json::to_string(&draft).unwrap();
    assert!(!json.contains("short\""), "the value is not kept: {json}");
    let html = render(&wizard.page(&draft, "contact").unwrap().into());
    assert!(html.contains("longer than or equal to 8"), "{html}");
    assert!(html.contains(r#"value="anu@example.com""#), "{html}");
    // Moving away and back without typing it again keeps the words; typing it right clears them.
    post(
        &wizard,
        &mut draft,
        "contact",
        "back",
        &[("email", "anu@example.com")],
    );
    let html = render(&wizard.page(&draft, "contact").unwrap().into());
    assert!(html.contains("longer than or equal to 8"), "{html}");
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    let html = render(&wizard.page(&draft, "contact").unwrap().into());
    assert!(!html.contains("longer than or equal to 8"), "{html}");
    // Changing the email without a password drops the old words: nothing says what a password it hasn't seen got wrong.
    post(
        &wizard,
        &mut draft,
        "contact",
        "back",
        &[("email", "gauri@example.com")],
    );
    let html = render(&wizard.page(&draft, "contact").unwrap().into());
    assert!(!html.contains("longer than or equal to 8"), "{html}");
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Account {
    /// Email
    #[validate(email)]
    #[form(widget = email)]
    email: String,
    /// Password
    #[validate(length(min = 8), custom(not_common))]
    #[form(widget = password)]
    password: String,
    /// Again
    #[validate(skip)]
    #[form(widget = password)]
    again: String,
}

/// A rule that says the value back, as a message often does.
fn not_common(value: &String) -> Result<(), String> {
    if value == "hunter2hunter2" {
        Err(format!("{value} is on the list of common passwords."))
    } else {
        Ok(())
    }
}

#[test]
fn a_rule_that_says_a_password_back_has_it_masked_before_the_words_are_kept() {
    let wizard = Wizard::new("account")
        .base("/join")
        .step(WizardStep::of::<Account>("account", "Account"))
        .review();
    let mut draft = wizard.start();
    let out = post(
        &wizard,
        &mut draft,
        "account",
        "next",
        &[
            ("email", "anu@example.com"),
            ("password", "hunter2hunter2"),
            ("again", "hunter2hunter2"),
        ],
    );
    assert_eq!(out, WizardOutcome::Invalid("account".into()));
    let json = serde_json::to_string(&draft).unwrap();
    assert!(
        !json.contains("hunter2hunter2"),
        "the value is not kept: {json}"
    );
    // The words are still said, with the value hidden.
    let html = render(&wizard.page(&draft, "account").unwrap().into());
    assert!(
        html.contains("is on the list of common passwords."),
        "{html}"
    );
    assert!(!html.contains("hunter2hunter2"), "{html}");
    // A password inside the words of another field's rule is masked as well.
    let words = &draft.record("account").unwrap().secret_errors;
    assert!(
        words
            .values()
            .flatten()
            .all(|text| !text.contains("hunter2")),
        "{words:?}"
    );
}

#[test]
fn a_post_over_the_limits_is_refused_and_leaves_the_draft_as_it_was() {
    let wizard = wizard();
    let mut draft = wizard.start();
    let before = draft.clone();
    let huge = "x".repeat(70 * 1024);
    let out = post(&wizard, &mut draft, "basics", "next", &[("title", &huge)]);
    assert_eq!(out, WizardOutcome::Refused(WizardRefusal::TooLarge));
    assert_eq!(draft, before);
    let many: Vec<(String, String)> = (0..1001)
        .map(|_| ("title".to_owned(), "a".to_owned()))
        .collect();
    let mut all = pairs(&[("_rev", "0"), ("_wizard", "next")]);
    all.extend(many);
    assert_eq!(
        wizard.submit(&mut draft, "basics", &all),
        WizardOutcome::Refused(WizardRefusal::TooLarge)
    );
    assert_eq!(draft, before);
    // Many steps each just under the value limit pass the total.
    let mut big = Wizard::new("big").base("/b");
    for n in 0..6 {
        big = big.step(WizardStep::of::<Free>(format!("s{n}"), format!("S{n}")).optional());
    }
    let mut d = big.start();
    let chunk = "y".repeat(50 * 1024);
    let mut refused = false;
    for n in 0..6 {
        let out = post(
            &big,
            &mut d,
            &format!("s{n}"),
            "goto:s5",
            &[("notes", &chunk)],
        );
        if out == WizardOutcome::Refused(WizardRefusal::TooLarge) {
            refused = true;
            break;
        }
    }
    assert!(refused);
    assert!(d.bytes() <= navree_ui::flow::MAX_DRAFT_BYTES);
}

#[test]
fn an_optional_step_with_an_answer_is_checked_whichever_way_it_was_left() {
    let long = "n".repeat(300);
    for action in ["back", "goto:terms", "save"] {
        let wizard = wizard();
        let mut draft = wizard.start();
        post(&wizard, &mut draft, "basics", "next", BASICS);
        post(&wizard, &mut draft, "contact", "next", CONTACT);
        post(&wizard, &mut draft, "extras", action, &[("notes", &long)]);
        // The answer is kept, counts as tried, and the rules refuse it: the stepper says so, the
        // steps after it are closed, and the step says what is wrong when it is opened again.
        assert_eq!(draft.value("extras", "notes").map(str::len), Some(300));
        assert_eq!(
            states(&wizard, &draft, "contact")[2],
            StepState::Error,
            "{action}"
        );
        assert_eq!(
            wizard.page(&draft, "terms").unwrap_err(),
            WizardError::NotReachable,
            "{action}"
        );
        let html = render(&wizard.page(&draft, "extras").unwrap().into());
        assert!(
            html.contains("shorter than or equal to 200"),
            "{action}: {html}"
        );
    }
    // An answer that holds is as good as Next: the steps after it open, and it is Done.
    let wizard = wizard();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    post(
        &wizard,
        &mut draft,
        "extras",
        "goto:terms",
        &[("notes", "fine")],
    );
    assert_eq!(draft.step, "terms");
    assert_eq!(states(&wizard, &draft, "terms")[2], StepState::Done);
    // An optional step with nothing in it is skipped, whichever way it was left.
    let wizard = self::wizard();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    post(
        &wizard,
        &mut draft,
        "extras",
        "goto:terms",
        &[("notes", "  ")],
    );
    assert_eq!(draft.step, "terms");
    assert_eq!(states(&wizard, &draft, "terms")[2], StepState::Upcoming);
}

#[test]
fn finish_and_the_review_check_an_optional_answer_the_person_left_by_back() {
    // In a wizard that can be left in any order, the review opens whatever the steps hold.
    let long = "n".repeat(300);
    let wizard = self::wizard().free();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    post(&wizard, &mut draft, "extras", "back", &[("notes", &long)]);
    post(
        &wizard,
        &mut draft,
        "contact",
        "goto:terms",
        &[("email", "anu@example.com")],
    );
    post(&wizard, &mut draft, "terms", "next", TERMS);
    let html = render(&wizard.page(&draft, "review").unwrap().into());
    let words = "This step needs attention.";
    assert_eq!(html.matches(words).count(), 1, "one step is flagged");
    assert!(
        html.find(words) > html.find("review-extras")
            && html.find(words) < html.find("review-terms"),
        "and it is the optional one"
    );
    assert_eq!(
        post(&wizard, &mut draft, "review", "finish", &[]),
        WizardOutcome::Invalid("extras".into())
    );
    assert!(!draft.submitted);
    // Corrected, it finishes.
    post(
        &wizard,
        &mut draft,
        "extras",
        "goto:review",
        &[("notes", "short")],
    );
    assert_eq!(
        post(&wizard, &mut draft, "review", "finish", &[]),
        WizardOutcome::Done
    );
    // Left empty it is skipped and nothing is flagged.
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(&wizard, &mut draft, "contact", "next", CONTACT);
    post(&wizard, &mut draft, "extras", "back", &[("notes", "")]);
    post(
        &wizard,
        &mut draft,
        "contact",
        "goto:terms",
        &[("email", "anu@example.com")],
    );
    post(&wizard, &mut draft, "terms", "next", TERMS);
    let html = render(&wizard.page(&draft, "review").unwrap().into());
    assert!(
        !html.contains("This step needs attention."),
        "nothing is flagged"
    );
    assert_eq!(
        post(&wizard, &mut draft, "review", "finish", &[]),
        WizardOutcome::Done
    );
}

#[test]
fn the_review_marks_a_step_as_not_done_when_the_stepper_does_and_not_otherwise() {
    // A step with a password is as done in the review as in the stepper, though its password is not kept.
    let wizard = wizard();
    let draft = finished_steps(&wizard);
    let html = render(&wizard.page(&draft, "review").unwrap().into());
    assert!(
        !html.contains("needs attention") && !html.contains("data-invalid"),
        "{html}"
    );
    // In a wizard that can be left in any order, the review can open with a step wrong or not tried.
    let wizard = self::wizard().free();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    post(
        &wizard,
        &mut draft,
        "contact",
        "goto:terms",
        &[("email", "anu@example.com"), ("password", "short")],
    );
    post(&wizard, &mut draft, "terms", "next", TERMS);
    let html = render(&wizard.page(&draft, "review").unwrap().into());
    // The contact step was tried and its password was wrong; nothing else is flagged but the one not tried (extras is optional).
    assert_eq!(html.matches("needs attention").count(), 1, "{html}");
    assert!(
        html.find("needs attention") > html.find("review-contact")
            && html.find("needs attention") < html.find("review-extras"),
        "{html}"
    );
}

#[test]
fn the_review_shows_answers_as_text_with_a_button_to_each_step() {
    let wizard = wizard();
    let draft = finished_steps(&wizard);
    let html = render(&wizard.page(&draft, "review").unwrap().into());
    assert!(
        html.contains("A new customer"),
        "option labels, not values: {html}"
    );
    assert!(html.contains("anu@example.com"), "{html}");
    assert!(!html.contains("correct horse"), "{html}");
    assert!(html.contains(r#"value="goto:basics""#), "{html}");
    assert!(html.contains(r#"value="goto:terms""#), "{html}");
    assert!(html.contains(r#"value="finish""#), "{html}");
    assert!(
        !html.contains(r#"value="next""#)
            || html.find(r#"value="finish""#) < html.find(r#"value="next""#),
        "{html}"
    );
    assert!(html.contains("Check your answers"), "{html}");
    // A switch is yes or no.
    assert!(
        html.contains("<dt>Send me updates</dt><dd>No</dd>"),
        "{html}"
    );
}

#[test]
fn a_step_page_is_a_plain_form_that_posts_to_its_own_address() {
    let wizard = wizard();
    let draft = wizard.start();
    let html = render(&wizard.page(&draft, "basics").unwrap().csrf("tok123").into());
    assert!(
        html.contains(&format!(r#"action="/deals/new/{}/basics""#, draft.id)),
        "{html}"
    );
    assert!(html.contains(r#"method="post""#), "{html}");
    assert!(html.contains(r#"name="_csrf" value="tok123""#), "{html}");
    assert!(html.contains(r#"name="_rev" value="0""#), "{html}");
    // The first submit button is the one Enter presses: Next, not a step of the stepper.
    let first = html.find(r#"type="submit""#).unwrap();
    assert!(
        html[first..first + 200].contains(r#"value="next""#),
        "{}",
        &html[first..first + 200]
    );
    assert!(html.contains("Save and leave"), "{html}");
    assert!(
        !html.contains(">Back<"),
        "no Back on the first step: {html}"
    );
    let second = wizard.page(&finished_steps(&wizard), "contact").unwrap();
    assert!(render(&second.into()).contains(">Back<"));
    assert!(!html.contains("style="), "{html}");
}

#[test]
fn the_last_step_without_a_review_finishes() {
    let wizard = Wizard::new("w")
        .base("/w")
        .step(WizardStep::of::<Basics>("only", "Only"));
    let draft = wizard.start();
    let html = render(&wizard.page(&draft, "only").unwrap().into());
    assert!(
        html.contains(r#"value="finish""#) && !html.contains(r#"value="next""#),
        "{html}"
    );
    let mut draft = draft;
    assert_eq!(
        post(&wizard, &mut draft, "only", "next", BASICS),
        WizardOutcome::Refused(WizardRefusal::UnknownAction)
    );
    assert_eq!(
        post(&wizard, &mut draft, "only", "finish", BASICS),
        WizardOutcome::Done
    );
}

#[test]
fn save_and_leave_keeps_the_draft_and_it_resumes_where_it_was() {
    let wizard = wizard();
    let mut draft = wizard.start();
    post(&wizard, &mut draft, "basics", "next", BASICS);
    let out = post(&wizard, &mut draft, "contact", "save", &[("email", "half")]);
    assert_eq!(out, WizardOutcome::Left);
    assert_eq!(draft.step, "contact");
    assert_eq!(wizard.entry(&draft), "contact");
    let json = serde_json::to_string(&draft).unwrap();
    let resumed: Draft = serde_json::from_str(&json).unwrap();
    assert_eq!(wizard.entry(&resumed), "contact");
    // A draft that points somewhere that isn't open resumes at the last open step.
    let mut lost = resumed;
    lost.step = "terms".into();
    assert_eq!(wizard.entry(&lost), "contact");
    lost.step = "garbage".into();
    assert_eq!(wizard.entry(&lost), "contact");
}

#[test]
fn a_free_wizard_opens_every_step_but_still_checks_them_all_to_finish() {
    let wizard = Wizard::new("f")
        .base("/f")
        .free()
        .step(WizardStep::of::<Basics>("a", "A"))
        .step(WizardStep::of::<Terms>("b", "B"))
        .review();
    let mut draft = wizard.start();
    assert!(wizard.page(&draft, "b").is_ok());
    assert!(wizard.page(&draft, "review").is_ok());
    assert_eq!(
        post(&wizard, &mut draft, "review", "finish", &[]),
        WizardOutcome::Invalid("a".into())
    );
}

#[test]
fn ids_are_cleaned_and_made_unique_and_review_is_taken() {
    let wizard = Wizard::new("a b<c>")
        .step(WizardStep::of::<Basics>("one two", "A"))
        .step(WizardStep::of::<Basics>("one two", "B"))
        .step(WizardStep::of::<Basics>("review", "C"))
        .step(WizardStep::of::<Basics>("", "D"))
        .step(WizardStep::of::<Basics>("x".repeat(100), "E"))
        .review();
    let ids: Vec<String> = wizard
        .states(&wizard.start(), "one-two")
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "{ids:?}");
    assert!(
        ids.iter().all(|id| navree_ui::flow::is_step_id(id)),
        "{ids:?}"
    );
    assert_eq!(
        ids.iter().filter(|id| *id == "review").count(),
        1,
        "{ids:?}"
    );
}

#[test]
fn a_wizard_of_one_hundred_steps_and_one_past_it() {
    let mut wizard = Wizard::new("many").base("/m");
    for n in 0..120 {
        wizard = wizard.step(WizardStep::of::<Terms>(
            format!("s{n}"),
            format!("Step {n}"),
        ));
    }
    assert_eq!(wizard.len(), 100);
    let draft = wizard.start();
    let html = render(&wizard.page(&draft, "s0").unwrap().into());
    assert_eq!(html.matches("data-slot=\"step\"").count(), 100);
    assert!(Wizard::new("none").is_empty());
    assert!(!Wizard::new("none").start().step.is_empty());
}

#[test]
fn a_wizard_page_loads_the_core_runtime_and_nothing_else() {
    let wizard = wizard();
    let node: navree_ui::Node = wizard.page(&wizard.start(), "basics").unwrap().into();
    assert_eq!(measure(&node).uses, Uses::CORE);
}

#[test]
fn hostile_text_in_titles_and_values_is_text() {
    let wizard = Wizard::new("w").base("/w").step(
        WizardStep::of::<Basics>("a", "<script>alert(1)</script>")
            .description("\"><img src=x onerror=y>"),
    );
    let mut draft = wizard.start();
    post(
        &wizard,
        &mut draft,
        "a",
        "save",
        &[("title", "\"><script>x</script>"), ("kind", "new")],
    );
    let html = render(&wizard.page(&draft, "a").unwrap().notice("<b>n</b>").into());
    assert!(
        !html.contains("<script>") && !html.contains("<img") && !html.contains("<b>"),
        "{html}"
    );
}

/// A small generator, so the sequences below are the same everywhere.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a, T>(&mut self, list: &'a [T]) -> &'a T {
        &list[(self.next() % list.len() as u64) as usize]
    }
}

#[test]
fn any_sequence_of_posts_keeps_the_draft_sound() {
    let wizard = wizard();
    let steps = ["basics", "contact", "extras", "terms", "review", "nope"];
    let actions = [
        "next",
        "back",
        "save",
        "finish",
        "goto:basics",
        "goto:contact",
        "goto:extras",
        "goto:terms",
        "goto:review",
        "junk",
    ];
    let field_sets: [&[(&str, &str)]; 6] = [
        BASICS,
        CONTACT,
        TERMS,
        &[("notes", "n")],
        &[("title", ""), ("email", "x")],
        &[],
    ];
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for _ in 0..300 {
        let mut draft = wizard.start();
        for _ in 0..40 {
            let before = draft.clone();
            let step = *rng.pick(&steps);
            let action = *rng.pick(&actions);
            let fields = *rng.pick(&field_sets);
            // Sometimes a stale revision.
            let rev = if rng.next().is_multiple_of(9) {
                draft.revision + 1
            } else {
                draft.revision
            };
            let mut all = vec![
                ("_rev".to_owned(), rev.to_string()),
                ("_wizard".to_owned(), action.to_owned()),
            ];
            all.extend(pairs(fields));
            let out = wizard.submit(&mut draft, step, &all);
            if !out.changed() {
                assert_eq!(draft, before, "{step} {action} {out:?}");
            } else {
                assert_eq!(
                    draft.revision,
                    before.revision + 1,
                    "{step} {action} {out:?}"
                );
            }
            if out == WizardOutcome::Done {
                // Done means every required step really is valid.
                for id in ["basics", "contact", "terms"] {
                    assert!(draft.record(id).is_some_and(|r| r.visited), "{id}");
                }
                assert!(draft.read::<Basics>("basics").is_ok());
                assert!(draft.read::<Terms>("terms").is_ok());
                break;
            }
            // The page for where the draft says it is can always be drawn.
            let entry = wizard.entry(&draft);
            assert!(wizard.page(&draft, &entry).is_ok(), "{entry}");
            let _ = render(&wizard.page(&draft, &entry).unwrap().into());
            assert!(draft.bytes() <= navree_ui::flow::MAX_DRAFT_BYTES);
        }
    }
}

#[test]
fn any_action_text_parses_or_is_nothing_and_what_parses_reads_back() {
    let pieces = [
        "next", "back", "save", "finish", "goto:", "goto:a", "b-1", "_", " ", "", ":", "\u{0}",
        "\u{e9}", "../", "NEXT",
    ];
    let mut rng = Rng(99);
    let mut parsed = 0;
    for _ in 0..200_000 {
        let count = (rng.next() % 4) as usize;
        let text: String = (0..count).map(|_| *rng.pick(&pieces)).collect();
        if let Some(action) = WizardAction::parse(&text) {
            parsed += 1;
            let again = match &action {
                WizardAction::Next => "next".to_owned(),
                WizardAction::Back => "back".to_owned(),
                WizardAction::Save => "save".to_owned(),
                WizardAction::Finish => "finish".to_owned(),
                WizardAction::Goto(id) => format!("goto:{id}"),
            };
            assert_eq!(again, text);
            assert_eq!(WizardAction::parse(&again), Some(action));
        }
    }
    assert!(parsed > 1000, "{parsed}");
}

// ---- A slider in a derived form -------------------------------------------------

fn volume_grid() -> navree_ui::flow::SliderSpec {
    navree_ui::flow::SliderSpec::new(0, 100).step(5)
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Settings {
    /// Volume
    #[validate(range(min = 0, max = 100))]
    #[form(widget = slider, slider = volume_grid)]
    volume: i64,
    /// Quality
    #[validate(range(min = 1, max = 10))]
    #[form(widget = slider)]
    quality: i64,
}

#[test]
fn a_slider_field_draws_its_grid_and_the_server_holds_the_post_to_it() {
    use navree_ui::form::{FormView, decode};
    let html = render(
        &FormView::<Settings>::new()
            .with_values(pairs(&[("volume", "40"), ("quality", "7")]))
            .into(),
    );
    assert!(html.contains(r#"type="range""#), "{html}");
    assert!(
        html.contains(r#"name="volume""#)
            && html.contains(r#"step="5""#)
            && html.contains(r#"value="40""#),
        "{html}"
    );
    // The grid of a field with no function of its own is its range rule, in steps of 1.
    assert!(
        html.contains(r#"name="quality""#)
            && html.contains(r#"min="1" max="10""#)
            && html.contains(r#"value="7""#),
        "{html}"
    );
    let ok: Settings = decode(&pairs(&[("volume", "40"), ("quality", "7")])).unwrap();
    assert_eq!((ok.volume, ok.quality), (40, 7));
    for (volume, quality, field, says) in [
        ("37", "7", "volume", "steps of 5"),
        ("40", "11", "quality", "10 or less"),
        ("40", "0", "quality", "1 or more"),
        ("40", "7.5", "quality", ""),
        ("abc", "7", "volume", ""),
        ("", "7", "volume", ""),
    ] {
        let errors =
            decode::<Settings>(&pairs(&[("volume", volume), ("quality", quality)])).unwrap_err();
        let said = errors
            .get(field)
            .unwrap_or_else(|| panic!("{field}: {errors:?}"))
            .join(" ");
        assert!(said.contains(says), "{volume} {quality}: {said}");
    }
    // A bad post comes back drawn, with its error on the slider and the number it had.
    let again = render(
        &FormView::<Settings>::new()
            .with_values(pairs(&[("volume", "37"), ("quality", "7")]))
            .with_errors(
                decode::<Settings>(&pairs(&[("volume", "37"), ("quality", "7")])).unwrap_err(),
            )
            .into(),
    );
    assert!(again.contains("steps of 5"), "{again}");
}

fn unit_any() -> navree_ui::flow::SliderSpec {
    navree_ui::flow::SliderSpec::new(0, 1).any_step()
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Mixer {
    /// Mix
    #[validate(skip)]
    #[form(widget = slider, slider = unit_any)]
    mix: f64,
}

#[test]
fn a_slider_that_takes_any_value_posts_what_a_drag_lands_on_and_it_is_accepted() {
    use navree_ui::form::{FormView, decode};
    // A browser posts a double written with up to 17 digits, or an exponent for a tiny value.
    let ok: Mixer = decode(&pairs(&[("mix", "0.5033333333333333")])).unwrap();
    assert!((ok.mix - 0.503_333_333_333_333_3).abs() < 1e-12);
    let tiny: Mixer = decode(&pairs(&[("mix", "1e-7")])).unwrap();
    assert!((tiny.mix - 1e-7).abs() < 1e-15);
    // Outside the grid is still refused, and so is what is not a number.
    for (text, says) in [
        ("1.5", "1 or less"),
        ("-0.2", "0 or more"),
        ("NaN", "number"),
    ] {
        let errors = decode::<Mixer>(&pairs(&[("mix", text)])).unwrap_err();
        let said = errors
            .get("mix")
            .unwrap_or_else(|| panic!("{text}: {errors:?}"))
            .join(" ");
        assert!(said.contains(says), "{text}: {said}");
    }
    // A form drawn again after an error keeps the long value on the thumb, not the minimum.
    let again = render(
        &FormView::<Mixer>::new()
            .with_values(pairs(&[("mix", "0.5033333333333333")]))
            .into(),
    );
    assert!(again.contains(r#"value="0.503333333""#), "{again}");
}
