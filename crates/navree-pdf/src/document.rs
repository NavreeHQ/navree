//! [`Document`]: a PDF read lazily, within its [`ReadLimits`].

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};

use bytes::Bytes;

use crate::budget::{Budget, ReadLimits};
use crate::crypt::Crypt;
use crate::error::PdfError;
use crate::filters::{self, DecodeError};
use crate::object::{Dictionary, Object, ObjectId, Stream};
use crate::parse::{Fail, Lexer, MAX_DEPTH};
use crate::xref::{self, Entry};

/// How many references are followed to reach an object (128).
pub(crate) const MAX_HOPS: usize = 128;
/// How deep reading one object may lead to reading others (a stream's
/// `Length` in another object, an object stream): 32.
const MAX_LOADING: usize = 32;
/// How deep the page tree may nest (256).
const MAX_TREE_DEPTH: usize = 256;
/// What reading a stream object holds besides its dictionary: the shared
/// handle on its bytes.
const STREAM_MEMORY: usize = 32;
/// What an object stream's list holds for each object in it, with room to
/// grow: a number and offset, in the list and in its index by number.
const MEMBER_MEMORY: usize = 72;
/// What each entry of a page's `Contents` costs to look at, in bytes of work.
pub(crate) const CONTENTS_ENTRY_COST: usize = 16;

/// An object stream, inflated: its objects' numbers and where they start.
struct Container {
    data: Vec<u8>,
    /// Where the objects start (`First`).
    first: usize,
    /// The object numbers and offsets the stream lists, in order.
    members: Vec<(u32, usize)>,
    /// Where each number is, for an entry whose index is wrong; the last one
    /// listed when a number is listed twice.
    by_number: HashMap<u32, usize>,
}

/// What a scan of the file found: where each object is, and where objects
/// start, in order.
struct Scanned {
    entries: BTreeMap<u32, Entry>,
    offsets: Vec<usize>,
}

/// One entry of the cross-reference, and its object once read.
struct Slot {
    number: u32,
    entry: Entry,
    object: OnceCell<Option<Object>>,
}

/// A PDF document read lazily: the cross-reference when it is loaded, each
/// object the first time it is asked for, all of it held to the document's
/// [`ReadLimits`].
///
/// Asking for an object never fails: an object that is damaged or missing
/// is `None`, as it would be for a reader that skips it. When a limit runs
/// out, every later request is `None` too, and [`check`](Self::check)
/// returns the error: callers check once they are done.
///
/// ```
/// use navree_pdf::{Document, ReadLimits};
///
/// # fn main() -> Result<(), navree_pdf::PdfError> {
/// let mut objects = navree_pdf::Objects::new();
/// let pages = objects.reserve();
/// let page = objects.add(navree_pdf::dictionary! { "Type" => "Page", "Parent" => pages });
/// objects.insert(pages, navree_pdf::dictionary! {
///     "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
/// });
/// let catalog = objects.add(navree_pdf::dictionary! { "Type" => "Catalog", "Pages" => pages });
/// objects.trailer.set("Root", catalog);
///
/// let document = Document::load(objects.save()?, ReadLimits::default())?;
/// assert_eq!(document.pages().len(), 1);
/// document.check()?;
/// # Ok(())
/// # }
/// ```
pub struct Document {
    data: Bytes,
    version: String,
    trailer: Dictionary,
    slots: Vec<Slot>,
    /// Where objects start, in order: one ends where the next begins.
    offsets: Vec<usize>,
    containers: BTreeMap<u32, OnceCell<Option<Container>>>,
    budget: Budget,
    crypt: Option<Crypt>,
    /// The `Encrypt` dictionary's object, which is not encrypted.
    encrypt_id: Option<ObjectId>,
    /// The first limit that ran out.
    failure: RefCell<Option<PdfError>>,
    /// The objects being read, innermost last.
    loading: RefCell<Vec<u32>>,
    /// Whether the cross-reference was rebuilt from a scan of the file.
    rebuilt: bool,
    /// The scan of a file whose cross-reference points at the wrong place,
    /// made the first time an object is not where it should be.
    scanned: OnceCell<Scanned>,
    depth: Cell<usize>,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("version", &self.version)
            .field("objects", &self.slots.len())
            .field("bytes", &self.data.len())
            .finish()
    }
}

