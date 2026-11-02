//! [`Objects`]: a document's objects held to be changed, and the writer
//! that saves them as a PDF.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;

use crate::budget::{LOOKUP_COST, ReadLimits};
use crate::document::{Document, MAX_HOPS, NodeKind, ReadObjects, node_kind};
use crate::error::{PdfError, size};
use crate::filters;
use crate::object::{Dictionary, Object, ObjectId, Stream, StringFormat};
use crate::parse::{
    ENTRY_MEMORY, MAX_DEPTH, OBJECT_MEMORY, TABLE_MEMORY, block, dictionary_extra, is_regular,
    items_block,
};

/// Objects a writer puts in one object stream (100).
const OBJECTS_PER_STREAM: usize = 100;
/// The highest object number kept as it is when a document is read to be
/// edited (8,388,607, the limit of PDF 1.7 on indirect objects); a
/// document with a higher one is renumbered from 1, so new numbers always
/// fit.
const MAX_KEPT_NUMBER: u32 = (1 << 23) - 1;
/// How deep the page tree may nest (256).
const MAX_TREE_DEPTH: usize = 256;
/// The shortest stream [`Objects::compress`] compresses (64 bytes): making a
/// compressor costs more than a shorter one could give back.
const SHORTEST_COMPRESSED: usize = 64;
/// The keys of a stream's dictionary that say how its bytes are read, or where:
/// a stream with one of them is left as it is, since a filter added to it would
/// be read with parameters meant for no filter, or for another.
const DESCRIBES_BYTES: [&[u8]; 6] = [
    b"Filter",
    b"DecodeParms",
    b"DP",
    b"F",
    b"FFilter",
    b"FDecodeParms",
];
/// What compressing one stream costs in work, besides [`COMPRESS_PER_BYTE`] for
/// each of its own bytes (4 KiB): the compressor's tables.
const COMPRESS_COST: usize = 4096;
/// What compressing costs in work for each byte of a stream (2): the fastest
/// level of Flate takes 7 ns a byte at the most, on data of any kind, and a
/// byte of work is about 8 ns.
const COMPRESS_PER_BYTE: usize = 2;
/// The keys of a page tree node that lead to what a page needs, besides its
/// `Kids`.
const NEEDED_KEYS: [&[u8]; 5] = [
    b"Contents",
    b"Resources",
    b"MediaBox",
    b"CropBox",
    b"Rotate",
];

/// What a document's objects may still take, once read: the work of looking
/// objects up, and the memory an edit adds. The first limit that ran out is
/// kept: [`Objects::check`] and saving return it.
#[derive(Clone, Debug)]
struct EditBudget {
    work: Cell<usize>,
    work_limit: usize,
    added: Cell<usize>,
    added_limit: usize,
    failure: RefCell<Option<PdfError>>,
}

impl Default for EditBudget {
    fn default() -> Self {
        Self {
            work: Cell::new(0),
            work_limit: usize::MAX,
            added: Cell::new(0),
            added_limit: usize::MAX,
            failure: RefCell::new(None),
        }
    }
}

impl EditBudget {
    /// Keeps `error`, if it is the first.
    fn fail(&self, error: PdfError) -> PdfError {
        self.failure.borrow_mut().get_or_insert(error).clone()
    }

    fn work(&self, bytes: usize) -> Result<(), PdfError> {
        let used = self.work.get().saturating_add(bytes);
        self.work.set(used);
        if used > self.work_limit {
            return Err(self.fail(PdfError::TooLarge(format!(
                "an edit of a PDF takes more work than the limit of {}",
                size(self.work_limit)
            ))));
        }
        Ok(())
    }

    fn hold(&self, bytes: usize) -> Result<(), PdfError> {
        let used = self.added.get().saturating_add(bytes);
        self.added.set(used);
        if used > self.added_limit {
            return Err(self.fail(PdfError::TooLarge(format!(
                "an edit of a PDF would add more than the limit of {} of memory",
                size(self.added_limit)
            ))));
        }
        Ok(())
    }
}

/// What `object` holds, counted as a read counts it: the object where it
/// sits, and what it owns (the bytes of a stream too, which an object made for
/// an edit owns).
pub fn memory_of(object: &Object) -> usize {
    fn held(object: &Object, depth: usize) -> usize {
        if depth > MAX_DEPTH {
            return 0;
        }
        let entries = |dict: &Dictionary| -> usize {
            dict.iter()
                .map(|(key, value)| {
                    ENTRY_MEMORY + block(key.heap_len()) + OBJECT_MEMORY + held(value, depth + 1)
                })
                .sum::<usize>()
                .saturating_add(dictionary_extra(dict.len()))
        };
        match object {
            Object::Name(name) => block(name.heap_len()),
            Object::String(bytes, _) => block(bytes.len()),
            Object::Array(items) => items
                .iter()
                .map(|item| OBJECT_MEMORY + held(item, depth + 1))
                .sum::<usize>()
                .saturating_add(items_block(items.len())),
            Object::Dictionary(dict) => entries(dict),
            Object::Stream(stream) => block(std::mem::size_of::<Stream>())
                .saturating_add(stream.content.len())
                .saturating_add(entries(&stream.dict)),
            _ => 0,
        }
    }
    OBJECT_MEMORY + held(object, 0)
}

