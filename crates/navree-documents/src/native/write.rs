//! Laid-out pages to PDF with krilla: glyph runs with their source text
//! (subset CID fonts with `ToUnicode` maps), images, fills and strokes,
//! link annotations, the tag tree, the outline and the metadata.

use std::num::NonZeroU16;

use chrono::{DateTime, Datelike, Timelike, Utc};
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::metadata::{Metadata, TextDirection};
use krilla::num::NormalizedF32;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;
use krilla::tagging::{
    Artifact, ArtifactType, ContentTag, Identifier, ListNumbering, SpanTag, Tag, TagGroup,
    TagKind, TagTree, TableHeaderScope,
};
use krilla::text::{GlyphId, KrillaGlyph};
use krilla::{Document, SerializeSettings};

use super::fonts::FontBook;
use super::layout::{Draw, LaidDocument};
use super::model::Color;
use super::tags::{Child, Furniture, Mark, NodeId, NodeKind, StructTree};

/// What goes into the document's metadata.
#[derive(Clone, Debug, Default)]
pub(crate) struct Info {
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) lang: Option<String>,
    pub(crate) rtl: bool,
    pub(crate) created: Option<DateTime<Utc>>,
}

/// The PDF of `laid`.
pub(crate) fn write(laid: &LaidDocument, fonts: &FontBook, info: &Info) -> Result<Vec<u8>, String> {
    let settings = SerializeSettings {
        enable_tagging: true,
        ..SerializeSettings::default()
    };
    let mut document = Document::new_with(settings);
    let mut slots: Vec<Vec<Identifier>> = vec![Vec::new(); laid.tree.slots];
    let size = PageSettings::from_wh(laid.width, laid.height)
        .ok_or_else(|| format!("invalid page size {} x {}", laid.width, laid.height))?;
    for page_layout in &laid.pages {
        let mut page = document.start_page_with(size.clone());
        let mut links = Vec::new();
        {
            let mut surface = page.surface();
            for draw in &page_layout.draws {
                match draw {
                    Draw::Link {
                        x,
                        y,
                        w,
                        h,
                        url,
                        slot,
                    } => links.push((*x, *y, *w, *h, url.clone(), *slot)),
                    other => draw_one(&mut surface, other, fonts, &mut slots),
                }
            }
            surface.finish();
        }
        for (x, y, w, h, url, slot) in links {
            let Some(rect) = Rect::from_xywh(x, y, w.max(0.1), h.max(0.1)) else {
                continue;
            };
            let link = LinkAnnotation::new(
                rect,
                Target::Action(Action::Link(LinkAction::new(url.to_string()))),
            );
            let annotation = Annotation::new_link(link, Some(url.to_string()));
            match slot {
                Some(slot) => {
                    let id = page.add_tagged_annotation(annotation);
                    slots[slot].push(id);
                }
                None => page.add_annotation(annotation),
            }
        }
        page.finish();
    }
    document.set_tag_tree(tag_tree(&laid.tree, &mut slots, info.lang.clone()));
    if let Some(outline) = outline(laid) {
        document.set_outline(outline);
    }
    document.set_metadata(metadata(info));
    document
        .finish()
        .map_err(|error| format!("writing the PDF failed: {error:?}"))
}