impl Document {
    /// Reads the header, the cross-reference and the trailer of `bytes`:
    /// an error unless they are a PDF (a broken cross-reference is rebuilt
    /// from the objects in the file). Objects are read later, when asked
    /// for. A file encrypted with a password to open it is
    /// [`PdfError::Encrypted`].
    pub fn load(bytes: impl Into<Bytes>, limits: ReadLimits) -> Result<Document, PdfError> {
        let bytes = bytes.into();
        let start = xref::find(&bytes, b"%PDF-", 0)
            .ok_or_else(|| PdfError::damaged("not a PDF: the file has no %PDF- header"))?;
        let data = bytes.slice(start..);
        let version = {
            let mut lexer = Lexer::new(&data, 5);
            String::from_utf8_lossy(lexer.word()).into_owned()
        };
        let budget = Budget::new(limits);
        budget.work(start.saturating_add(data.len().min(1024)))?;
        let xref = xref::read(&data, &budget)?;
        // What the table of objects holds, one slot an object.
        let in_use = xref
            .entries
            .iter()
            .filter(|(number, entry)| **number != 0 && **entry != Entry::Free)
            .count();
        budget.memory(in_use.saturating_mul(std::mem::size_of::<Slot>()))?;
        // An object ends where the next one that is really there starts:
        // not at an entry that points at bytes that are not its object.
        let mut offsets: Vec<usize> = xref
            .located
            .iter()
            .chain(xref.starts.iter())
            .copied()
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        let containers: BTreeMap<u32, OnceCell<Option<Container>>> = xref
            .entries
            .values()
            .filter_map(|entry| match entry {
                Entry::Compressed { container, .. } => Some((*container, OnceCell::new())),
                _ => None,
            })
            .collect();
        budget.memory(
            containers
                .len()
                .saturating_mul(std::mem::size_of::<Option<Container>>() + 32),
        )?;
        let slots = xref
            .entries
            .into_iter()
            // Object 0 heads the free list, whatever a damaged file says.
            .filter(|(number, entry)| *number != 0 && *entry != Entry::Free)
            .map(|(number, entry)| Slot {
                number,
                entry,
                object: OnceCell::new(),
            })
            .collect();
        let mut document = Document {
            data,
            version,
            trailer: xref.trailer,
            slots,
            offsets,
            containers,
            budget,
            crypt: None,
            encrypt_id: None,
            failure: RefCell::new(None),
            loading: RefCell::new(Vec::new()),
            rebuilt: xref.rebuilt,
            scanned: OnceCell::new(),
            depth: Cell::new(0),
        };
        if let Some(encrypt) = document.trailer.get(b"Encrypt").cloned() {
            document.encrypt_id = encrypt.as_reference();
            let dictionary = document
                .resolve(&encrypt)
                .and_then(Object::as_dict)
                .cloned()
                .ok_or(PdfError::Encrypted)?;
            let id = document
                .trailer
                .get(b"ID")
                .and_then(Object::as_array)
                .and_then(|ids| ids.first())
                .and_then(Object::as_str)
                .unwrap_or_default()
                .to_vec();
            let crypt = Crypt::open(&dictionary, &id)?;
            // Objects read so far were read without decryption.
            for slot in &mut document.slots {
                if Some((slot.number, 0)) != document.encrypt_id.map(|(n, _)| (n, 0)) {
                    slot.object = OnceCell::new();
                }
            }
            document.crypt = Some(crypt);
            document.trailer.remove(b"Encrypt");
        }
        document.check()?;
        Ok(document)
    }

    /// The version in the header (`1.7`).
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The trailer: `Root`, `Info`, `ID`, `Size`. A file that was encrypted
    /// has no `Encrypt` here: its objects are read decrypted.
    pub fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    /// Whether the cross-reference was rebuilt by scanning the file for
    /// objects, because the one in the file could not be read.
    pub fn rebuilt(&self) -> bool {
        self.rebuilt
    }

    /// The error of the first limit that ran out, if one did.
    pub fn check(&self) -> Result<(), PdfError> {
        match self.failure.borrow().as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// Records `error` when it is a limit: reading stops there.
    fn fail(&self, error: PdfError) {
        let mut failure = self.failure.borrow_mut();
        if failure.is_none() {
            *failure = Some(error);
        }
    }

    fn failed(&self) -> bool {
        self.failure.borrow().is_some()
    }

    /// The ids of the objects in the cross-reference, in order.
    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.slots.iter().map(|slot| match slot.entry {
            Entry::Normal { generation, .. } => (slot.number, generation),
            _ => (slot.number, 0),
        })
    }

