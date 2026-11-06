//! The stepper, the wizard, the slider, the tree view and the toolbar, in the states
//! the conformance suite runs against.
//!
//! | Page | Shows |
//! |---|---|
//! | `/flow` | every part, in its states |
//! | `/flow/echo` | what a form of the page posted, and what the server made of each slider |
//! | `/flow/tree/children?parent=` | the children of a lazy node (`fail`, `slow`, `empty`, `big`, `hostile` misbehave) |
//! | `/flow/hits` | how many times each parent was asked for |
//! | `/flow/wizard/start`, `/flow/wizard/{id}/{step}` | a wizard of four steps and a review |

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use http::{Method, StatusCode};
use navree::http::extract::{Path, RawQuery, urlencoded};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::flow::{
    Decimal, DraftStore, MemoryDrafts, SliderError, SliderLabels, SliderSpec, TreeSelection,
    WizardError,
};
use navree::ui::*;
use serde::Deserialize;

/// How many times each lazy parent was asked for.
static HITS: LazyLock<Mutex<BTreeMap<String, u32>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
static DRAFTS: OnceLock<MemoryDrafts> = OnceLock::new();
const OWNER: &str = "proof";

fn drafts() -> &'static MemoryDrafts {
    DRAFTS.get_or_init(MemoryDrafts::default)
}

fn pairs(input: &str) -> Vec<(String, String)> {
    urlencoded::parse(input.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(4).children(content)))
}

// ---- Stepper -------------------------------------------------------------

fn steppers() -> Element {
    section(
        "steppers",
        "Stepper",
        (
            Stepper::new("Checkout")
                .id("s-horizontal")
                .step(Step::new("Cart").link("/flow?step=cart#steppers"))
                .step(Step::new("Address").link("/flow?step=address#steppers"))
                .step(Step::new("Payment").state(StepState::Error).link("/flow?step=payment#steppers"))
                .step(Step::new("Review"))
                .step(Step::new("Done").state(StepState::Disabled))
                .current(3),
            Stepper::new("Setup")
                .id("s-vertical")
                .vertical()
                .step(Step::new("Create the account").description("Your name and email").link("/flow?step=1#steppers"))
                .step(Step::new("Invite the team").description("Optional").optional().link("/flow?step=2#steppers"))
                .step(Step::new("Connect the mailbox").description("Needs an administrator").state(StepState::Error))
                .step(Step::new("Launch").state(StepState::Disabled))
                .current(1),
            Stepper::new("Long names")
                .id("s-long")
                .step(Step::new("A step whose name is much too long to sit on one line of a narrow screen").description("And a description that is just as long as the name of the step above it, for good measure"))
                .step(Step::new("الخطوة الثانية في العملية"))
                .step(Step::new("Pneumonoultramicroscopicsilicovolcanoconiosis_Pneumonoultramicroscopicsilicovolcanoconiosis"))
                .current(0),
            Stepper::new("Fifty")
                .id("s-fifty")
                .steps((1..=50).map(|n| Step::new(format!("Step {n}"))))
                .current(24),
            Stepper::new("One")
                .id("s-one")
                .step(Step::new("Only"))
                .current(0),
            Stepper::new("Plain")
                .id("s-plain")
                .step(Step::new("Ordered"))
                .step(Step::new("Shipped"))
                .step(Step::new("Delivered"))
                .current(1),
        ),
    )
}

// ---- Slider --------------------------------------------------------------

fn field(id: &'static str, label: &'static str, control: impl Into<Node>) -> Node {
    Field::new(id, label).control(control).into()
}

/// A number written out, for a grid with a very small step.
fn tiny(text: &str) -> Decimal {
    text.parse().unwrap_or(Decimal::ONE)
}