/// A document's objects and trailer, held in memory to be changed and
/// saved: what merging, stamping and other edits work on.
#[derive(Clone, Debug, Default)]
pub struct Objects {
    /// The objects, by id.
    pub objects: BTreeMap<ObjectId, Object>,
    /// The trailer: `Root`, `Info`, `ID`. Saving sets `Size`.
    pub trailer: Dictionary,
    /// The version the header names (`1.7`).
    pub version: String,
    /// The highest object number given out.
    max_number: u32,
    /// The most saving may write, when it is held to a limit.
    output_limit: Option<usize>,
    budget: EditBudget,
    /// The limits the objects were read within, and the memory and the work
    /// reading them was counted as.
    read_limits: ReadLimits,
    read_memory: usize,
    read_work: usize,
    /// The streams hold bytes of their own, decrypted, and not the file's.
    owns_stream_bytes: bool,
}

impl Objects {
    /// No objects, version 1.7.
    pub fn new() -> Objects {
        Objects {
            version: "1.7".into(),
            ..Objects::default()
        }
    }

    /// Every object `document` can read, and its trailer, moved out of it:
    /// an error when a limit runs out while reading them, or when an object
    /// that cannot be read is one the document needs (one its page tree,
    /// contents or resources lead to): what is saved would lose it without a
    /// word. Any other that cannot be read is left out, and so is one a later
    /// section of the cross-reference marks free, unless the trailer leads to
    /// it. The objects keep their numbers, unless one is over 8,388,607: then
    /// they are numbered again from 1.
    ///
    /// Saving them is held to the bytes of the file plus
    /// [`ReadLimits::memory`](crate::ReadLimits::memory) (see
    /// [`output_limit`](Self::output_limit)). What is done to them is held to
    /// budgets of its own, as much again as the document's: the work of the
    /// lookups, and the memory added (see [`check`](Self::check)).
    pub fn read(document: Document) -> Result<Objects, PdfError> {
        let mut trailer = document.trailer().clone();
        for key in [&b"Size"[..], b"Prev", b"XRefStm", b"Encrypt"] {
            trailer.remove(key);
        }
        let version = if document.version().is_empty() {
            "1.7".into()
        } else {
            document.version().to_owned()
        };
        let listed = document.max_number().min(MAX_KEPT_NUMBER);
        let limits = document.limits();
        let output_limit = document.file_len().saturating_add(limits.memory);
        let owns_stream_bytes = document.is_encrypted();
        let ReadObjects {
            objects,
            lost,
            freed,
            memory,
            work,
        } = document.into_objects()?;
        let highest = objects.keys().map(|id| id.0).max().unwrap_or(0);
        let mut read = Objects {
            objects,
            trailer,
            version,
            max_number: highest.max(listed),
            output_limit: Some(output_limit),
            budget: EditBudget {
                work_limit: limits.work,
                added_limit: limits.memory,
                ..EditBudget::default()
            },
            read_limits: limits,
            read_memory: memory,
            read_work: work,
            owns_stream_bytes,
        };
        read.name_tree_nodes();
        if let Some((number, generation)) = read.first_needed(&lost) {
            return Err(PdfError::damaged(format!(
                "object {number} {generation} cannot be read, and the document needs it"
            )));
        }
        if !freed.is_empty() {
            let reached = read.reached();
            read.objects
                .retain(|id, _| !freed.contains(id) || reached.contains(id));
        }
        if highest > MAX_KEPT_NUMBER {
            read.renumber(1);
        }
        Ok(read)
    }

    /// The id the page tree's root is at, through references to references.
    fn tree_root(&self) -> Option<ObjectId> {
        let catalog = self.followed(self.trailer.get(b"Root")?)?.1?.as_dict()?;
        Some(self.followed(catalog.get(b"Pages")?)?.0)
    }