    /// The number of objects in the cross-reference.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the cross-reference lists no objects.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The highest object number in the cross-reference.
    pub fn max_number(&self) -> u32 {
        self.slots.last().map_or(0, |slot| slot.number)
    }

    /// Every object, read now, with its id; those that cannot be read are
    /// left out.
    pub fn objects(&self) -> impl Iterator<Item = (ObjectId, &Object)> + '_ {
        self.ids().filter_map(|id| Some((id, self.object(id)?)))
    }

    /// Every object that can be read, moved out of the document: what it
    /// holds is not copied. Also returns the ids of those the cross-reference
    /// lists that could not be read.
    pub(crate) fn into_objects(
        self,
    ) -> Result<(BTreeMap<ObjectId, Object>, HashSet<ObjectId>), PdfError> {
        let ids: Vec<ObjectId> = self.ids().collect();
        for &id in &ids {
            self.object(id);
        }
        self.check()?;
        let mut objects = BTreeMap::new();
        let mut lost = HashSet::new();
        for (id, slot) in ids.into_iter().zip(self.slots) {
            match slot.object.into_inner().flatten() {
                Some(object) => {
                    objects.insert(id, object);
                }
                None => {
                    lost.insert(id);
                }
            }
        }
        Ok((objects, lost))
    }

    /// The number of bytes of the file, from its header on.
    pub(crate) fn file_len(&self) -> usize {
        self.data.len()
    }

    /// Object `id`, read the first time it is asked for: `None` when it is
    /// missing or damaged (or a limit ran out: see [`check`](Self::check)).
    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        let index = self
            .slots
            .binary_search_by_key(&id.0, |slot| slot.number)
            .ok()?;
        let slot = self.slots.get(index)?;
        match slot.entry {
            Entry::Normal { generation, .. } if generation != id.1 => return None,
            Entry::Compressed { .. } if id.1 != 0 => return None,
            _ => {}
        }
        if let Some(object) = slot.object.get() {
            return object.as_ref();
        }
        if self.failed() || self.loading.borrow().contains(&id.0) || self.depth.get() >= MAX_LOADING
        {
            return None;
        }
        self.loading.borrow_mut().push(id.0);
        self.depth.set(self.depth.get() + 1);
        let read = self.read(id, slot.entry);
        self.depth.set(self.depth.get() - 1);
        self.loading.borrow_mut().pop();
        let object = match read {
            Ok(object) => object,
            Err(error) => {
                self.fail(error);
                return None;
            }
        };
        let _ = slot.object.set(object);
        slot.object.get().and_then(Option::as_ref)
    }

    /// Reads the object an entry points at, decrypted.
    fn read(&self, id: ObjectId, entry: Entry) -> Result<Option<Object>, PdfError> {
        let object = match entry {
            Entry::Free => None,
            Entry::Normal { offset, .. } => match self.read_at(id, offset)? {
                Some(object) => Some(object),
                None => match self.scanned_entry(id.0)? {
                    Some(offset) if offset != entry_offset(entry) => self.read_at(id, offset)?,
                    _ => None,
                },
            },
            Entry::Compressed { container, index } => self.read_compressed(id, container, index)?,
        };
        let Some(mut object) = object else {
            return Ok(None);
        };
        if let Some(crypt) = &self.crypt
            && matches!(entry, Entry::Normal { .. })
            && Some(id) != self.encrypt_id
        {
            let identity = match &object {
                Object::Stream(stream) => {
                    crate::crypt::opts_out(&stream.dict, &|value| self.resolve(value))
                }
                _ => false,
            };
            if let Object::Stream(stream) = &object {
                // A decrypted stream holds its own bytes.
                self.budget.memory(stream.content.len())?;
                self.budget.work(stream.content.len())?;
            }
            crypt.decrypt(id, &mut object, identity);
        }
        Ok(Some(object))
    }

    /// The object at `offset`, if it is object `id`.
    fn read_at(&self, id: ObjectId, offset: usize) -> Result<Option<Object>, PdfError> {
        let next_of = |offsets: &[usize]| {
            let next = offsets.partition_point(|&at| at <= offset);
            offsets.get(next).copied().unwrap_or(self.data.len())
        };
        // Objects a scan found end at the next one it found, too.
        let end = next_of(&self.offsets).min(
            self.scanned
                .get()
                .map_or(self.data.len(), |scanned| next_of(&scanned.offsets)),
        );
        let length = |target: ObjectId| -> Option<i64> {
            let value = self.object(target)?;
            value.as_i64().or_else(|| {
                value
                    .as_float()
                    .filter(|v| v.fract() == 0.0)
                    .map(|v| v as i64)
            })
        };
        let Some((found, object)) = xref::indirect(&self.data, offset, end, &self.budget, &length)?
        else {
            return Ok(None);
        };
        if found.0 != id.0 {
            return Ok(None);
        }
        if object.as_stream().is_some() {
            // The bytes are a handle on the file's, not a copy.
            self.budget.memory(STREAM_MEMORY)?;
        }
        Ok(Some(object))
    }

    /// Where a scan of the file finds object `number`, for an object the
    /// cross-reference puts elsewhere.
    fn scanned_entry(&self, number: u32) -> Result<Option<usize>, PdfError> {
        if self.scanned.get().is_none() {
            let entries = xref::scan(&self.data, &self.budget)?;
            self.budget
                .memory(entries.len().saturating_mul(xref::OFFSET_MEMORY))?;
            let mut offsets: Vec<usize> =
                entries.values().filter_map(xref::normal_offset).collect();
            offsets.sort_unstable();
            let _ = self.scanned.set(Scanned { entries, offsets });
        }
        Ok(self
            .scanned
            .get()
            .and_then(|scanned| xref::normal_offset(scanned.entries.get(&number)?)))
    }

    /// The `index`th object of object stream `container`, if it is object
    /// `id`.
    fn read_compressed(
        &self,
        id: ObjectId,
        container: u32,
        index: u32,
    ) -> Result<Option<Object>, PdfError> {
        let Some(cell) = self.containers.get(&container) else {
            return Ok(None);
        };
        if cell.get().is_none() {
            let loaded = self.load_container(container)?;
            let _ = cell.set(loaded);
        }
        let Some(Some(stream)) = cell.get() else {
            return Ok(None);
        };
        let offset = match stream.members.get(index as usize) {
            Some(&(number, offset)) if number == id.0 => offset,
            _ => match stream.by_number.get(&id.0) {
                Some(&offset) => offset,
                None => return Ok(None),
            },
        };
        let Some(start) = stream
            .first
            .checked_add(offset)
            .filter(|&at| at < stream.data.len())
        else {
            return Ok(None);
        };
        let mut lexer = Lexer::new(&stream.data, start);
        lexer.skip_white();
        let parsed = lexer.object(MAX_DEPTH, &self.budget);
        self.budget.work(lexer.reached().saturating_sub(start))?;
        match parsed {
            Ok(object) => Ok(Some(object)),
            Err(Fail::Over(error)) => Err(error),
            Err(_) => Ok(None),
        }
    }

    /// Inflates object stream `number` and reads its list of objects.
    /// `None` when it is missing, damaged, or inflates past
    /// [`ReadLimits::object_stream`].
    fn load_container(&self, number: u32) -> Result<Option<Container>, PdfError> {
        let Some(stream) = self.object((number, 0)).and_then(Object::as_stream) else {
            return Ok(None);
        };
        let limit = self
            .budget
            .limits
            .object_stream
            .min(self.budget.memory_left());
        let mut spent = 0;
        let decoded = self.decode_counted(stream, limit, &mut spent);
        self.budget.work(spent)?;
        let data = match decoded {
            Ok(data) => data,
            Err(ContentError::Over) if limit < self.budget.limits.object_stream => {
                return Err(self.budget.out_of_memory());
            }
            Err(_) => return Ok(None),
        };
        self.budget.memory(data.len())?;
        self.budget.work(data.len())?;
        let Some(first) = self
            .get(&stream.dict, b"First")
            .and_then(Object::as_i64)
            .and_then(|first| usize::try_from(first).ok())
            .filter(|&first| first <= data.len())
        else {
            return Ok(None);
        };
        let number = |word: &[u8]| -> Option<u64> { std::str::from_utf8(word).ok()?.parse().ok() };
        let mut words = data
            .get(..first)
            .unwrap_or_default()
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|word| !word.is_empty());
        let mut members = Vec::new();
        let mut by_number = HashMap::new();
        while let (Some(member), Some(offset)) = (words.next(), words.next()) {
            self.budget.memory(MEMBER_MEMORY)?;
            let member = number(member).and_then(|member| u32::try_from(member).ok());
            let offset = number(offset).and_then(|offset| usize::try_from(offset).ok());
            if let (Some(member), Some(offset)) = (member, offset) {
                members.push((member, offset));
                by_number.insert(member, offset);
            }
        }
        Ok(Some(Container {
            data,
            first,
            members,
            by_number,
        }))
    }

    /// `object`, or the object it refers to (through references to
    /// references, up to 128): `None` when that is missing.
    pub fn resolve<'a>(&'a self, object: &'a Object) -> Option<&'a Object> {
        let mut object = object;
        for _ in 0..MAX_HOPS {
            match object {
                Object::Reference(id) => object = self.object(*id)?,
                other => return Some(other),
            }
        }
        None
    }

    /// The dictionary object `id` is (not a stream's).
    pub fn dictionary(&self, id: ObjectId) -> Option<&Dictionary> {
        self.object(id).and_then(Object::as_dict)
    }

    /// The value of `key` in `dict`, resolved.
    pub fn get<'a>(&'a self, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
        self.resolve(dict.get(key)?)
    }

    /// The catalog (`Root`).
    pub fn catalog(&self) -> Option<&Dictionary> {
        self.trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.dictionary(id))
    }

    /// The information dictionary (`Info`), if any.
    pub fn info(&self) -> Option<&Dictionary> {
        self.trailer
            .get(b"Info")
            .and_then(|info| self.resolve(info)?.as_dict())
    }

    /// The pages, in order: the leaves of the page tree. A kid whose object
    /// cannot be read counts as a (blank) page; one that is neither a page
    /// nor a page tree node is skipped. A kid with no `Type` is a node when
    /// it has `Kids`, and a page otherwise. The tree is walked at most once
    /// per object in the file, and 256 levels deep.
    pub fn pages(&self) -> Vec<ObjectId> {
        let mut pages = Vec::new();
        let Some(root) = self
            .catalog()
            .and_then(|catalog| catalog.get(b"Pages"))
            .and_then(Object::as_reference)
        else {
            return pages;
        };
        let mut steps = self.slots.len();
        let mut stack: Vec<&[Object]> = Vec::new();
        let mut kids: &[Object] = self.kids(root);
        loop {
            while let Some((kid, rest)) = kids.split_first() {
                if steps == 0 {
                    return pages;
                }
                steps -= 1;
                kids = rest;
                let Some(id) = kid.as_reference() else {
                    continue;
                };
                match self
                    .dictionary(id)
                    .map(|node| node_kind(node, &|value| self.resolve(value)))
                {
                    Some(NodeKind::Page) => pages.push(id),
                    Some(NodeKind::Tree) if stack.len() < MAX_TREE_DEPTH => {
                        if !kids.is_empty() {
                            stack.push(kids);
                        }
                        kids = self.kids(id);
                    }
                    Some(_) => {}
                    None => pages.push(id),
                }
            }
            match stack.pop() {
                Some(next) => kids = next,
                None => return pages,
            }
        }
    }

    fn kids(&self, node: ObjectId) -> &[Object] {
        self.dictionary(node)
            .and_then(|node| self.get(node, b"Kids"))
            .and_then(Object::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// The streams a page draws: its `Contents`, a stream or an array of
    /// them. What the array lists is charged as work: pages can share one.
    pub fn page_contents(&self, page: ObjectId) -> Vec<ObjectId> {
        let streams = self.contents_of(page);
        if let Err(error) = self
            .budget
            .work(streams.len().saturating_mul(CONTENTS_ENTRY_COST))
        {
            self.fail(error);
            return Vec::new();
        }
        streams
    }

    fn contents_of(&self, page: ObjectId) -> Vec<ObjectId> {
        let mut streams = Vec::new();
        let Some(mut contents) = self.dictionary(page).and_then(|page| page.get(b"Contents"))
        else {
            return streams;
        };
        let mut hops = 0;
        loop {
            match contents {
                Object::Reference(id) => match self.object(*id) {
                    None | Some(Object::Stream(_)) => streams.push(*id),
                    Some(other) => {
                        hops += 1;
                        if hops < MAX_HOPS {
                            contents = other;
                            continue;
                        }
                    }
                },
                Object::Array(items) => {
                    streams.extend(items.iter().filter_map(Object::as_reference))
                }
                _ => {}
            }
            return streams;
        }
    }

    /// A page's content: its streams decoded and joined, each followed by a
    /// new line, within `limit` bytes in all. A stream that does not decode
    /// is taken as it is stored, if that fits.
    pub fn page_content(&self, page: ObjectId, limit: usize) -> Result<Vec<u8>, ContentError> {
        let mut content = Vec::new();
        for id in self.page_contents(page) {
            let Some(stream) = self.object(id).and_then(Object::as_stream) else {
                continue;
            };
            let left = limit.saturating_sub(content.len());
            match self.decode(stream, left) {
                Ok(data) => content.extend_from_slice(&data),
                Err(ContentError::Over) => return Err(ContentError::Over),
                Err(_) if stream.content.len() <= left => {
                    content.extend_from_slice(&stream.content)
                }
                Err(_) => return Err(ContentError::Over),
            }
            content.push(b'\n');
        }
        Ok(content)
    }

    /// `stream`'s bytes decoded through its filters, held to `limit` for
    /// each filter, 8 filters at most. What each filter reads and writes is
    /// charged as work, whether the ones after it work or not: when the work
    /// runs out this is [`ContentError::Over`], and
    /// [`check`](Self::check) returns the error.
    pub fn decode(&self, stream: &Stream, limit: usize) -> Result<Vec<u8>, ContentError> {
        let mut spent = 0;
        let decoded = self.decode_counted(stream, limit, &mut spent);
        if let Err(error) = self.budget.work(spent) {
            self.fail(error);
            return Err(ContentError::Over);
        }
        decoded
    }

    /// [`decode`](Self::decode), adding what it spends to `spent` for the
    /// caller to charge. Values in the stream's dictionary may be references.
    pub(crate) fn decode_counted<'a>(
        &'a self,
        stream: &'a Stream,
        limit: usize,
        spent: &mut usize,
    ) -> Result<Vec<u8>, ContentError> {
        filters::decode(
            &stream.dict,
            &stream.content,
            limit,
            &|value| self.resolve(value),
            spent,
        )
        .map_err(|error| match error {
            DecodeError::Over => ContentError::Over,
            DecodeError::Unsupported => ContentError::Unsupported,
            DecodeError::Invalid(why) => ContentError::Damaged(why),
        })
    }

    /// What has been counted of the memory limit so far.
    pub fn memory_used(&self) -> usize {
        self.budget.memory_used()
    }

    /// What has been counted of the work limit so far.
    pub fn work_used(&self) -> usize {
        self.budget.work_used()
    }

    /// The limits the document is read within.
    pub fn limits(&self) -> ReadLimits {
        self.budget.limits
    }
}