fn sliders() -> Element {
    let many_marks = (0..=12).fold(
        Slider::new("many")
            .id("sl-many")
            .label("Twelve marks")
            .max(12),
        |slider, n| slider.mark(n, n.to_string()),
    );
    let form = el("form")
        .attr("id", "sl-form")
        .attr("method", "post")
        .attr("action", "/flow/echo")
        .children((
            Stack::gap(5).children((
                field(
                    "sl-volume",
                    "Volume",
                    Slider::new("volume")
                        .id("sl-volume")
                        .max(100)
                        .step(5)
                        .value(40)
                        .suffix("%")
                        .ticks_every(5),
                ),
                field(
                    "sl-tenths",
                    "Tenths",
                    Slider::new("tenths")
                        .id("sl-tenths")
                        .max(1)
                        .step(0.1_f64)
                        .value(0.3_f64),
                ),
                Stack::gap(5).children((
                    field(
                        "sl-mix",
                        "Mix",
                        Slider::new("mix")
                            .id("sl-mix")
                            .max(1)
                            .any_step()
                            .value(0.5_f64),
                    ),
                    field(
                        "sl-fine",
                        "Fine",
                        Slider::new("fine")
                            .id("sl-fine")
                            .max(tiny("0.00001"))
                            .step(tiny("0.0000001")),
                    ),
                    field(
                        "sl-comma",
                        "Menge",
                        Slider::new("menge")
                            .id("sl-comma")
                            .max(1)
                            .step(0.25_f64)
                            .value(0.5_f64)
                            .labels(SliderLabels {
                                decimal_point: ",".into(),
                                ..SliderLabels::default()
                            }),
                    ),
                )),
                el("div").children((
                    el("p").attr("id", "sl-price-label").child("Price"),
                    Slider::range("price")
                        .id("sl-price")
                        .labelled_by("sl-price-label")
                        .max(1000)
                        .step(10)
                        .min_gap(2)
                        .values(200, 800)
                        .prefix("$")
                        .mark(0, "$0")
                        .mark(500, "$500")
                        .mark(1000, "$1000"),
                )),
                field(
                    "sl-level",
                    "Level",
                    Slider::new("level")
                        .id("sl-level")
                        .max(2)
                        .names(["Low", "Medium", "High"])
                        .value(1),
                ),
                field("sl-many", "Many marks", many_marks),
                field(
                    "sl-wide",
                    "A wide track",
                    Slider::new("wide")
                        .id("sl-wide")
                        .min(-1_000_000)
                        .max(1_000_000)
                        .step(1000)
                        .value(0),
                ),
                field(
                    "sl-off",
                    "Disabled",
                    Slider::new("off").id("sl-off").value(30).disabled(true),
                ),
                field(
                    "sl-still",
                    "Read only",
                    Slider::range("still")
                        .id("sl-still")
                        .values(10, 60)
                        .readonly(),
                ),
                field(
                    "sl-big",
                    "Big step of 25",
                    Slider::new("big")
                        .id("sl-big")
                        .max(200)
                        .big_step(25)
                        .value(100),
                ),
                field(
                    "sl-invalid",
                    "Marked invalid",
                    Slider::new("bad").id("sl-invalid").value(90).invalid(true),
                ),
            )),
            Button::primary("Post the sliders")
                .attr("id", "sl-post")
                .submit(),
        ));
    let rtl = el("div").attr("dir", "rtl").attr("id", "sl-rtl").child(
        Stack::gap(5).children((
            field(
                "sl-rtl-one",
                "حجم",
                Slider::new("rvolume")
                    .id("sl-rtl-one")
                    .max(100)
                    .step(10)
                    .value(30)
                    .ticks_every(10),
            ),
            field(
                "sl-rtl-range",
                "السعر",
                Slider::range("rprice")
                    .id("sl-rtl-range")
                    .max(100)
                    .values(20, 70)
                    .mark(0, "0")
                    .mark(100, "100"),
            ),
        )),
    );
    section(
        "sliders",
        "Slider",
        (
            form,
            rtl,
            Slider::new("lonely")
                .id("sl-lonely")
                .label("Not in a form")
                .max(10)
                .value(4),
        ),
    )
}