    /// Gives the `Type` the format requires to the nodes of the page tree that
    /// have none: what is saved is valid where the file was not. Each node is
    /// put on the stack once, and the kids looked at are held to what
    /// [`pages`](Self::pages) looks at.
    fn name_tree_nodes(&mut self) {
        let Some(root) = self.tree_root() else {
            return;
        };
        let mut steps = self.objects.len();
        let mut seen = HashSet::from([root]);
        let mut stack = vec![root];
        let mut unnamed: Vec<(ObjectId, &str)> = Vec::new();
        'walk: while let Some(node) = stack.pop() {
            let Some(dictionary) = self.dictionary(node) else {
                continue;
            };
            if !dictionary.has(b"Type") {
                unnamed.push((node, "Pages"));
            }
            let kids = dictionary
                .get(b"Kids")
                .and_then(|kids| self.resolve(kids))
                .and_then(Object::as_array)
                .map_or(&[][..], Vec::as_slice);
            for kid in kids {
                if steps == 0 {
                    break 'walk;
                }
                steps -= 1;
                let Some((id, kid)) = self.followed(kid) else {
                    continue;
                };
                let Some(kid) = kid.and_then(Object::as_dict) else {
                    continue;
                };
                if !seen.insert(id) {
                    continue;
                }
                match node_kind(kid, &|value| self.resolve(value)) {
                    NodeKind::Tree => stack.push(id),
                    NodeKind::Page if !kid.has(b"Type") => unnamed.push((id, "Page")),
                    _ => {}
                }
            }
        }
        for (id, name) in unnamed {
            if let Some(dictionary) = self.dictionary_mut(id) {
                dictionary.set("Type", name);
            }
        }
    }

    /// One of `ids` that a document's pages need, if any: a node of the page
    /// tree, or what the nodes' contents, resources and boxes lead to. What
    /// else a page refers to (`Annots`, `Thumb`) can go without.
    fn first_needed(&self, ids: &HashSet<ObjectId>) -> Option<ObjectId> {
        if ids.is_empty() {
            return None;
        }
        let root = self.trailer.get(b"Root").and_then(Object::as_reference)?;
        if ids.contains(&root) {
            return Some(root);
        }
        let catalog = self.followed(&Object::Reference(root))?.1?.as_dict()?;
        let mut nodes: Vec<ObjectId> = Vec::new();
        let mut needed: Vec<ObjectId> = Vec::new();
        let (mut seen_nodes, mut seen) = (HashSet::new(), HashSet::new());
        let mut node_of = |value: &Object, nodes: &mut Vec<ObjectId>| {
            references(value, 0, &mut |id| {
                if seen_nodes.insert(id) {
                    nodes.push(id);
                }
            });
        };
        node_of(catalog.get(b"Pages")?, &mut nodes);
        while let Some(id) = nodes.pop() {
            if ids.contains(&id) {
                return Some(id);
            }
            let Some(node) = self.objects.get(&id) else {
                continue;
            };
            let Object::Dictionary(node) = node else {
                // A reference to a list of kids, or to a node.
                node_of(node, &mut nodes);
                continue;
            };
            if let Some(kids) = node.get(b"Kids") {
                node_of(kids, &mut nodes);
            }
            for key in NEEDED_KEYS {
                if let Some(value) = node.get(key) {
                    references(value, 0, &mut |id| {
                        if seen.insert(id) {
                            needed.push(id);
                        }
                    });
                }
            }
        }
        while let Some(id) = needed.pop() {
            if ids.contains(&id) {
                return Some(id);
            }
            if let Some(object) = self.objects.get(&id) {
                references(object, 0, &mut |id| {
                    if seen.insert(id) {
                        needed.push(id);
                    }
                });
            }
        }
        None
    }

    /// What the trailer leads to.
    fn reached(&self) -> HashSet<ObjectId> {
        let mut reached: HashSet<ObjectId> = HashSet::new();
        let mut queue: Vec<ObjectId> = Vec::new();
        for (_, value) in self.trailer.iter() {
            references(value, 0, &mut |id| {
                if reached.insert(id) {
                    queue.push(id);
                }
            });
        }
        while let Some(id) = queue.pop() {
            if let Some(object) = self.objects.get(&id) {
                references(object, 0, &mut |id| {
                    if reached.insert(id) {
                        queue.push(id);
                    }
                });
            }
        }
        reached
    }

    /// The first limit that ran out on what was done to these objects, if one
    /// did: [`save`](Self::save) returns it too. Objects from
    /// [`read`](Self::read) may take as much work to look up, and add as much
    /// memory, as the document could take to read
    /// ([`ReadLimits::work`](crate::ReadLimits::work) and
    /// [`ReadLimits::memory`](crate::ReadLimits::memory)); objects from
    /// [`new`](Self::new) are the app's own to size.
    pub fn check(&self) -> Result<(), PdfError> {
        match self.budget.failure.borrow().as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// Counts `bytes` of memory about to be added, before it is: an error when
    /// that passes the limit.
    pub fn hold(&self, bytes: usize) -> Result<(), PdfError> {
        self.budget.hold(bytes)
    }

    /// The memory counted as added so far.
    pub fn memory_added(&self) -> usize {
        self.budget.added.get()
    }

    /// The memory counted for these objects: what reading them was counted as
    /// ([`Document::memory_used`] when all of them were read), and what has
    /// been added since.
    pub fn memory_counted(&self) -> usize {
        self.read_memory.saturating_add(self.budget.added.get())
    }

    /// The work counted for these objects: what reading them was counted as
    /// ([`Document::work_used`]), and what looking them up has taken since.
    pub fn work_counted(&self) -> usize {
        self.read_work.saturating_add(self.budget.work.get())
    }

    /// Sets the most memory an edit may add.
    pub fn set_added_limit(&mut self, limit: usize) {
        self.budget.added_limit = limit;
    }

    /// Sets the most work looking objects up, compressing and the like may
    /// take.
    pub fn set_work_limit(&mut self, limit: usize) {
        self.budget.work_limit = limit;
    }

    /// Holds what is done to these objects to what is left of the limits they
    /// were read within after reading them, instead of to as much again: the
    /// memory added and the work done count with what reading them counted, so
    /// that the whole is within one document's limits.
    pub fn share_limits(&mut self) {
        self.budget.added_limit = self.read_limits.memory.saturating_sub(self.read_memory);
        self.budget.work_limit = self.read_limits.work.saturating_sub(self.read_work);
    }

    /// The limits these objects were read within.
    pub fn limits(&self) -> ReadLimits {
        self.read_limits
    }

    /// What is left of the limits these objects were read within, after what
    /// reading and editing them counted: the limits to read another document
    /// within along with them.
    pub fn limits_left(&self) -> ReadLimits {
        ReadLimits {
            memory: self
                .read_limits
                .memory
                .saturating_sub(self.memory_counted()),
            work: self.read_limits.work.saturating_sub(self.work_counted()),
            ..self.read_limits
        }
    }

    /// Counts `bytes` of work done for these objects, such as reading another
    /// document along with them: an error when that passes the limit.
    pub fn spend_work(&self, bytes: usize) -> Result<(), PdfError> {
        self.budget.work(bytes)
    }

    /// The work counted so far: what looking objects up took.
    pub fn work_used(&self) -> usize {
        self.budget.work.get()
    }

    /// The highest object number given out.
    pub fn max_number(&self) -> u32 {
        self.max_number
    }

    /// A new object id, for an object inserted later.
    pub fn reserve(&mut self) -> ObjectId {
        self.max_number = self.max_number.saturating_add(1);
        (self.max_number, 0)
    }

    /// Adds `object` with a new id, counted as memory added (see
    /// [`check`](Self::check)).
    pub fn add(&mut self, object: impl Into<Object>) -> ObjectId {
        let id = self.reserve();
        self.insert(id, object);
        id
    }

    /// Puts `object` at `id`, counted as memory added (see
    /// [`check`](Self::check)).
    pub fn insert(&mut self, id: ObjectId, object: impl Into<Object>) {
        let object = object.into();
        // The error is kept for `check` and for saving.
        let _ = self.budget.hold(memory_of(&object) + TABLE_MEMORY);
        self.adopt(id, object);
    }

    /// Puts at `id` an object already counted: one read by another
    /// [`Objects`], or held with [`hold`](Self::hold).
    pub fn adopt(&mut self, id: ObjectId, object: Object) {
        self.max_number = self.max_number.max(id.0);
        self.objects.insert(id, object);
    }

    /// What `object`, one of these objects, holds when it moves to other
    /// [`Objects`] with [`adopt`](Self::adopt), its place in their table
    /// included, for the caller to take from the limits it reads the rest
    /// within. The bytes of a stream are left out: they are the file's own,
    /// which the caller is holding anyway, and moving the stream copies none of
    /// them. A decrypted stream's are its own, and count.
    pub fn memory_kept(&self, object: &Object) -> usize {
        let counted = memory_of(object).saturating_add(TABLE_MEMORY);
        match object {
            Object::Stream(stream) if !self.owns_stream_bytes => {
                counted.saturating_sub(stream.content.len())
            }
            _ => counted,
        }
    }

    /// The ids of the objects `roots` lead to, themselves included.
    pub fn reachable_from(&self, roots: impl IntoIterator<Item = ObjectId>) -> HashSet<ObjectId> {
        let mut reached: HashSet<ObjectId> = HashSet::new();
        let mut queue: Vec<ObjectId> = Vec::new();
        for id in roots {
            if reached.insert(id) {
                queue.push(id);
            }
        }
        while let Some(id) = queue.pop() {
            if let Some(object) = self.objects.get(&id) {
                references(object, 0, &mut |id| {
                    if reached.insert(id) {
                        queue.push(id);
                    }
                });
            }
        }
        reached
    }

    /// Counts one lookup; `false` once the work has run out.
    fn looked_up(&self) -> bool {
        self.budget.work(LOOKUP_COST).is_ok()
    }

    /// Object `id`. Each lookup is counted (see [`check`](Self::check)), and
    /// once the work has run out there is no object.
    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        if !self.looked_up() {
            return None;
        }
        self.objects.get(&id)
    }

    /// Object `id`, to change.
    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        if !self.looked_up() {
            return None;
        }
        self.objects.get_mut(&id)
    }

    /// The dictionary object `id` is.
    pub fn dictionary(&self, id: ObjectId) -> Option<&Dictionary> {
        self.get(id).and_then(Object::as_dict)
    }

    /// The dictionary object `id` is, to change.
    pub fn dictionary_mut(&mut self, id: ObjectId) -> Option<&mut Dictionary> {
        self.get_mut(id).and_then(Object::as_dict_mut)
    }

    /// `object`, or the object it refers to (through up to 8 references).
    pub fn resolve<'a>(&'a self, object: &'a Object) -> Option<&'a Object> {
        let mut object = object;
        for _ in 0..MAX_HOPS {
            match object {
                Object::Reference(id) => object = self.get(*id)?,
                other => return Some(other),
            }
        }
        match object {
            Object::Reference(_) => None,
            other => Some(other),
        }
    }

    /// Where the reference `object` leads, through references to references:
    /// the id, and the object when there is one. `None` for what is no
    /// reference, or a chain over the limit.
    pub fn followed(&self, object: &Object) -> Option<(ObjectId, Option<&Object>)> {
        let mut id = object.as_reference()?;
        for _ in 0..MAX_HOPS {
            match self.get(id) {
                Some(Object::Reference(next)) => id = *next,
                other => return Some((id, other)),
            }
        }
        None
    }

    /// The pages, in order, read as [`Document::pages`] reads them.
    pub fn pages(&self) -> Vec<ObjectId> {
        let mut pages = Vec::new();
        let Some(root) = self.tree_root() else {
            return pages;
        };
        let kids = |node: ObjectId| -> &[Object] {
            self.dictionary(node)
                .and_then(|node| self.resolve(node.get(b"Kids")?))
                .and_then(Object::as_array)
                .map_or(&[], Vec::as_slice)
        };
        let mut steps = self.objects.len();
        let mut stack: Vec<&[Object]> = Vec::new();
        let mut current = kids(root);
        loop {
            while let Some((kid, rest)) = current.split_first() {
                if steps == 0 {
                    return pages;
                }
                steps -= 1;
                current = rest;
                let Some((id, node)) = self.followed(kid) else {
                    continue;
                };
                match node
                    .and_then(Object::as_dict)
                    .map(|node| node_kind(node, &|value| self.resolve(value)))
                {
                    Some(NodeKind::Page) => pages.push(id),
                    Some(NodeKind::Tree) if stack.len() < MAX_TREE_DEPTH => {
                        if !current.is_empty() {
                            stack.push(current);
                        }
                        current = kids(id);
                    }
                    Some(_) => {}
                    None => pages.push(id),
                }
            }
            match stack.pop() {
                Some(next) => current = next,
                None => return pages,
            }
        }
    }

    /// Gives every object a new number from `first` on, in the order of
    /// their ids, generation 0, and changes every reference to match. A
    /// reference to an object that does not exist becomes `null`. Returns
    /// each old id's new one.
    pub fn renumber(&mut self, first: u32) -> HashMap<ObjectId, ObjectId> {
        let map: HashMap<ObjectId, ObjectId> = self
            .objects
            .keys()
            .zip(first..)
            .map(|(&old, new)| (old, (new, 0)))
            .collect();
        let objects = std::mem::take(&mut self.objects);
        self.objects = objects
            .into_iter()
            .map(|(id, mut object)| {
                renumber_in(&mut object, &map, 0);
                (map.get(&id).copied().unwrap_or(id), object)
            })
            .collect();
        for (_, value) in self.trailer.iter_mut() {
            renumber_in(value, &map, 0);
        }
        self.max_number = self
            .objects
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .max(first.saturating_sub(1));
        map
    }

    /// Drops the objects nothing reachable from the trailer refers to.
    pub fn prune(&mut self) {
        let reached = self.reached();
        self.objects.retain(|id, _| reached.contains(id));
    }

    /// Compresses (Flate) every stream of 64 bytes or more that has no filter
    /// (and no parameters for one, nor a file its bytes are in), when that makes
    /// it smaller, as long as there is work left for it: it takes 4 KiB and 2
    /// bytes for each byte. A stream past that stays as it is.
    pub fn compress(&mut self) {
        let budget = &self.budget;
        for object in self.objects.values_mut() {
            if let Object::Stream(stream) = object
                && stream.content.len() >= SHORTEST_COMPRESSED
                && !DESCRIBES_BYTES.iter().any(|key| stream.dict.has(key))
            {
                let cost = stream
                    .content
                    .len()
                    .saturating_mul(COMPRESS_PER_BYTE)
                    .saturating_add(COMPRESS_COST);
                if budget.work.get().saturating_add(cost) > budget.work_limit {
                    continue;
                }
                budget.work.set(budget.work.get() + cost);
                let packed = filters::deflate(&stream.content);
                if packed.len() + 19 < stream.content.len() {
                    stream.dict.set("Filter", "FlateDecode");
                    stream.content = packed.into();
                }
            }
        }
    }

    /// The most [`save`](Self::save) and [`save_with`](Self::save_with) may
    /// write. Objects from [`new`](Self::new) have no limit: what an app
    /// writes itself is its own to size. Objects from [`read`](Self::read)
    /// may write the bytes of the file they came from plus
    /// [`ReadLimits::memory`](crate::ReadLimits::memory), so that an edit of a
    /// small file cannot write a large one.
    pub fn output_limit(&self) -> usize {
        self.output_limit.unwrap_or(usize::MAX)
    }

    /// Sets the most [`save`](Self::save) may write: for an edit that takes
    /// its objects from more than one document, the sum of what each may.
    pub fn set_output_limit(&mut self, limit: usize) {
        self.output_limit = Some(limit);
    }

    /// The document as a PDF, with a cross-reference table.
    /// [`PdfError::TooLarge`] for one over its
    /// [`output_limit`](Self::output_limit), or too large for the offsets a
    /// cross-reference can hold.
    pub fn save(&self) -> Result<Vec<u8>, PdfError> {
        self.save_with(false)
    }

    /// The document as a PDF; with `object_streams`, the objects other than
    /// streams go into compressed object streams, and the cross-reference
    /// is a stream. Held to the [`output_limit`](Self::output_limit), checked
    /// before each object is written.
    pub fn save_with(&self, object_streams: bool) -> Result<Vec<u8>, PdfError> {
        self.check()?;
        let limit = self.output_limit();
        let over = |written: usize| -> Result<(), PdfError> {
            if written > limit {
                return Err(PdfError::TooLarge(format!(
                    "an edit of a PDF would write more than the limit of {}",
                    size(limit)
                )));
            }
            Ok(())
        };
        // What it will take is asked for at once, so that an output there is no
        // memory for is an error, and growing it doesn't hold two copies of it.
        let room = |out: &mut Vec<u8>, bytes: usize| {
            out.try_reserve(bytes).map_err(|_| {
                PdfError::TooLarge(format!(
                    "there is no memory for an edit of a PDF that writes {}",
                    size(out.len().saturating_add(bytes))
                ))
            })
        };
        let expected = self.objects.values().fold(0usize, |sum, object| {
            sum.saturating_add(memory_of(object))
        });
        let mut out = Vec::new();
        room(&mut out, expected.min(limit))?;
        let _ = write!(out, "%PDF-{}\n%\u{e2}\u{e3}\u{cf}\u{d3}\n", self.version);
        // Where each object is: at an offset (with its generation), or in
        // an object stream (with its index there).
        let mut entries: BTreeMap<u32, (u8, u64, u16)> = BTreeMap::new();
        let too_large = || PdfError::TooLarge("a PDF too large for its cross-reference".into());
        let mut next = self.max_number;
        let mut members: Vec<(&ObjectId, &Object)> = Vec::new();
        for (id, object) in &self.objects {
            if object_streams && id.1 == 0 && !matches!(object, Object::Stream(_)) {
                members.push((id, object));
            } else {
                if let Object::Stream(stream) = object {
                    over(out.len().saturating_add(stream.content.len()))?;
                }
                room(&mut out, memory_of(object))?;
                entries.insert(id.0, (1, out.len() as u64, id.1));
                write_indirect(&mut out, *id, object);
                over(out.len())?;
            }
        }
        for group in members.chunks(OBJECTS_PER_STREAM) {
            next = next.checked_add(1).ok_or_else(too_large)?;
            let mut header = Vec::new();
            let mut body = Vec::new();
            for (index, (id, object)) in group.iter().enumerate() {
                let _ = write!(header, "{} {} ", id.0, body.len());
                write_object(&mut body, object);
                body.push(b'\n');
                over(out.len().saturating_add(body.len()))?;
                entries.insert(id.0, (2, u64::from(next), index as u16));
            }
            let first = header.len();
            header.extend_from_slice(&body);
            let mut dict = Dictionary::new();
            dict.set("Type", "ObjStm");
            dict.set("N", group.len() as i64);
            dict.set("First", first as i64);
            dict.set("Filter", "FlateDecode");
            let stream = Stream::new(dict, filters::deflate(&header));
            entries.insert(next, (1, out.len() as u64, 0));
            write_indirect(&mut out, (next, 0), &Object::from(stream));
        }
        let mut trailer = self.trailer.clone();
        for key in [&b"Prev"[..], b"XRefStm", b"Encrypt"] {
            trailer.remove(key);
        }
        let start = out.len() as u64;
        if object_streams {
            // The cross-reference stream is the last object.
            let own = next.checked_add(1).ok_or_else(too_large)?;
            entries.insert(own, (1, start, 0));
            entries.insert(0, (0, 0, 65535));
            trailer.set("Size", i64::from(own) + 1);
            let mut rows = Vec::with_capacity(entries.len() * 7);
            for &(kind, second, third) in entries.values() {
                rows.push(kind);
                rows.extend_from_slice(
                    &u32::try_from(second)
                        .map_err(|_| too_large())?
                        .to_be_bytes(),
                );
                rows.extend_from_slice(&third.to_be_bytes());
            }
            trailer.set("Type", "XRef");
            trailer.set(
                "W",
                vec![Object::Integer(1), Object::Integer(4), Object::Integer(2)],
            );
            trailer.set("Index", runs(entries.keys()));
            trailer.set("Filter", "FlateDecode");
            let stream = Stream::new(trailer, filters::deflate(&rows));
            write_indirect(&mut out, (own, 0), &Object::from(stream));
            over(out.len())?;
        } else {
            entries.insert(0, (0, 0, 65535));
            trailer.set("Size", i64::from(next) + 1);
            out.extend_from_slice(b"xref\n");
            let numbers: Vec<u32> = entries.keys().copied().collect();
            for run in numbers.chunk_by(|a, b| a.checked_add(1) == Some(*b)) {
                let _ = writeln!(out, "{} {}", run[0], run.len());
                for number in run {
                    let (kind, offset, generation) =
                        entries.get(number).copied().unwrap_or((0, 0, 0));
                    if offset > 9_999_999_999 {
                        return Err(too_large());
                    }
                    let kind = if kind == 0 { 'f' } else { 'n' };
                    let _ = write!(out, "{offset:010} {generation:05} {kind}\r\n");
                }
            }
            out.extend_from_slice(b"trailer\n");
            write_object(&mut out, &Object::Dictionary(trailer));
            out.push(b'\n');
        }
        let _ = write!(out, "startxref\n{start}\n%%EOF\n");
        over(out.len())?;
        out.shrink_to_fit();
        Ok(out)
    }
}