fn draw_one(surface: &mut Surface<'_>, draw: &Draw, fonts: &FontBook, slots: &mut [Vec<Identifier>]) {
    match draw {
        Draw::Fill { x, y, w, h, color } => {
            let Some(rect) = Rect::from_xywh(*x, *y, w.max(0.0), h.max(0.0)) else {
                return;
            };
            let mut path = PathBuilder::new();
            path.push_rect(rect);
            let Some(path) = path.finish() else {
                return;
            };
            artifact(surface, |surface| {
                surface.set_stroke(None);
                surface.set_fill(Some(fill(*color)));
                surface.draw_path(&path);
            });
        }
        Draw::Stroke {
            x1,
            y1,
            x2,
            y2,
            width,
            color,
        } => {
            let mut path = PathBuilder::new();
            path.move_to(*x1, *y1);
            path.line_to(*x2, *y2);
            let Some(path) = path.finish() else {
                return;
            };
            artifact(surface, |surface| {
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: rgb(*color).into(),
                    width: *width,
                    ..Stroke::default()
                }));
                surface.draw_path(&path);
                surface.set_stroke(None);
            });
        }
        Draw::Text {
            x,
            baseline,
            run,
            mark,
        } => {
            if run.glyphs.is_empty() {
                return;
            }
            let size = run.style.size;
            let glyphs: Vec<KrillaGlyph> = run
                .glyphs
                .iter()
                .map(|g| {
                    KrillaGlyph::new(
                        GlyphId::new(g.id),
                        g.advance / size,
                        g.x_offset / size,
                        g.y_offset / size,
                        0.0,
                        g.range.clone(),
                        None,
                    )
                })
                .collect();
            let font = fonts.face(run.face).pdf.clone();
            let id = begin(surface, *mark, true);
            surface.set_stroke(None);
            surface.set_fill(Some(fill(run.style.color)));
            surface.draw_glyphs(Point::from_xy(*x, *baseline), &glyphs, font, &run.text, size, false);
            surface.end_tagged();
            if let (Mark::Content(slot), Some(id)) = (mark, id) {
                slots[*slot].push(id);
            }
        }
        Draw::Image {
            x,
            y,
            w,
            h,
            image,
            mark,
        } => {
            let Some(size) = Size::from_wh(w.max(0.1), h.max(0.1)) else {
                return;
            };
            let id = begin(surface, *mark, false);
            surface.push_transform(&Transform::from_translate(*x, *y));
            surface.draw_image(image.clone(), size);
            surface.pop();
            surface.end_tagged();
            if let (Mark::Content(slot), Some(id)) = (mark, id) {
                slots[*slot].push(id);
            }
        }
        Draw::Link { .. } => {}
    }
}

/// Starts a tagged section: content (a span of text, or other content)
/// or an artifact. Returns the identifier of content.
fn begin(surface: &mut Surface<'_>, mark: Mark, text: bool) -> Option<Identifier> {
    match mark {
        Mark::Content(_) => {
            let tag = if text {
                ContentTag::Span(SpanTag::empty())
            } else {
                ContentTag::Other
            };
            Some(surface.start_tagged(tag))
        }
        Mark::Artifact(furniture) => {
            let kind = match furniture {
                Furniture::Header => ArtifactType::Header,
                Furniture::Footer => ArtifactType::Footer,
                Furniture::Layout => ArtifactType::Layout,
                Furniture::Pagination => ArtifactType::PaginationOther,
            };
            surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(kind)));
            None
        }
    }
}

/// Draws layout furniture (fills, lines) as an artifact.
fn artifact(surface: &mut Surface<'_>, draw: impl FnOnce(&mut Surface<'_>)) {
    surface.start_tagged(ContentTag::Artifact(Artifact::with_kind(ArtifactType::Layout)));
    draw(surface);
    surface.end_tagged();
}

fn rgb(color: Color) -> rgb::Color {
    rgb::Color::new(color.r, color.g, color.b)
}

fn fill(color: Color) -> Fill {
    Fill {
        paint: rgb(color).into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    }
}

/// The tag tree: each structure node a group, each slot the identifiers
/// drawn for it, in order.
fn tag_tree(tree: &StructTree, slots: &mut [Vec<Identifier>], lang: Option<String>) -> TagTree {
    let mut tags = TagTree::new().with_lang(lang);
    for &root in &tree.roots {
        if let Some(group) = group(tree, root, slots) {
            tags.push(group);
        }
    }
    tags
}