/// What a kid of the page tree is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    Page,
    Tree,
    Neither,
}

/// What the dictionary `node` is in a page tree: by its `Type`, or, with none,
/// by whether it has `Kids`. A linearized file's first object is not a page.
pub(crate) fn node_kind<'a>(
    node: &'a Dictionary,
    resolve: &dyn Fn(&'a Object) -> Option<&'a Object>,
) -> NodeKind {
    let kind = node
        .get(b"Type")
        .and_then(resolve)
        .and_then(Object::as_name);
    match kind {
        Some(b"Page") => NodeKind::Page,
        Some(b"Pages") => NodeKind::Tree,
        Some(_) => NodeKind::Neither,
        None if node.has(b"Linearized") => NodeKind::Neither,
        None if node.has(b"Kids") => NodeKind::Tree,
        None => NodeKind::Page,
    }
}

fn entry_offset(entry: Entry) -> usize {
    match entry {
        Entry::Normal { offset, .. } => offset,
        _ => usize::MAX,
    }
}

/// Why a stream's data could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentError {
    /// It decodes to more than the limit.
    Over,
    /// One of its filters is an image codec this reader leaves encoded, or
    /// one it does not know.
    Unsupported,
    /// The data is damaged.
    Damaged(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary;
    use crate::write::Objects;

    /// A document of `pages` pages, each showing its number.
    fn sample(pages: usize) -> Objects {
        let mut objects = Objects::new();
        let tree = objects.reserve();
        let kids: Vec<Object> = (0..pages)
            .map(|i| {
                let content = objects.add(Stream::new(
                    Dictionary::new(),
                    format!("BT /F1 12 Tf 50 700 Td (Page {i}) Tj ET").into_bytes(),
                ));
                objects
                    .add(dictionary! { "Type" => "Page", "Parent" => tree, "Contents" => content })
                    .into()
            })
            .collect();
        objects.insert(
            tree,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => pages as i64 },
        );
        let catalog = objects.add(dictionary! { "Type" => "Catalog", "Pages" => tree });
        objects.trailer.set("Root", catalog);
        objects
    }

    #[test]
    fn objects_are_read_when_asked_for() {
        let bytes = sample(3).save().unwrap();
        let document = Document::load(bytes, ReadLimits::default()).unwrap();
        let before = document.memory_used();
        let pages = document.pages();
        assert_eq!(pages.len(), 3);
        assert!(document.memory_used() > before);
        let content = document.page_content(pages[2], 1024).unwrap();
        assert_eq!(content, b"BT /F1 12 Tf 50 700 Td (Page 2) Tj ET\n");
        assert_eq!(document.page_content(pages[2], 10), Err(ContentError::Over));
        document.check().unwrap();
    }

    #[test]
    fn object_streams_are_read_lazily() {
        let bytes = sample(5).save_with(true).unwrap();
        let document = Document::load(bytes, ReadLimits::default()).unwrap();
        assert_eq!(document.pages().len(), 5);
        assert_eq!(
            document.catalog().and_then(|c| c.type_name()),
            Some(&b"Catalog"[..])
        );
    }

    #[test]
    fn not_a_pdf_is_damaged() {
        assert!(matches!(
            Document::load(&b"not a pdf"[..], ReadLimits::default()),
            Err(PdfError::Damaged(_))
        ));
        assert!(matches!(
            Document::load(&b"%PDF-1.7 broken"[..], ReadLimits::default()),
            Err(PdfError::Damaged(_))
        ));
    }

    #[test]
    fn a_page_tree_that_loops_ends() {
        let mut objects = Objects::new();
        let tree = objects.reserve();
        let page = objects.add(dictionary! { "Type" => "Page", "Parent" => tree });
        // The tree lists itself.
        objects.insert(tree, dictionary! { "Type" => "Pages", "Kids" => vec![page.into(), tree.into()], "Count" => 2 });
        let catalog = objects.add(dictionary! { "Type" => "Catalog", "Pages" => tree });
        objects.trailer.set("Root", catalog);
        let document = Document::load(objects.save().unwrap(), ReadLimits::default()).unwrap();
        let pages = document.pages();
        assert!(
            !pages.is_empty() && pages.len() <= document.len(),
            "{pages:?}"
        );
    }

    #[test]
    fn object_zero_is_never_an_object() {
        // A damaged file whose page tree is written as object 0, the head
        // of the free list: rebuilt, it has no pages, and so does what is
        // saved from it.
        let file = b"%PDF-1.7\n\
            0 0 obj\n<< /Type /Pages /Kids [5 0 R] /Count 1 >>\nendobj\n\
            5 0 obj\n<< /Type /Page /Parent 0 0 R >>\nendobj\n\
            8 0 obj\n<< /Type /Catalog /Pages 0 0 R >>\nendobj\n\
            trailer\n<< /Root 8 0 R /Size 9 >>\n%%EOF\n";
        let document = Document::load(&file[..], ReadLimits::default()).unwrap();
        assert!(document.rebuilt());
        assert!(document.object((0, 0)).is_none());
        assert!(document.pages().is_empty());
        let objects = Objects::read(document).unwrap();
        assert!(objects.pages().is_empty());
        let saved = Document::load(objects.save().unwrap(), ReadLimits::default()).unwrap();
        assert!(saved.pages().is_empty());
    }

    #[test]
    fn a_reference_cycle_resolves_to_nothing() {
        let mut objects = sample(1);
        let a = objects.reserve();
        let b = objects.add(Object::Reference(a));
        objects.insert(a, Object::Reference(b));
        let document = Document::load(objects.save().unwrap(), ReadLimits::default()).unwrap();
        assert_eq!(document.resolve(&Object::Reference(a)), None);
    }
}
