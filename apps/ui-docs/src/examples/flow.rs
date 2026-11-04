//! Stepper, Wizard, Slider, Tree and Toolbar.

use navree::ui::flow::{SliderLabels, SliderSpec, TreeState};
use navree::ui::*;
use serde::Deserialize;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("flow.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("stepper", "thumb", "A stepper", stepper_thumb)
            .start()
            .height(90),
        single!(
            "stepper",
            "default",
            "Steps and their states",
            stepper_default
        )
        .start()
        .height(110),
        single!("stepper", "vertical", "Vertical", stepper_vertical)
            .start()
            .height(300),
        single!("stepper", "links", "Steps that are links", stepper_links)
            .start()
            .height(110),
        single!("stepper", "narrow", "In a narrow box", stepper_narrow)
            .start()
            .width(390)
            .height(120),
        single!("wizard", "thumb", "A wizard step", wizard_thumb)
            .start()
            .width(820)
            .height(420),
        single!("wizard", "default", "A step of a wizard", wizard_default)
            .start()
            .width(820)
            .height(480),
        single!("wizard", "errors", "A step with errors", wizard_errors)
            .start()
            .width(820)
            .height(520),
        single!("wizard", "review", "The review", wizard_review)
            .start()
            .width(820)
            .height(560),
        single!("slider", "thumb", "A slider", slider_thumb)
            .start()
            .height(80),
        single!("slider", "default", "One value", slider_default)
            .start()
            .height(110),
        single!("slider", "range", "A range", slider_range)
            .start()
            .height(130),
        single!("slider", "marks", "Marks", slider_marks)
            .start()
            .height(130),
        single!("slider", "names", "Named steps", slider_names)
            .start()
            .height(130),
        single!("slider", "decimals", "Decimal steps", slider_decimals)
            .start()
            .height(110),
        gallery!("slider", "states", "States", slider_states).medium(),
        single!("slider", "rtl", "Right to left", slider_rtl)
            .start()
            .height(140),
        single!("slider", "check", "Checking a post", slider_check)
            .start()
            .height(110),
        single!("tree-view", "thumb", "A tree", tree_thumb)
            .start()
            .height(150),
        single!("tree-view", "default", "A tree of links", tree_default)
            .start()
            .height(230),
        single!(
            "tree-view",
            "multiple",
            "Choosing many, with parents that follow",
            tree_multiple
        )
        .start()
        .height(250),
        single!("tree-view", "single", "Choosing one", tree_single)
            .start()
            .height(200),
        single!("tree-view", "lazy", "Children from the server", tree_lazy)
            .start()
            .height(150),
        single!("tree-view", "paged", "A long list in pages", tree_paged)
            .start()
            .height(260),
        single!(
            "tree-view",
            "state",
            "Open branches in the address",
            tree_state
        )
        .start()
        .height(220),
        single!("toolbar", "thumb", "A toolbar", toolbar_thumb)
            .start()
            .height(80),
        single!("toolbar", "default", "A toolbar", toolbar_default)
            .start()
            .height(100),
        // Narrow enough that the last tools move into More.
        single!("toolbar", "overflow", "At a narrow width", toolbar_overflow)
            .start()
            .width(300)
            .height(100),
        single!("toolbar", "vertical", "Vertical", toolbar_vertical)
            .start()
            .height(200),
        single!("toolbar", "groups", "Groups and a menu", toolbar_groups)
            .start()
            .height(110),
    ]
}

fn stepper_thumb() -> Stepper {
    Stepper::new("Setup")
        .step(Step::new("Account"))
        .step(Step::new("Team"))
        .step(Step::new("Launch"))
        .current(1)
}

fn stepper_default() -> Stepper {
    Stepper::new("Checkout")
        .step(Step::new("Cart"))
        .step(Step::new("Address"))
        .step(Step::new("Payment").state(StepState::Error))
        .step(Step::new("Review"))
        .step(Step::new("Done").state(StepState::Disabled))
        .current(3)
}