fn group(tree: &StructTree, id: NodeId, slots: &mut [Vec<Identifier>]) -> Option<TagGroup> {
    let node = &tree.nodes[id];
    let kind: TagKind = match &node.kind {
        NodeKind::P => Tag::P.into(),
        NodeKind::H(level) => Tag::Hn(NonZeroU16::new(u16::from(*level)).unwrap_or(NonZeroU16::MIN), None).into(),
        NodeKind::L(ordered) => Tag::L(if *ordered {
            ListNumbering::Decimal
        } else {
            ListNumbering::Disc
        })
        .into(),
        NodeKind::Li => Tag::LI.into(),
        NodeKind::Lbl => Tag::Lbl.into(),
        NodeKind::LBody => Tag::LBody.into(),
        NodeKind::Table => Tag::Table.into(),
        NodeKind::THead => Tag::THead.into(),
        NodeKind::TBody => Tag::TBody.into(),
        NodeKind::Tr => Tag::TR.into(),
        NodeKind::Th => Tag::TH(TableHeaderScope::Column).into(),
        NodeKind::Td => Tag::TD.into(),
        NodeKind::Figure(alt) => Tag::Figure(alt.clone()).into(),
        NodeKind::Link => Tag::Link.into(),
    };
    let mut group = TagGroup::new(kind);
    let mut children = 0;
    for child in &node.children {
        match child {
            Child::Node(child) => {
                if let Some(sub) = self::group(tree, *child, slots) {
                    group.push(sub);
                    children += 1;
                }
            }
            Child::Slot(slot) => {
                for identifier in std::mem::take(&mut slots[*slot]) {
                    group.push(identifier);
                    children += 1;
                }
            }
        }
    }
    (children > 0).then_some(group)
}

/// The outline: headings of levels 1 to 3, nested by level.
fn outline(laid: &LaidDocument) -> Option<Outline> {
    let mut entries: Vec<(u8, OutlineNode)> = Vec::new();
    let mut outline = Outline::new();
    // Builds bottom-up: a node joins the nearest open node of a lower level.
    fn close(entries: &mut Vec<(u8, OutlineNode)>, outline: &mut Outline, level: u8) {
        while let Some((last, _)) = entries.last() {
            if *last < level {
                break;
            }
            let (_, node) = entries.pop().expect("an entry");
            match entries.last_mut() {
                Some((_, parent)) => parent.push_child(node),
                None => outline.push_child(node),
            }
        }
    }
    let mut any = false;
    for (index, page) in laid.pages.iter().enumerate() {
        for (anchor, y) in &page.anchors {
            if anchor.level > 3 || anchor.title.is_empty() {
                continue;
            }
            any = true;
            close(&mut entries, &mut outline, anchor.level);
            let node = OutlineNode::new(
                anchor.title.clone(),
                XyzDestination::new(index, Point::from_xy(0.0, *y)),
            );
            entries.push((anchor.level, node));
        }
    }
    close(&mut entries, &mut outline, 0);
    any.then_some(outline)
}

fn metadata(info: &Info) -> Metadata {
    let mut metadata = Metadata::new().creator("Navree".to_owned());
    if let Some(title) = &info.title {
        metadata = metadata.title(title.clone());
    }
    if let Some(author) = &info.author {
        metadata = metadata.authors(vec![author.clone()]);
    }
    if let Some(lang) = &info.lang {
        metadata = metadata.language(lang.clone());
    }
    if info.rtl {
        metadata = metadata.text_direction(TextDirection::RightToLeft);
    }
    if let Some(now) = info.created {
        let date = krilla::metadata::DateTime::new(now.year().clamp(0, 9999) as u16)
            .month(now.month() as u8)
            .day(now.day() as u8)
            .hour(now.hour() as u8)
            .minute(now.minute() as u8)
            .second(now.second() as u8)
            .utc_offset_hour(0)
            .utc_offset_minute(0);
        metadata = metadata.creation_date(date);
    }
    metadata
}