// ---- Tree ----------------------------------------------------------------

fn sample(id: &str) -> Vec<TreeNode> {
    let src = |parent: &str| format!("/flow/tree/children?parent={parent}");
    match id {
        "lazy-docs" => vec![
            TreeNode::leaf("docs/plans", "Plans").icon(icons::FILE),
            TreeNode::lazy("docs/archive", "Archive", src("lazy-archive")).icon(icons::FOLDER),
            TreeNode::leaf("docs/notes", "Notes").icon(icons::FILE),
        ],
        "lazy-archive" => vec![
            TreeNode::leaf("docs/archive/2024", "2024"),
            TreeNode::leaf("docs/archive/2025", "2025"),
        ],
        _ => Vec::new(),
    }
}

fn tree_state(query: &str) -> TreeState {
    TreeState::from_query("/flow", query)
}

fn links_tree(state: &TreeState) -> Tree {
    let src = |parent: &str| format!("/flow/tree/children?parent={parent}");
    let docs = if state.is_open("docs") {
        TreeNode::branch("docs", "Documents", sample("lazy-docs")).icon(icons::FOLDER)
    } else {
        TreeNode::lazy("docs", "Documents", src("lazy-docs")).icon(icons::FOLDER)
    };
    Tree::new("nav", "Files")
        .id("t-links")
        .state(state.clone())
        .node(docs)
        .node(TreeNode::lazy("music", "Music", src("lazy-docs")).icon(icons::FOLDER))
        .node(
            TreeNode::leaf("readme", "README")
                .link("/flow?file=readme")
                .icon(icons::FILE)
                .current(true),
        )
        .node(
            TreeNode::leaf("license", "LICENSE")
                .link("/flow?file=license")
                .icon(icons::FILE),
        )
}

fn multi_tree() -> Tree {
    let src = |parent: &str| format!("/flow/tree/children?parent={parent}");
    Tree::new("access", "Folders to share")
        .id("t-multi")
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
        .node(TreeNode::lazy("ops", "Operations", src("lazy-docs")))
        .node(TreeNode::lazy("slow", "Slow folder", src("slow")))
        .node(TreeNode::lazy("fail", "Folder that fails", src("fail")))
        .node(TreeNode::lazy("empty", "Folder with nothing", src("empty")))
        .node(TreeNode::leaf("misc", "Miscellaneous"))
}

fn single_tree() -> Tree {
    Tree::new("owner", "Owner")
        .id("t-single")
        .select(TreeSelect::Single)
        .node(
            TreeNode::branch(
                "sales",
                "Sales",
                [
                    TreeNode::leaf("sales/anu", "Anu"),
                    TreeNode::leaf("sales/gauri", "Gauri").selected(true),
                ],
            )
            .open(true),
        )
        .node(TreeNode::branch(
            "support",
            "Support",
            [TreeNode::leaf("support/arun", "Arun")],
        ))
}

const PAGE: usize = 50;
const BIG: usize = 10_000;

fn big_tree() -> Tree {
    Tree::new("big", "Everyone")
        .id("t-big")
        .select(TreeSelect::Multiple)
        .node(
            TreeNode::branch("everyone", "Everyone (10,000)", big_page(0).0)
                .open(true)
                .children_total(BIG)
                .more(
                    format!("/flow/tree/children?parent=big&after={PAGE}"),
                    Some(BIG - PAGE),
                ),
        )
}

fn big_page(after: usize) -> (Vec<TreeNode>, Option<(Href, Option<usize>)>) {
    let nodes = (after..(after + PAGE).min(BIG))
        .map(|n| TreeNode::leaf(format!("person-{n}"), format!("Person {n}")))
        .collect();
    let next = after + PAGE;
    let more = (next < BIG).then(|| {
        (
            Href::from(format!("/flow/tree/children?parent=big&after={next}")),
            Some(BIG - next),
        )
    });
    (nodes, more)
}