fn stepper_vertical() -> Stepper {
    Stepper::new("Setup")
        .vertical()
        .step(Step::new("Create the account").description("Your name and email"))
        .step(
            Step::new("Invite the team")
                .description("You can do this later")
                .optional(),
        )
        .step(Step::new("Connect the mailbox").state(StepState::Error))
        .step(Step::new("Launch").state(StepState::Disabled))
        .current(1)
}

fn stepper_links() -> Stepper {
    Stepper::new("Order")
        .step(Step::new("Placed").link("/orders/42"))
        .step(Step::new("Packed").link("/orders/42/packed"))
        .step(Step::new("Shipped").link("/orders/42/shipped"))
        .step(Step::new("Delivered"))
        .current(2)
}

fn stepper_narrow() -> Stepper {
    Stepper::new("Setup")
        .step(Step::new("Create the account").description("Your name and email"))
        .step(Step::new("Invite the team"))
        .step(Step::new("Connect the mailbox"))
        .step(Step::new("Launch"))
        .current(1)
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct Basics {
    /// Deal name
    #[validate(length(min = 1, max = 60))]
    #[form(error = "Give the deal a name.")]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct Contact {
    /// Email
    #[validate(email)]
    #[form(widget = email, error = "Enter an email address like harleen@example.com.")]
    pub email: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct Terms {
    /// I accept the terms
    #[validate(custom(accepted))]
    pub accept: bool,
}

fn accepted(value: &bool) -> Result<(), String> {
    if *value {
        Ok(())
    } else {
        Err("Accept the terms to finish.".to_owned())
    }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn wizard_thumb() -> WizardPage {
    let wizard = Wizard::new("thumb")
        .base("/deals/new")
        .step(WizardStep::of::<Basics>("basics", "Basics"))
        .step(WizardStep::of::<Contact>("contact", "Contact"))
        .review();
    wizard
        .page(&wizard.draft("demo-draft-0001"), "basics")
        .expect("the first step opens")
}

fn wizard_default() -> WizardPage {
    let wizard = Wizard::new("deal")
        .base("/deals/new")
        .step(WizardStep::of::<Basics>("basics", "Basics").description("What the deal is"))
        .step(WizardStep::of::<Contact>("contact", "Contact").description("Who to write to"))
        .step(WizardStep::of::<Terms>("terms", "Terms"))
        .review();
    // The first step is done: its answers are in the draft, kept on the server.
    let mut draft = wizard.draft("demo-draft-0001");
    let answers = pairs(&[("_rev", "0"), ("_wizard", "next"), ("title", "Annual plan")]);
    wizard.submit(&mut draft, "basics", &answers);
    wizard
        .page(&draft, "contact")
        .expect("the second step is open")
}

fn wizard_errors() -> WizardPage {
    let wizard = Wizard::new("errors")
        .base("/deals/new")
        .step(WizardStep::of::<Basics>("basics", "Basics"))
        .step(WizardStep::of::<Contact>("contact", "Contact"))
        .review();
    let mut draft = wizard.draft("demo-draft-0001");
    let first = pairs(&[("_rev", "0"), ("_wizard", "next"), ("title", "Annual plan")]);
    wizard.submit(&mut draft, "basics", &first);
    // Next with an address that is not one: the step stays and says why.
    let second = pairs(&[("_rev", "1"), ("_wizard", "next"), ("email", "harleen@")]);
    wizard.submit(&mut draft, "contact", &second);
    wizard.page(&draft, "contact").expect("the step is open")
}

fn wizard_review() -> WizardPage {
    let wizard = Wizard::new("review")
        .base("/deals/new")
        .step(WizardStep::of::<Basics>("basics", "Basics"))
        .step(WizardStep::of::<Contact>("contact", "Contact"))
        .review();
    let mut draft = wizard.draft("demo-draft-0001");
    let first = pairs(&[("_rev", "0"), ("_wizard", "next"), ("title", "Annual plan")]);
    wizard.submit(&mut draft, "basics", &first);
    let second = pairs(&[
        ("_rev", "1"),
        ("_wizard", "next"),
        ("email", "harleen@example.com"),
    ]);
    wizard.submit(&mut draft, "contact", &second);
    wizard.page(&draft, "review").expect("the review is open")
}

fn slider_thumb() -> Slider {
    Slider::new("thumb").label("Volume").value(40)
}

fn slider_default() -> Slider {
    Slider::new("volume")
        .label("Volume")
        .max(100)
        .step(5)
        .value(40)
        .suffix("%")
}

fn slider_range() -> Slider {
    Slider::range("price")
        .label("Price")
        .max(1000)
        .step(10)
        .min_gap(2)
        .values(200, 800)
        .prefix("₹")
}

fn slider_marks() -> Slider {
    Slider::new("level")
        .label("Level")
        .max(100)
        .step(5)
        .value(50)
        .ticks_every(5)
        .mark(0, "0")
        .mark(50, "50")
        .mark(100, "100")
}

fn slider_names() -> Slider {
    Slider::new("size")
        .label("Size")
        .max(2)
        .names(["Small", "Medium", "Large"])
        .value(1)
}

fn slider_decimals() -> Slider {
    Slider::new("opacity")
        .label("Opacity")
        .max(1)
        .step(0.1_f64)
        .value(0.7_f64)
}

fn slider_states() -> Vec<(&'static str, Slider)> {
    vec![
        (
            "Disabled",
            Slider::new("a").label("Disabled").value(30).disabled(true),
        ),
        (
            "Read only: shown, and posted",
            Slider::range("b")
                .label("Read only")
                .values(10, 60)
                .readonly(),
        ),
        (
            "Invalid",
            Slider::new("c").label("Invalid").value(90).invalid(true),
        ),
    ]
}

fn slider_rtl() -> Element {
    el("div").attr("dir", "rtl").child(
        Slider::range("rprice")
            .label("السعر")
            .max(100)
            .values(20, 70)
            .mark(0, "0")
            .mark(100, "100"),
    )
}

fn slider_check() -> Text {
    // The grid the browser draws is the grid the server holds a post to.
    let grid = SliderSpec::new(0, 100).step(5);
    let posted = [("volume".to_owned(), "37".to_owned())];
    match grid.read(&posted, "volume") {
        Ok(value) => Text::new(format!("Volume {value}")),
        Err(error) => Text::new(grid.message(error, &SliderLabels::default())),
    }
}

fn tree_thumb() -> Tree {
    Tree::new("thumb", "Folders")
        .node(
            TreeNode::branch("docs", "Documents", [TreeNode::leaf("plans", "Plans")])
                .icon(icons::FOLDER)
                .open(true),
        )
        .node(TreeNode::leaf("music", "Music").icon(icons::FOLDER))
}

fn tree_default() -> Tree {
    Tree::new("files", "Files")
        .node(
            TreeNode::branch(
                "docs",
                "Documents",
                [
                    TreeNode::leaf("plans", "Plans")
                        .link("/files/plans")
                        .icon(icons::FILE),
                    TreeNode::leaf("notes", "Notes")
                        .link("/files/notes")
                        .icon(icons::FILE)
                        .current(true),
                ],
            )
            .icon(icons::FOLDER)
            .open(true),
        )
        .node(
            TreeNode::branch("music", "Music", [TreeNode::leaf("song", "Song")])
                .icon(icons::FOLDER),
        )
        .node(
            TreeNode::leaf("readme", "README")
                .link("/files/readme")
                .icon(icons::FILE),
        )
}

fn tree_multiple() -> Tree {
    Tree::new("access", "Folders to share")
        .select(TreeSelect::Multiple)
        .cascade()
        .node(
            TreeNode::branch(
                "eng",
                "Engineering",
                [
                    TreeNode::leaf("eng/web", "Web"),
                    TreeNode::leaf("eng/api", "API").selected(true),
                    TreeNode::leaf("eng/old", "Retired").disabled(true),
                ],
            )
            .open(true),
        )
        .node(TreeNode::branch(
            "ops",
            "Operations",
            [TreeNode::leaf("ops/run", "Runbooks")],
        ))
}

fn tree_single() -> Tree {
    Tree::new("owner", "Owner")
        .select(TreeSelect::Single)
        .node(
            TreeNode::branch(
                "sales",
                "Sales",
                [
                    TreeNode::leaf("sales/harleen", "Harleen"),
                    TreeNode::leaf("sales/gurpreet", "Gurpreet").selected(true),
                ],
            )
            .open(true),
        )
        .node(TreeNode::branch(
            "support",
            "Support",
            [TreeNode::leaf("support/manpreet", "Manpreet")],
        ))
}

fn tree_lazy() -> Tree {
    Tree::new("lazy", "Folders")
        .node(TreeNode::lazy("docs", "Documents", "/tree/children?parent=docs").icon(icons::FOLDER))
        .node(TreeNode::lazy("music", "Music", "/tree/children?parent=music").icon(icons::FOLDER))
}

fn tree_paged() -> Tree {
    Tree::new("people", "People").node(
        TreeNode::branch(
            "all",
            "Everyone",
            (1..=5).map(|n| TreeNode::leaf(format!("p{n}"), format!("Person {n}"))),
        )
        .open(true)
        .children_total(10_000)
        .more("/people?after=5", Some(9_995)),
    )
}

fn tree_state() -> Tree {
    // The address says which branches are open, and each caret is a link that changes it.
    let state = TreeState::from_query("/files", "open=docs");
    Tree::new("nav", "Files")
        .state(state)
        .node(
            TreeNode::branch("docs", "Documents", [TreeNode::leaf("plans", "Plans")])
                .icon(icons::FOLDER),
        )
        .node(TreeNode::lazy("music", "Music", "/tree/children?parent=music").icon(icons::FOLDER))
}

fn toolbar_thumb() -> Toolbar {
    Toolbar::new("thumb", "Formatting")
        .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
        .item(ToolbarItem::icon_toggle("italic", icons::ITALIC, "Italic").pressed(true))
        .item(ToolbarItem::icon_toggle(
            "underline",
            icons::UNDERLINE,
            "Underline",
        ))
}

fn toolbar_default() -> Toolbar {
    Toolbar::new("format", "Text formatting")
        .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
        .item(ToolbarItem::icon_toggle("italic", icons::ITALIC, "Italic").pressed(true))
        .item(ToolbarItem::icon_toggle(
            "underline",
            icons::UNDERLINE,
            "Underline",
        ))
        .item(ToolbarItem::separator())
        .item(
            ToolbarItem::radios("align", "Alignment")
                .option("left", "Left")
                .option("center", "Center")
                .option("right", "Right")
                .value("left"),
        )
        .item(ToolbarItem::separator())
        .item(ToolbarItem::button("Copy").icon(icons::COPY))
        .item(ToolbarItem::link("Help", "/help"))
}

fn toolbar_overflow() -> Toolbar {
    // Too many tools for the width: the ones at the end move into More, and the one that is kept stays.
    Toolbar::new("narrow", "Text formatting")
        .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
        .item(ToolbarItem::icon_toggle("italic", icons::ITALIC, "Italic"))
        .item(ToolbarItem::button("Copy").icon(icons::COPY))
        .item(ToolbarItem::button("Paste"))
        .item(ToolbarItem::link("Help", "/help"))
        .item(ToolbarItem::button("Save").keep())
}

fn toolbar_vertical() -> Toolbar {
    Toolbar::new("tools", "Tools")
        .vertical()
        .item(ToolbarItem::button("Select").icon(icons::LIST))
        .item(ToolbarItem::button("Move"))
        .item(ToolbarItem::separator())
        .item(ToolbarItem::button("Delete").variant(ButtonVariant::Ghost))
}

fn toolbar_groups() -> Toolbar {
    Toolbar::new("groups", "Document")
        .item(ToolbarItem::group(
            "History",
            [
                ToolbarItem::button("Undo").icon(icons::ARROW_LEFT),
                ToolbarItem::button("Redo").icon(icons::ARROW_RIGHT),
            ],
        ))
        .item(ToolbarItem::menu(
            "Share",
            DropdownMenu::new("share", "Share")
                .variant(ButtonVariant::Ghost)
                .size(Size::Sm)
                .caret()
                .item(MenuItem::link("By email", "/share/email"))
                .item(MenuItem::action("Copy the link")),
        ))
}