/// The runs of consecutive `numbers`, as `/Index` lists them: first number,
/// count.
fn runs<'a>(numbers: impl Iterator<Item = &'a u32>) -> Vec<Object> {
    let numbers: Vec<u32> = numbers.copied().collect();
    numbers
        .chunk_by(|a, b| a.checked_add(1) == Some(*b))
        .flat_map(|run| [Object::from(run[0]), Object::Integer(run.len() as i64)])
        .collect()
}

/// Replaces every reference in `object` through `map`.
fn renumber_in(object: &mut Object, map: &HashMap<ObjectId, ObjectId>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match object {
        Object::Reference(id) => match map.get(id) {
            Some(new) => *id = *new,
            None => *object = Object::Null,
        },
        Object::Array(items) => {
            for item in items {
                renumber_in(item, map, depth + 1);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter_mut() {
                renumber_in(value, map, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter_mut() {
                renumber_in(value, map, depth + 1);
            }
        }
        _ => {}
    }
}

/// Calls `found` with every reference in `object`.
fn references(object: &Object, depth: usize, found: &mut dyn FnMut(ObjectId)) {
    if depth > MAX_DEPTH {
        return;
    }
    match object {
        Object::Reference(id) => found(*id),
        Object::Array(items) => {
            for item in items {
                references(item, depth + 1, found);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter() {
                references(value, depth + 1, found);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                references(value, depth + 1, found);
            }
        }
        _ => {}
    }
}

fn write_indirect(out: &mut Vec<u8>, (number, generation): ObjectId, object: &Object) {
    let _ = writeln!(out, "{number} {generation} obj");
    write_object(out, object);
    out.extend_from_slice(b"\nendobj\n");
}

/// Writes `object` as PDF syntax. A stream's `Length` is set from its
/// bytes.
pub(crate) fn write_object(out: &mut Vec<u8>, object: &Object) {
    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(value) => out.extend_from_slice(if *value { b"true" } else { b"false" }),
        Object::Integer(value) => {
            let _ = write!(out, "{value}");
        }
        Object::Real(value) => write_real(out, *value),
        Object::Name(name) => write_name(out, name),
        Object::String(bytes, StringFormat::Literal) => {
            out.push(b'(');
            for &byte in bytes {
                match byte {
                    b'(' | b')' | b'\\' => {
                        out.push(b'\\');
                        out.push(byte);
                    }
                    b'\r' => out.extend_from_slice(b"\\r"),
                    _ => out.push(byte),
                }
            }
            out.push(b')');
        }
        Object::String(bytes, StringFormat::Hexadecimal) => {
            out.push(b'<');
            for byte in bytes {
                let _ = write!(out, "{byte:02X}");
            }
            out.push(b'>');
        }
        Object::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b' ');
                }
                write_object(out, item);
            }
            out.push(b']');
        }
        Object::Dictionary(dict) => write_dictionary(out, dict),
        Object::Stream(stream) => {
            let mut dict = stream.dict.clone();
            dict.set("Length", stream.content.len() as i64);
            write_dictionary(out, &dict);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&stream.content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((number, generation)) => {
            let _ = write!(out, "{number} {generation} R");
        }
    }
}