/// Choices that reach what is under them, with children the person can't choose and children they
/// haven't been shown: a disabled child, a chosen node that is disabled, and a branch in pages.
fn grant_tree() -> Tree {
    Tree::new("grant", "Grant access")
        .id("t-grant")
        .select(TreeSelect::Multiple)
        .cascade()
        .node(
            TreeNode::branch(
                "proj",
                "Projects",
                [
                    TreeNode::leaf("proj/open", "Open project").selected(true),
                    TreeNode::leaf("proj/secret", "Secret project").disabled(true),
                ],
            )
            .open(true),
        )
        .node(
            TreeNode::branch(
                "team",
                "Teams",
                [
                    TreeNode::leaf("team/a", "Team A"),
                    TreeNode::leaf("team/locked", "Locked team").disabled(true),
                ],
            )
            .open(true),
        )
        .node(
            TreeNode::leaf("kept", "Always on")
                .selected(true)
                .disabled(true),
        )
        .node(
            TreeNode::branch(
                "pool",
                "Pool in pages",
                [
                    TreeNode::leaf("person-0", "Member 0"),
                    TreeNode::leaf("person-1", "Member 1"),
                ],
            )
            .open(true)
            .children_total(BIG)
            .more("/flow/tree/children?parent=big&after=2", Some(BIG - 2)),
        )
}

fn hostile_tree() -> Tree {
    Tree::new("hostile", "Hostile names")
        .id("t-hostile")
        .select(TreeSelect::Multiple)
        .node(TreeNode::leaf("\"><img src=x onerror=alert(1)>", "<img src=x onerror=alert(1)> and a very long word: Pneumonoultramicroscopicsilicovolcanoconiosis_Pneumonoultramicroscopicsilicovolcanoconiosis"))
        .node(TreeNode::leaf("a b", "id with a space"))
        .node(TreeNode::lazy("toString", "toString", "/flow/tree/children?parent=hostile"))
}

fn deep_tree() -> Tree {
    let mut node = TreeNode::leaf("deep-leaf", "The bottom");
    for n in (0..30).rev() {
        node = TreeNode::branch(format!("deep-{n}"), format!("Level {n}"), [node]).open(n < 5);
    }
    Tree::new("deep", "Thirty levels").id("t-deep").node(node)
}

fn trees(query: &str) -> Element {
    let state = tree_state(query);
    let form = el("form")
        .attr("id", "t-form")
        .attr("method", "post")
        .attr("action", "/flow/echo")
        .children((
            Stack::gap(4).children((
                multi_tree(),
                single_tree(),
                big_tree(),
                hostile_tree(),
                grant_tree(),
            )),
            Button::primary("Post the choices")
                .attr("id", "t-post")
                .submit(),
        ));
    section(
        "trees",
        "Tree",
        (
            links_tree(&state),
            form,
            deep_tree(),
            Tree::new("none", "Nothing in it").id("t-empty"),
            el("div").attr("dir", "rtl").attr("id", "t-rtl").child(
                Tree::new("rtl", "شجرة")
                    .id("t-rtl-tree")
                    .select(TreeSelect::Multiple)
                    .cascade()
                    .node(
                        TreeNode::branch(
                            "r1",
                            "الجذر",
                            [
                                TreeNode::leaf("r1a", "ورقة"),
                                TreeNode::leaf("r1b", "ورقة ثانية"),
                            ],
                        )
                        .open(true),
                    ),
            ),
        ),
    )
}

// ---- Toolbar -------------------------------------------------------------

fn format_bar(id: &'static str) -> Toolbar {
    Toolbar::new(id, "Text formatting")
        .item(ToolbarItem::icon_toggle("bold", icons::LETTER_B, "Bold"))
        .item(ToolbarItem::icon_toggle("italic", icons::ITALIC, "Italic").pressed(true))
        .item(ToolbarItem::icon_toggle(
            "underline",
            icons::UNDERLINE,
            "Underline",
        ))
        .item(ToolbarItem::separator())
        .item(ToolbarItem::group(
            "Alignment",
            [ToolbarItem::radios("align", "Alignment")
                .option("left", "Left")
                .option("center", "Center")
                .option("right", "Right")
                .value("left")],
        ))
        .item(ToolbarItem::separator())
        .item(
            ToolbarItem::button("Copy")
                .icon(icons::COPY)
                .on_click(Cmd::toast(ToastTone::Success, "Copied.")),
        )
        .item(ToolbarItem::button("Disabled").disabled(true))
        .item(ToolbarItem::link("Help", "/flow?help=1"))
        .item(ToolbarItem::post("Archive", "/flow/archive"))
        .item(ToolbarItem::menu(
            "Share",
            DropdownMenu::new(format!("{id}-share"), "Share")
                .variant(ButtonVariant::Ghost)
                .size(Size::Sm)
                .caret()
                .item(MenuItem::link("By email", "/flow?share=email"))
                .item(
                    MenuItem::action("Copy the link")
                        .on_click(Cmd::toast(ToastTone::Info, "Link copied.")),
                ),
        ))
        .item(ToolbarItem::button("Always shown").keep())
}

fn toolbars() -> Element {
    let form = el("form")
        .attr("id", "tb-form")
        .attr("method", "post")
        .attr("action", "/flow/echo")
        .children((
            format_bar("tb-main"),
            Button::primary("Post the toolbar")
                .attr("id", "tb-post")
                .submit(),
        ));
    section(
        "toolbars",
        "Toolbar",
        (
            form,
            el("form")
                .attr("id", "tb-late-form")
                .attr("method", "post")
                .attr("action", "/flow/echo")
                .children((
                    Toolbar::new("tb-late", "Late toggles")
                        .item(ToolbarItem::button("Always here").keep())
                        .item(ToolbarItem::button("Another wide tool"))
                        .item(ToolbarItem::button("And one more wide tool"))
                        .item(ToolbarItem::icon_toggle(
                            "late_bold",
                            icons::LETTER_B,
                            "Late bold",
                        ))
                        .item(
                            ToolbarItem::icon_toggle("late_italic", icons::ITALIC, "Late italic")
                                .pressed(true),
                        )
                        .item(
                            ToolbarItem::radios("late_size", "Size")
                                .option("s", "S")
                                .option("m", "M")
                                .value("s"),
                        ),
                    Button::primary("Post the late toggles")
                        .attr("id", "tb-late-post")
                        .submit(),
                )),
            Toolbar::new("tb-many", "Many tools").items((1..=24).map(|n| {
                ToolbarItem::button(format!("Tool {n}"))
                    .on_click(Cmd::toast(ToastTone::Info, format!("Tool {n}")))
            })),
            Toolbar::new("tb-vertical", "Vertical tools")
                .vertical()
                .item(ToolbarItem::button("One"))
                .item(ToolbarItem::button("Two"))
                .item(ToolbarItem::separator())
                .item(ToolbarItem::button("Three")),
            Toolbar::new("tb-one", "One tool").item(ToolbarItem::button("Only")),
            Toolbar::new("tb-none", "No tools"),
            el("div").attr("dir", "rtl").attr("id", "tb-rtl-box").child(
                Toolbar::new("tb-rtl", "أدوات")
                    .item(ToolbarItem::button("أول"))
                    .item(ToolbarItem::button("ثاني"))
                    .item(ToolbarItem::button("ثالث")),
            ),
        ),
    )
}

fn page(shell: &AppShell, query: &str) -> AppPage {
    shell.page("Flow").body(
        Stack::gap(10).children((
            Row::gap(3)
                .wrap()
                .children((Button::outline("Start the wizard")
                    .href("/flow/wizard/start")
                    .attr("id", "start-wizard"),)),
            steppers(),
            sliders(),
            trees(query),
            toolbars(),
        )),
    )
}

// ---- The echo --------------------------------------------------------------