fn write_dictionary(out: &mut Vec<u8>, dict: &Dictionary) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        write_name(out, key);
        out.push(b' ');
        write_object(out, value);
    }
    out.extend_from_slice(b">>");
}

/// A name, with `#xx` for every byte that is not a regular printable
/// character.
pub(crate) fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &byte in name {
        if is_regular(byte) && byte != b'#' && (0x21..=0x7E).contains(&byte) {
            out.push(byte);
        } else {
            let _ = write!(out, "#{byte:02X}");
        }
    }
}

/// A real without an exponent, with a decimal point so that it reads back
/// as a real; one that is not a number is 0.
pub(crate) fn write_real(out: &mut Vec<u8>, value: f32) {
    if value.is_finite() {
        let start = out.len();
        let _ = write!(out, "{value}");
        if !out.get(start..).unwrap_or_default().contains(&b'.') {
            out.extend_from_slice(b".0");
        }
    } else {
        out.push(b'0');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary;
    use crate::parse;

    fn written(object: &Object) -> Vec<u8> {
        let mut out = Vec::new();
        write_object(&mut out, object);
        out
    }

    #[test]
    fn objects_written_read_back_the_same() {
        let objects = [
            Object::Null,
            Object::Boolean(true),
            Object::Integer(-42),
            Object::Real(0.5),
            Object::Real(-1234.25),
            Object::name("A B#(c)"),
            Object::string_literal(b"a (b) \\ c\r\nd".to_vec()),
            Object::String(vec![0xFE, 0xFF, 0, 0x41], StringFormat::Hexadecimal),
            Object::Array(vec![1.into(), Object::Reference((3, 1)), "N".into()]),
            Object::Dictionary(
                dictionary! { "Type" => "Page", "Kids" => vec![Object::Reference((2, 0))] },
            ),
        ];
        for object in objects {
            let bytes = written(&object);
            assert_eq!(
                parse::object(&bytes).as_ref(),
                Some(&object),
                "{}",
                String::from_utf8_lossy(&bytes)
            );
        }
        assert_eq!(written(&Object::Real(1e-7)), b"0.0000001");
        assert_eq!(written(&Object::Real(12.0)), b"12.0");
        assert_eq!(written(&Object::Real(f32::NAN)), b"0");
    }

    #[test]
    fn renumbering_moves_every_reference() {
        let mut objects = Objects::new();
        let a = objects.reserve();
        let b = objects.add(dictionary! { "Next" => a, "Gone" => Object::Reference((99, 0)) });
        objects.insert(a, dictionary! { "Back" => b });
        objects.trailer.set("Root", b);
        objects.renumber(10);
        assert_eq!(
            objects.trailer.get(b"Root"),
            Some(&Object::Reference((11, 0)))
        );
        let moved = objects.dictionary((11, 0)).unwrap();
        assert_eq!(moved.get(b"Next"), Some(&Object::Reference((10, 0))));
        assert_eq!(moved.get(b"Gone"), Some(&Object::Null));
        assert_eq!(objects.max_number(), 11);
    }

    #[test]
    fn a_document_with_the_highest_object_numbers_is_renumbered_to_be_edited() {
        let mut objects = Objects::new();
        let (page, pages, catalog) = ((u32::MAX, 0), (u32::MAX - 1, 0), (u32::MAX - 2, 0));
        objects.insert(page, dictionary! { "Type" => "Page", "Parent" => pages });
        objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        );
        objects.insert(
            catalog,
            dictionary! { "Type" => "Catalog", "Pages" => pages },
        );
        objects.trailer.set("Root", catalog);
        // With object streams, there is no number left for them.
        assert!(matches!(
            objects.save_with(true),
            Err(PdfError::TooLarge(_))
        ));
        let saved = objects.save().unwrap();
        let document = Document::load(saved, crate::ReadLimits::default()).unwrap();
        assert_eq!(document.pages(), [page]);
        let mut read = Objects::read(document).unwrap();
        assert_eq!(read.max_number(), 3);
        assert_eq!(read.reserve(), (4, 0));
        assert_eq!(read.pages().len(), 1);
        read.save_with(true).unwrap();
    }

    #[test]
    fn pruning_keeps_what_the_trailer_reaches() {
        let mut objects = Objects::new();
        let kept = objects.add(Object::Integer(1));
        let root = objects.add(dictionary! { "K" => kept });
        objects.add(Object::Integer(2));
        objects.trailer.set("Root", root);
        objects.prune();
        assert_eq!(objects.objects.len(), 2);
    }

    #[test]
    fn compressing_leaves_a_stream_alone_when_its_dictionary_describes_its_bytes() {
        for key in ["DecodeParms", "DP", "F", "FFilter", "FDecodeParms"] {
            let mut objects = Objects::new();
            let mut dict = Dictionary::new();
            dict.set(key, dictionary! { "Predictor" => 12, "Columns" => 4 });
            let data = b"BT /F1 12 Tf 72 700 Td (padded, padded, padded, padded, padded) Tj ET";
            let id = objects.add(Stream::new(dict.clone(), data.to_vec()));
            objects.compress();
            let stream = objects.get(id).and_then(Object::as_stream).unwrap();
            assert_eq!(&stream.content[..], &data[..], "{key}");
            assert_eq!(stream.dict, dict, "{key}");
        }
        // Without them, the same stream is compressed.
        let mut objects = Objects::new();
        let id = objects.add(Stream::new(
            Dictionary::new(),
            b"BT /F1 12 Tf 72 700 Td (padded, padded, padded, padded, padded) Tj ET
"
            .repeat(8),
        ));
        objects.compress();
        let stream = objects.get(id).and_then(Object::as_stream).unwrap();
        assert_eq!(stream.filters(), [&b"FlateDecode"[..]]);
    }
}