/// What the form posted, and what the server made of the sliders.
fn echo(shell: &AppShell, body: &str) -> AppPage {
    let all = pairs(body);
    let labels = SliderLabels::default();
    let mut lines: Vec<String> = all
        .iter()
        .filter(|(name, _)| name != "_csrf")
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    let mut checks: Vec<String> = Vec::new();
    let mut judge = |name: &str, result: Result<String, (SliderSpec, SliderError)>| {
        checks.push(match result {
            Ok(text) => format!("{name}: ok {text}"),
            Err((spec, error)) => format!("{name}: {}", spec.message(error, &labels)),
        });
    };
    if all.iter().any(|(name, _)| name == "volume") {
        let spec = SliderSpec::new(0, 100).step(5);
        judge(
            "volume",
            spec.read(&all, "volume")
                .map(|v| v.to_string())
                .map_err(|e| (spec, e)),
        );
        let spec = SliderSpec::new(0, 1).step(0.1_f64);
        judge(
            "tenths",
            spec.read(&all, "tenths")
                .map(|v| v.to_string())
                .map_err(|e| (spec, e)),
        );
        let spec = SliderSpec::new(0, 1000).step(10).min_gap(2);
        judge(
            "price",
            spec.read_range(&all, "price")
                .map(|(a, b)| format!("{a}-{b}"))
                .map_err(|e| (spec, e)),
        );
        let spec = SliderSpec::new(0, 1).any_step();
        judge(
            "mix",
            spec.read(&all, "mix")
                .map(|v| v.to_string())
                .map_err(|e| (spec, e)),
        );
        let spec = SliderSpec::new(0, tiny("0.00001")).step(tiny("0.0000001"));
        judge(
            "fine",
            spec.read(&all, "fine")
                .map(|v| v.to_string())
                .map_err(|e| (spec, e)),
        );
        let spec = SliderSpec::new(0, 2);
        judge(
            "level",
            spec.read(&all, "level")
                .map(|v| v.to_string())
                .map_err(|e| (spec, e)),
        );
    }
    if ["access", "owner", "big", "hostile"]
        .iter()
        .any(|name| all.iter().any(|(field, _)| field == *name))
    {
        for field in ["access", "owner", "big", "hostile"] {
            match TreeSelection::parse(&all, field) {
                Ok(chosen) if !chosen.is_empty() => checks.push(format!(
                    "{field}: {} chosen: {}",
                    chosen.len(),
                    chosen
                        .ids()
                        .iter()
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" | ")
                )),
                Ok(_) => {}
                Err(error) => checks.push(format!("{field}: {error}")),
            }
        }
    }
    lines.sort();
    shell.page("Posted").body(Stack::gap(4).children((
        el("pre").attr("id", "posted").child(lines.join("\n")),
        el("pre").attr("id", "judged").child(checks.join("\n")),
        Button::outline("Back").href("/flow"),
    )))
}

// ---- Tree children -----------------------------------------------------------

#[derive(Deserialize)]
struct ChildrenQuery {
    parent: String,
    #[serde(default)]
    after: usize,
}

async fn children(query: ChildrenQuery) -> Response {
    {
        let mut hits = HITS.lock().unwrap_or_else(PoisonError::into_inner);
        *hits.entry(query.parent.clone()).or_insert(0) += 1;
    }
    let template = Tree::new("access", "Folders to share")
        .select(TreeSelect::Multiple)
        .cascade();
    match query.parent.as_str() {
        "fail" => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        "empty" => Node::Empty.into_response_html(),
        "slow" => {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            template
                .children(
                    2,
                    0,
                    None,
                    [
                        TreeNode::leaf("slow/a", "Slow A"),
                        TreeNode::leaf("slow/b", "Slow B"),
                    ],
                    None,
                )
                .into_response_html()
        }
        "big" => {
            let (nodes, more) = big_page(query.after);
            let template = Tree::new("big", "Everyone").select(TreeSelect::Multiple);
            template
                .children(2, query.after, Some(BIG), nodes, more)
                .into_response_html()
        }
        "hostile" => template
            .children(
                2,
                0,
                None,
                [TreeNode::leaf(
                    "\"><script>x</script>",
                    "<script>window.__pwned=1</script>",
                )],
                None,
            )
            .into_response_html(),
        other => {
            let nodes = sample(other);
            template
                .children(2, 0, None, nodes, None)
                .into_response_html()
        }
    }
}

/// A fragment as an HTML response.
trait Fragment {
    fn into_response_html(self) -> Response;
}

impl<T: Into<Node>> Fragment for T {
    fn into_response_html(self) -> Response {
        (
            [
                (http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (http::header::CACHE_CONTROL, "no-store"),
            ],
            render(&self.into()),
        )
            .into_response()
    }
}

fn hits() -> Response {
    let hits = HITS.lock().unwrap_or_else(PoisonError::into_inner);
    let body = hits
        .iter()
        .map(|(name, count)| format!("\"{name}\":{count}"))
        .collect::<Vec<_>>()
        .join(",");
    (
        [(http::header::CONTENT_TYPE, "application/json")],
        format!("{{{body}}}"),
    )
        .into_response()
}

// ---- The wizard --------------------------------------------------------------

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[option(label = "A new customer")]
    New,
    #[option(label = "An existing customer")]
    Existing,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct Basics {
    /// Deal name
    #[validate(length(min = 1, max = 60))]
    #[form(error = "Give the deal a name.", autofocus)]
    pub title: String,
    /// Who is it for
    #[validate(skip)]
    #[form(widget = radio)]
    pub kind: Kind,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct Contact {
    /// Email
    #[validate(email)]
    #[form(widget = email, error = "Enter an email address like anu@example.com.")]
    pub email: String,
    /// Access code
    #[validate(length(min = 6))]
    #[form(widget = password, help = "Typed again if you leave and come back; a draft never keeps it.")]
    pub code: String,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct Extras {
    /// Notes
    #[validate(length(max = 200))]
    #[form(widget = textarea, wide)]
    pub notes: String,
    /// Send me updates
    #[validate(skip)]
    pub news: bool,
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

fn deal_wizard() -> Wizard {
    Wizard::new("deal")
        .base("/flow/wizard")
        .step(
            WizardStep::of::<Basics>("basics", "Basics")
                .description("What the deal is, and who it is for"),
        )
        .step(WizardStep::of::<Contact>("contact", "Contact"))
        .step(WizardStep::of::<Extras>("extras", "Extras").optional())
        .step(WizardStep::of::<Terms>("terms", "Terms"))
        .review()
}

fn wizard_page(
    shell: &AppShell,
    wizard: &Wizard,
    draft: &Draft,
    step: &str,
    stale: bool,
) -> Response {
    match wizard.page(draft, step) {
        Ok(page) => {
            let page = if stale { page.stale() } else { page };
            let title = page.title_text();
            let mut response = shell
                .page(format!("New deal: {title}"))
                .body(Card::new().body(page))
                .into_response();
            if stale {
                *response.status_mut() = StatusCode::CONFLICT;
            }
            response
        }
        Err(WizardError::NotReachable | WizardError::UnknownStep) => {
            Reply::redirect(wizard.url(&draft.id, &wizard.entry(draft))).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn wizard_start() -> Response {
    let wizard = deal_wizard();
    let draft = wizard.start();
    match drafts().save(OWNER, &draft, None).await {
        Ok(()) => Reply::redirect(wizard.url(&draft.id, &draft.step)).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn wizard_get(shell: AppShell, id: String, step: String) -> Response {
    let wizard = deal_wizard();
    match drafts().load(OWNER, &id).await {
        Ok(Some(draft)) if draft.submitted => {
            Reply::redirect(format!("/flow/wizard/done/{id}")).into_response()
        }
        Ok(Some(draft)) => wizard_page(&shell, &wizard, &draft, &step, false),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn wizard_post(shell: AppShell, id: String, step: String, body: String) -> Response {
    let wizard = deal_wizard();
    let Ok(Some(mut draft)) = drafts().load(OWNER, &id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let before = draft.revision;
    let all = pairs(&body);
    let outcome = wizard.submit(&mut draft, &step, &all);
    if outcome.changed() && drafts().save(OWNER, &draft, Some(before)).await.is_err() {
        // Another post got there first.
        return match drafts().load(OWNER, &id).await {
            Ok(Some(latest)) => wizard_page(&shell, &wizard, &latest, &wizard.entry(&latest), true),
            _ => StatusCode::CONFLICT.into_response(),
        };
    }
    match outcome {
        WizardOutcome::Go(to) | WizardOutcome::Invalid(to) => {
            Reply::redirect(wizard.url(&id, &to)).into_response()
        }
        WizardOutcome::Left => Reply::redirect("/flow?saved=1")
            .toast(ToastTone::Success, "Draft saved. Come back any time.")
            .into_response(),
        WizardOutcome::Done => {
            // The record is made, and the draft goes.
            let made = draft
                .read::<Basics>("basics")
                .map(|b| b.title)
                .unwrap_or_default();
            let _ = drafts().discard(OWNER, &id).await;
            DONE.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id.clone(), made);
            // How many times this draft made a deal: a second post of Finish must not make a second.
            *HITS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(format!("made-{id}"))
                .or_insert(0) += 1;
            Reply::redirect(format!("/flow/wizard/done/{id}")).into_response()
        }
        WizardOutcome::Stale => match drafts().load(OWNER, &id).await {
            Ok(Some(latest)) => wizard_page(&shell, &wizard, &latest, &wizard.entry(&latest), true),
            _ => StatusCode::NOT_FOUND.into_response(),
        },
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

/// The deals the wizard made, by draft id: how many times each was made shows a double post.
static DONE: LazyLock<Mutex<BTreeMap<String, String>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn wizard_done(shell: &AppShell, id: &str) -> Response {
    let made = DONE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(id)
        .cloned();
    match made {
        Some(title) => shell
            .page("Deal created")
            .body(Alert::new(AlertTone::Success).title(format!("Created: {title}")))
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---- The controller -----------------------------------------------------------

pub struct FlowPages;

impl Injectable for FlowPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for FlowPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/flow",
                |shell: AppShell, RawQuery(query): RawQuery| async move {
                    page(&shell, query.as_deref().unwrap_or_default())
                },
            )
            .public(),
            RouteDef::on(Method::POST, "!/flow/echo", |shell: AppShell, body: String| async move {
                echo(&shell, &body)
            })
            .public(),
            RouteDef::on(Method::POST, "!/flow/archive", || async {
                Reply::ops_or_redirect("/flow").toast(ToastTone::Success, "Archived by the toolbar.")
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/flow/tree/children",
                |RawQuery(query): RawQuery| async move {
                    let query = query.unwrap_or_default();
                    match navree::http::extract::urlencoded::from_bytes::<ChildrenQuery>(query.as_bytes()) {
                        Ok(query) => children(query).await,
                        Err(_) => StatusCode::BAD_REQUEST.into_response(),
                    }
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/flow/hits", || async { hits() }).public(),
            RouteDef::on(Method::GET, "!/flow/wizard/start", || async { wizard_start().await }).public(),
            RouteDef::on(
                Method::GET,
                "!/flow/wizard/done/{id}",
                |shell: AppShell, Path(id): Path<String>| async move { wizard_done(&shell, &id) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/flow/wizard/{id}/{step}",
                |shell: AppShell, Path((id, step)): Path<(String, String)>| async move {
                    wizard_get(shell, id, step).await
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/flow/wizard/{id}/{step}",
                |shell: AppShell, Path((id, step)): Path<(String, String)>, body: String| async move {
                    wizard_post(shell, id, step, body).await
                },
            )
            .public(),
        ]
    }
}
