//! Span-based AST rewriter: parses Rust source with `syn`, records byte-range
//! edits over the original text so every comment, attribute, doc block and
//! blank line outside the touched tokens stays byte-for-byte untouched.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;

use anyhow::Context;
use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::spanned::Spanned;

use super::rules::{
    self, CallKind, DATETIME_EXPORTS, DROPPED_CHRONO_IMPORTS, ORM_NEW_TRAIT, ORM_OLD_TRAIT,
    TargetCrate, notes,
};

/// A `// navree upgrade: <message>` note inserted above an ambiguous line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeNote {
    /// 1-based line number in the original source where the construct appeared.
    pub line: usize,
    /// Explanation placed after `// navree upgrade: `.
    pub message: String,
}

/// Result of running the rewriter on a single Rust source string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteOutcome {
    /// Rewritten Rust source (equal to input when `rewrites == 0` and `notes.is_empty()`).
    pub source: String,
    /// Number of AST rewrites applied (types, paths, calls, methods, imports, traits).
    pub rewrites: usize,
    /// New `// navree upgrade:` notes added during this run.
    pub notes: Vec<UpgradeNote>,
    /// Whether any `chrono` import or `chrono::` path remains in the rewritten source.
    pub still_uses_chrono: bool,
    /// Whether the rewritten source names `navree_datetime` or `navree::datetime`.
    pub uses_datetime: bool,
}

/// Additional context about the package a `.rs` file belongs to.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileContext {
    /// Target module path (`navree::datetime` or `navree_datetime`).
    pub target: TargetCrateOrDefault,
    /// Whether this file belongs to the `navree-orm` crate itself.
    pub is_navree_orm: bool,
}

/// Wrapper providing `Default` -> `TargetCrate::Facade`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetCrateOrDefault(pub TargetCrate);

impl Default for TargetCrateOrDefault {
    fn default() -> Self {
        Self(TargetCrate::Facade)
    }
}

/// Rewrites `source` using `target` (`navree::datetime` or `navree_datetime`).
pub fn rewrite_source(source: &str, target: TargetCrate) -> anyhow::Result<RewriteOutcome> {
    rewrite_source_with(
        source,
        FileContext {
            target: TargetCrateOrDefault(target),
            is_navree_orm: false,
        },
    )
}

/// Rewrites `source` with package context.
pub fn rewrite_source_with(source: &str, ctx: FileContext) -> anyhow::Result<RewriteOutcome> {
    let file = syn::parse_file(source).context("cannot parse Rust source")?;
    let mut engine = Engine::new(source, ctx.target.0, ctx.is_navree_orm);
    engine.process_scope_items(&file.items, None);

    let new_source = engine.edits.apply(source);
    if new_source == source {
        let (still_uses_chrono, uses_datetime) = inspect_crate_refs(&file);
        return Ok(RewriteOutcome {
            source: source.to_owned(),
            rewrites: 0,
            notes: Vec::new(),
            still_uses_chrono,
            uses_datetime,
        });
    }

    let (still_uses_chrono, uses_datetime) = match syn::parse_file(&new_source) {
        Ok(parsed) => inspect_crate_refs(&parsed),
        Err(_) => (new_source.contains("chrono"), true),
    };

    Ok(RewriteOutcome {
        source: new_source,
        rewrites: engine.rewrite_count.max(1),
        notes: engine.added_notes,
        still_uses_chrono,
        uses_datetime,
    })
}

/// Checks whether a parsed `syn::File` references `chrono` or `navree_datetime` / `navree::datetime`
/// in actual AST paths or `use` trees (ignoring comments and string literals).
pub(crate) fn inspect_crate_refs(file: &syn::File) -> (bool, bool) {
    struct RefChecker {
        chrono: bool,
        datetime: bool,
    }

    impl<'ast> syn::visit::Visit<'ast> for RefChecker {
        fn visit_use_tree(&mut self, node: &'ast syn::UseTree) {
            match node {
                syn::UseTree::Path(p) => {
                    if p.ident == "chrono" {
                        self.chrono = true;
                    } else if p.ident == "navree_datetime"
                        || (p.ident == "navree"
                            && matches!(
                                &*p.tree,
                                syn::UseTree::Path(inner) if inner.ident == "datetime"
                            ))
                        || (p.ident == "navree"
                            && matches!(
                                &*p.tree,
                                syn::UseTree::Name(inner) if inner.ident == "datetime"
                            ))
                    {
                        self.datetime = true;
                    }
                }
                syn::UseTree::Name(n) if n.ident == "chrono" => self.chrono = true,
                syn::UseTree::Name(n) if n.ident == "navree_datetime" => self.datetime = true,
                _ => {}
            }
            syn::visit::visit_use_tree(self, node);
        }

        fn visit_path(&mut self, node: &'ast syn::Path) {
            let segs: Vec<_> = node.segments.iter().map(|s| s.ident.to_string()).collect();
            if segs.first().is_some_and(|s| s == "chrono") {
                self.chrono = true;
            }
            if segs.first().is_some_and(|s| s == "navree_datetime") {
                self.datetime = true;
            }
            if segs.len() >= 2 && segs[0] == "navree" && segs[1] == "datetime" {
                self.datetime = true;
            }
            syn::visit::visit_path(self, node);
        }
    }

    let mut checker = RefChecker {
        chrono: false,
        datetime: false,
    };
    syn::visit::Visit::visit_file(&mut checker, file);
    (checker.chrono, checker.datetime)
}

#[derive(Clone, Debug)]
struct TextEdit {
    range: Range<usize>,
    replacement: String,
    is_note: bool,
}

#[derive(Default)]
struct EditSet {
    edits: Vec<TextEdit>,
}

impl EditSet {
    fn replace(&mut self, range: Range<usize>, replacement: String) {
        self.edits.retain(|existing| {
            existing.is_note
                || !(existing.range.start >= range.start && existing.range.end <= range.end)
        });
        // Skip if an enclosing replacement already covers this range.
        if self.edits.iter().any(|existing| {
            !existing.is_note
                && existing.range.start <= range.start
                && existing.range.end >= range.end
        }) {
            return;
        }
        self.edits.push(TextEdit {
            range,
            replacement,
            is_note: false,
        });
    }

    fn insert_note(&mut self, offset: usize, text: String) -> bool {
        if self
            .edits
            .iter()
            .any(|e| e.is_note && e.range.start == offset && e.replacement == text)
        {
            return false;
        }
        self.edits.push(TextEdit {
            range: offset..offset,
            replacement: text,
            is_note: true,
        });
        true
    }

    fn render_slice(&self, source: &str, range: Range<usize>) -> String {
        let mut inner: Vec<&TextEdit> = self
            .edits
            .iter()
            .filter(|e| !e.is_note && e.range.start >= range.start && e.range.end <= range.end)
            .collect();
        if inner.is_empty() {
            return source[range].to_owned();
        }
        inner.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(a.range.end.cmp(&b.range.end))
        });
        let mut out = source[range.clone()].to_owned();
        for edit in inner.into_iter().rev() {
            let rel = (edit.range.start - range.start)..(edit.range.end - range.start);
            out.replace_range(rel, &edit.replacement);
        }
        out
    }

    fn apply(&self, source: &str) -> String {
        if self.edits.is_empty() {
            return source.to_owned();
        }
        let mut sorted: Vec<&TextEdit> = self.edits.iter().collect();
        // Sort by start byte; at the exact same start byte, zero-length notes come BEFORE
        // replacements that begin at that byte, so reverse application splices replacement first
        // and prepends the note right before it.
        sorted.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(b.is_note.cmp(&a.is_note))
                .then(a.range.end.cmp(&b.range.end))
        });

        // Deduplicate or drop any accidental overlap conservatively.
        let mut clean: Vec<&TextEdit> = Vec::with_capacity(sorted.len());
        let mut cursor = 0usize;
        for edit in sorted {
            if edit.range.start < cursor {
                continue;
            }
            cursor = edit.range.end;
            clean.push(edit);
        }

        let mut out = source.to_owned();
        for edit in clean.into_iter().rev() {
            out.replace_range(edit.range.clone(), &edit.replacement);
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownKind {
    Timestamp,
    Zoned,
    Date,
    Time,
    DateTime,
    Span,
    Offset,
}

#[derive(Clone, Debug)]
struct ChronoImportBinding {
    chrono_name: String,
    alias: Option<String>,
    is_pub: bool,
}

#[derive(Clone, Debug)]
struct UseItemRecord {
    /// Range from start of `vis` (or `use`) to `;` inclusive.
    code_range: Range<usize>,
    /// Range removing the entire line when the `use` has no attributes and becomes empty.
    line_remove_range: Range<usize>,
    has_attrs: bool,
    vis_prefix: String,
    indent: String,
}

#[derive(Clone, Debug, Default)]
struct ScopeState {
    has_chrono_glob: bool,
    /// Local identifier -> chrono binding info.
    chrono_bindings: BTreeMap<String, ChronoImportBinding>,
    /// Trait-only imports (`use chrono::Timelike as _`).
    chrono_trait_underscores: BTreeSet<String>,
    /// Every `use chrono::...;` item in this scope.
    chrono_uses: Vec<UseItemRecord>,
    /// Existing private `use <root>::...;` item in this scope, if any.
    existing_target_use: Option<(UseItemRecord, BTreeSet<String>)>,
    /// Symbols imported from foreign crates or declared locally that collide with `DATETIME_EXPORTS`.
    foreign_bindings: BTreeSet<String>,
    /// Whether `Timestamp` in this scope refers to `navree_orm::Timestamp` trait.
    orm_timestamp_in_scope: bool,
    /// Compile-time `const NAME: &str = "..."` names in scope.
    const_strings: BTreeSet<String>,
    /// Generic type parameter names currently in scope (`impl<Tz: TimeZone>`).
    type_params: BTreeSet<String>,
    /// Local/parameter type hints inside the current function.
    var_kinds: HashMap<String, KnownKind>,
    /// `<root>` symbols introduced or used unqualified in this scope (`Timestamp`, `Date`, ...).
    needed_target_imports: BTreeSet<String>,
    /// Aliased `<root>` symbols (`Span as ChronoDuration`) to preserve in `use <root>::...;`.
    needed_aliased_imports: BTreeSet<(String, String)>,
    /// Unrewritten `chrono` identifiers still referenced verbatim in this scope (`LocalResult`).
    kept_chrono_imports: BTreeSet<String>,
    /// Whether this scope has `use super::*;`.
    inherits_super: bool,
    /// Whether any `chrono` import or inline `chrono::` path exists in this scope or parents.
    chrono_active: bool,
}

impl ScopeState {
    fn has_use_slot(&self, parent: Option<&ScopeState>) -> bool {
        !self.chrono_uses.is_empty()
            || self.existing_target_use.is_some()
            || (self.inherits_super && parent.is_some_and(|p| p.has_use_slot(None)))
    }

    fn resolve_chrono_ident(&self, ident: &str) -> Option<&str> {
        if let Some(binding) = self.chrono_bindings.get(ident) {
            return Some(binding.chrono_name.as_str());
        }
        if self.has_chrono_glob {
            return match ident {
                "DateTime" => Some("DateTime"),
                "Utc" => Some("Utc"),
                "NaiveDate" => Some("NaiveDate"),
                "NaiveTime" => Some("NaiveTime"),
                "NaiveDateTime" => Some("NaiveDateTime"),
                "FixedOffset" => Some("FixedOffset"),
                "TimeDelta" => Some("TimeDelta"),
                "Duration" => Some("Duration"),
                "Weekday" => Some("Weekday"),
                "Month" => Some("Month"),
                "SecondsFormat" => Some("SecondsFormat"),
                "LocalResult" => Some("LocalResult"),
                "Days" => Some("Days"),
                "Months" => Some("Months"),
                _ => None,
            };
        }
        None
    }

    fn is_foreign_collision(&self, target_ident: &str) -> bool {
        self.foreign_bindings.contains(target_ident)
    }
}

struct Engine<'a> {
    source: &'a str,
    nl: &'static str,
    target: TargetCrate,
    is_navree_orm: bool,
    edits: EditSet,
    rewrite_count: usize,
    added_notes: Vec<UpgradeNote>,
    noted_lines: BTreeSet<(usize, &'static str)>,
}

impl<'a> Engine<'a> {
    fn new(source: &'a str, target: TargetCrate, is_navree_orm: bool) -> Self {
        let nl = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        Self {
            source,
            nl,
            target,
            is_navree_orm,
            edits: EditSet::default(),
            rewrite_count: 0,
            added_notes: Vec::new(),
            noted_lines: BTreeSet::new(),
        }
    }

    fn root_path(&self) -> &'static str {
        self.target.root_path()
    }

    /// Formats a `navree-datetime` item (`Timestamp`, `Date`, ...) either unqualified
    /// (recording an import in `scope`) or inline-qualified (`<root>::Timestamp`) when the
    /// original code wrote `chrono::...`, had no `use` slot, or has a foreign collision.
    fn emit_target_symbol(
        &self,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        target_name: &str,
        prefer_inline: bool,
        alias_used: Option<&str>,
    ) -> String {
        if let Some(alias) = alias_used {
            scope
                .needed_aliased_imports
                .insert((target_name.to_owned(), alias.to_owned()));
            return alias.to_owned();
        }
        let already_in_existing = scope
            .existing_target_use
            .as_ref()
            .is_some_and(|(_, set)| set.contains(target_name));
        if !prefer_inline
            && !scope.is_foreign_collision(target_name)
            && (scope.has_use_slot(parent) || already_in_existing)
        {
            scope.needed_target_imports.insert(target_name.to_owned());
            target_name.to_owned()
        } else {
            format!("{}::{target_name}", self.root_path())
        }
    }

    fn emit_note_at(&mut self, byte_pos: usize, message: &'static str) {
        let (line_start, indent) = line_start_and_indent(self.source, byte_pos);
        if !self.noted_lines.insert((line_start, message)) {
            return;
        }
        if has_note_above(self.source, line_start, message) {
            return;
        }
        let text = format!("{indent}// navree upgrade: {message}{}", self.nl);
        if self.edits.insert_note(line_start, text) {
            let line = 1 + self.source[..line_start]
                .bytes()
                .filter(|&b| b == b'\n')
                .count();
            self.added_notes.push(UpgradeNote {
                line,
                message: message.to_owned(),
            });
        }
    }

    fn process_scope_items(
        &mut self,
        items: &[syn::Item],
        parent: Option<&ScopeState>,
    ) -> ScopeState {
        let mut scope = self.collect_scope(items, parent);

        for item in items {
            self.visit_item(item, &mut scope, parent);
        }

        self.finalize_scope_uses(&mut scope);
        scope
    }

    fn collect_scope(&mut self, items: &[syn::Item], parent: Option<&ScopeState>) -> ScopeState {
        let mut scope = ScopeState::default();

        for item in items {
            if let syn::Item::Use(u) = item
                && is_use_super_glob(&u.tree)
            {
                scope.inherits_super = true;
            }
        }

        if scope.inherits_super
            && let Some(p) = parent
        {
            scope.has_chrono_glob = p.has_chrono_glob;
            scope.chrono_bindings = p.chrono_bindings.clone();
            scope.orm_timestamp_in_scope = p.orm_timestamp_in_scope;
            scope.const_strings = p.const_strings.clone();
            scope.foreign_bindings = p.foreign_bindings.clone();
            scope.chrono_active = p.chrono_active;
        }

        for item in items {
            match item {
                syn::Item::Use(item_use) => {
                    self.analyze_item_use(item_use, &mut scope);
                }
                syn::Item::Const(item_const) => {
                    if is_str_const(item_const) {
                        scope.const_strings.insert(item_const.ident.to_string());
                    }
                    let name = item_const.ident.to_string();
                    if DATETIME_EXPORTS.contains(&name.as_str()) {
                        scope.foreign_bindings.insert(name);
                    }
                }
                syn::Item::Struct(s) => {
                    let name = s.ident.to_string();
                    if DATETIME_EXPORTS.contains(&name.as_str()) {
                        scope.foreign_bindings.insert(name);
                    }
                }
                syn::Item::Enum(e) => {
                    let name = e.ident.to_string();
                    if DATETIME_EXPORTS.contains(&name.as_str()) {
                        scope.foreign_bindings.insert(name);
                    }
                }
                syn::Item::Type(t) => {
                    let name = t.ident.to_string();
                    if DATETIME_EXPORTS.contains(&name.as_str()) {
                        scope.foreign_bindings.insert(name);
                    }
                }
                syn::Item::Trait(tr) => {
                    let name = tr.ident.to_string();
                    if name == ORM_OLD_TRAIT && self.is_navree_orm {
                        scope.orm_timestamp_in_scope = true;
                    } else if DATETIME_EXPORTS.contains(&name.as_str()) {
                        scope.foreign_bindings.insert(name);
                    }
                }
                _ => {}
            }
        }

        if !scope.chrono_bindings.is_empty()
            || !scope.chrono_trait_underscores.is_empty()
            || scope.has_chrono_glob
            || items_contain_inline_chrono(items)
        {
            scope.chrono_active = true;
        }

        scope
    }

    fn analyze_item_use(&mut self, item_use: &syn::ItemUse, scope: &mut ScopeState) {
        let is_pub = !matches!(item_use.vis, syn::Visibility::Inherited);
        let Some(full_range) = token_range(item_use) else {
            return;
        };
        let code_start = if matches!(item_use.vis, syn::Visibility::Inherited) {
            item_use.use_token.span().byte_range().start
        } else {
            token_range(&item_use.vis).map_or(full_range.start, |r| r.start)
        };
        let code_range = code_start..full_range.end;
        let (line_start, indent) = line_start_and_indent(self.source, full_range.start);
        let line_remove_end = skip_single_newline(self.source, full_range.end);
        let vis_prefix = if matches!(item_use.vis, syn::Visibility::Inherited) {
            String::new()
        } else if let Some(vr) = token_range(&item_use.vis) {
            format!("{} ", &self.source[vr])
        } else {
            "pub ".to_owned()
        };
        let record = UseItemRecord {
            code_range,
            line_remove_range: line_start..line_remove_end,
            has_attrs: !item_use.attrs.is_empty(),
            vis_prefix,
            indent: indent.to_owned(),
        };

        let tree = strip_leading_colon_tree(&item_use.tree);

        // 1. `use chrono::...;`
        if let Some(sub) = match_root_use_tree(tree, &["chrono"]) {
            collect_chrono_tree(sub, is_pub, scope);
            scope.chrono_uses.push(record);
            return;
        }

        // 2. Existing `use navree::datetime::...;` or `use navree_datetime::...;`
        let target_segs: &[&str] = match self.target {
            TargetCrate::Facade => &["navree", "datetime"],
            TargetCrate::Direct => &["navree_datetime"],
        };
        if !is_pub
            && item_use.attrs.is_empty()
            && let Some(sub) = match_root_use_tree(tree, target_segs)
        {
            let mut names = BTreeSet::new();
            if collect_plain_use_names(sub, &mut names) && scope.existing_target_use.is_none() {
                scope.existing_target_use = Some((record, names));
                return;
            }
        }

        // 3. `use navree::orm::{..., Timestamp, ...}` or `use navree_orm::{..., Timestamp, ...}`
        if self.rewrite_orm_timestamp_in_use_tree(&item_use.tree, scope) {
            return;
        }

        // 4. Record foreign bindings that share a name with `DATETIME_EXPORTS`.
        collect_foreign_use_bindings(tree, &mut scope.foreign_bindings);
    }

    fn rewrite_orm_timestamp_in_use_tree(
        &mut self,
        tree: &syn::UseTree,
        scope: &mut ScopeState,
    ) -> bool {
        let mut prefix = Vec::new();
        self.walk_orm_use_tree(tree, &mut prefix, scope)
    }

    fn walk_orm_use_tree(
        &mut self,
        tree: &syn::UseTree,
        prefix: &mut Vec<String>,
        scope: &mut ScopeState,
    ) -> bool {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                let hit = self.walk_orm_use_tree(&p.tree, prefix, scope);
                prefix.pop();
                hit
            }
            syn::UseTree::Group(g) => {
                let mut any = false;
                for item in &g.items {
                    if self.walk_orm_use_tree(item, prefix, scope) {
                        any = true;
                    }
                }
                any
            }
            syn::UseTree::Name(n) if n.ident == ORM_OLD_TRAIT => {
                if is_orm_module_prefix(prefix, self.is_navree_orm) {
                    let r = n.ident.span().byte_range();
                    self.edits.replace(r, ORM_NEW_TRAIT.to_owned());
                    self.rewrite_count += 1;
                    scope.orm_timestamp_in_scope = true;
                    return true;
                }
                false
            }
            syn::UseTree::Rename(r) if r.ident == ORM_OLD_TRAIT => {
                if is_orm_module_prefix(prefix, self.is_navree_orm) {
                    let range = r.ident.span().byte_range();
                    self.edits.replace(range, ORM_NEW_TRAIT.to_owned());
                    self.rewrite_count += 1;
                    if r.rename == ORM_OLD_TRAIT {
                        let ren_range = r.rename.span().byte_range();
                        self.edits.replace(ren_range, ORM_NEW_TRAIT.to_owned());
                        scope.orm_timestamp_in_scope = true;
                    }
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn finalize_scope_uses(&mut self, scope: &mut ScopeState) {
        // Public re-exports (`pub use chrono::...`) preserve direct-mapped items.
        for binding in scope.chrono_bindings.values() {
            if binding.is_pub
                && let Some(rule) = rules::find_type_rule(&binding.chrono_name)
            {
                if let Some(alias) = &binding.alias {
                    scope
                        .needed_aliased_imports
                        .insert((rule.target_name.to_owned(), alias.clone()));
                } else {
                    scope
                        .needed_target_imports
                        .insert(rule.target_name.to_owned());
                }
            }
        }

        if scope.chrono_uses.is_empty() {
            // Merge any newly needed imports into an existing `use <root>::...;` if present.
            if (!scope.needed_target_imports.is_empty() || !scope.needed_aliased_imports.is_empty())
                && let Some((record, existing)) = &scope.existing_target_use
            {
                let mut merged: BTreeSet<String> = existing.clone();
                merged.extend(scope.needed_target_imports.iter().cloned());
                for (target, alias) in &scope.needed_aliased_imports {
                    merged.insert(format!("{target} as {alias}"));
                }
                if merged != *existing {
                    let formatted = format_use_statement(
                        &record.indent,
                        &record.vis_prefix,
                        self.root_path(),
                        &merged,
                        self.nl,
                    );
                    self.edits.replace(record.code_range.clone(), formatted);
                    self.rewrite_count += 1;
                }
            }
            return;
        }

        let mut target_specs: BTreeSet<String> = scope.needed_target_imports.clone();
        for (target, alias) in &scope.needed_aliased_imports {
            target_specs.insert(format!("{target} as {alias}"));
        }

        let kept_chrono: BTreeSet<String> = scope
            .kept_chrono_imports
            .iter()
            .filter(|name| !DROPPED_CHRONO_IMPORTS.contains(&name.as_str()) || *name == "TimeZone")
            .cloned()
            .collect();

        // Fixed-point guard: when every existing `chrono` binding in this scope is still kept,
        // no alias changed, and every needed target import is already in `existing_target_use`,
        // leave the scope's `use` items untouched.
        let orig_chrono_set: BTreeSet<String> = scope
            .chrono_bindings
            .iter()
            .filter(|(_, b)| b.alias.is_none())
            .map(|(k, _)| k.clone())
            .collect();
        let all_target_already_imported = match &scope.existing_target_use {
            Some((_, existing)) => target_specs.is_subset(existing),
            None => target_specs.is_empty(),
        };
        if scope.chrono_uses.len() == 1
            && scope.chrono_trait_underscores.is_empty()
            && !scope.has_chrono_glob
            && scope.chrono_bindings.values().all(|b| b.alias.is_none())
            && orig_chrono_set == kept_chrono
            && all_target_already_imported
        {
            return;
        }

        // Build the combined `<root>` import list (including existing `<root>` use if any).
        if let Some((existing_rec, existing_names)) = &scope.existing_target_use {
            target_specs.extend(existing_names.iter().cloned());
            if !existing_rec.has_attrs {
                self.edits
                    .replace(existing_rec.line_remove_range.clone(), String::new());
            } else {
                self.edits
                    .replace(existing_rec.code_range.clone(), String::new());
            }
        }

        let first = &scope.chrono_uses[0];
        let mut statements = Vec::new();
        if !kept_chrono.is_empty() {
            statements.push(format_use_statement(
                &first.indent,
                &first.vis_prefix,
                "chrono",
                &kept_chrono,
                self.nl,
            ));
        }
        if !target_specs.is_empty() {
            statements.push(format_use_statement(
                &first.indent,
                &first.vis_prefix,
                self.root_path(),
                &target_specs,
                self.nl,
            ));
        }

        if statements.is_empty() {
            if !first.has_attrs {
                self.edits
                    .replace(first.line_remove_range.clone(), String::new());
            } else {
                self.edits.replace(first.code_range.clone(), String::new());
            }
        } else {
            let joined = statements.join(&format!("{}{}", self.nl, first.indent));
            self.edits.replace(first.code_range.clone(), joined);
        }
        self.rewrite_count += 1;

        for extra in scope.chrono_uses.iter().skip(1) {
            if !extra.has_attrs {
                self.edits
                    .replace(extra.line_remove_range.clone(), String::new());
            } else {
                self.edits.replace(extra.code_range.clone(), String::new());
            }
        }
    }

    fn visit_item(
        &mut self,
        item: &syn::Item,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
    ) {
        match item {
            syn::Item::Mod(item_mod) => {
                if let Some((_, inner_items)) = &item_mod.content {
                    let child = self.process_scope_items(inner_items, Some(scope));
                    // If child had `use super::*;` and no `use` slot of its own, bubble needed
                    // imports up to `scope`.
                    if child.inherits_super
                        && child.chrono_uses.is_empty()
                        && child.existing_target_use.is_none()
                    {
                        scope
                            .needed_target_imports
                            .extend(child.needed_target_imports);
                        scope
                            .needed_aliased_imports
                            .extend(child.needed_aliased_imports);
                        scope.kept_chrono_imports.extend(child.kept_chrono_imports);
                    }
                }
            }
            syn::Item::Struct(s) => {
                let saved_tp = self.push_generics(&s.generics, scope, token_start(s));
                for field in &s.fields {
                    self.visit_type(&field.ty, scope, parent);
                }
                scope.type_params = saved_tp;
            }
            syn::Item::Enum(e) => {
                let saved_tp = self.push_generics(&e.generics, scope, token_start(e));
                for variant in &e.variants {
                    for field in &variant.fields {
                        self.visit_type(&field.ty, scope, parent);
                    }
                    if let Some((_, disc)) = &variant.discriminant {
                        self.visit_expr(disc, scope, parent);
                    }
                }
                scope.type_params = saved_tp;
            }
            syn::Item::Type(t) => {
                let saved_tp = self.push_generics(&t.generics, scope, token_start(t));
                self.visit_type(&t.ty, scope, parent);
                scope.type_params = saved_tp;
            }
            syn::Item::Const(c) => {
                self.visit_type(&c.ty, scope, parent);
                self.visit_expr(&c.expr, scope, parent);
            }
            syn::Item::Static(s) => {
                self.visit_type(&s.ty, scope, parent);
                self.visit_expr(&s.expr, scope, parent);
            }
            syn::Item::Trait(tr) => {
                if tr.ident == ORM_OLD_TRAIT && self.is_navree_orm {
                    self.edits
                        .replace(tr.ident.span().byte_range(), ORM_NEW_TRAIT.to_owned());
                    self.rewrite_count += 1;
                }
                let saved_tp = self.push_generics(&tr.generics, scope, token_start(tr));
                for bound in &tr.supertraits {
                    self.visit_type_param_bound(bound, scope, parent, token_start(tr));
                }
                for ti in &tr.items {
                    match ti {
                        syn::TraitItem::Fn(f) => {
                            let saved_fn_tp =
                                self.push_generics(&f.sig.generics, scope, token_start(f));
                            let saved_vars = scope.var_kinds.clone();
                            self.visit_signature(&f.sig, scope, parent);
                            if let Some(block) = &f.default {
                                self.visit_block(block, scope, parent);
                            }
                            scope.var_kinds = saved_vars;
                            scope.type_params = saved_fn_tp;
                        }
                        syn::TraitItem::Const(c) => {
                            self.visit_type(&c.ty, scope, parent);
                            if let Some((_, expr)) = &c.default {
                                self.visit_expr(expr, scope, parent);
                            }
                        }
                        syn::TraitItem::Type(t) => {
                            for b in &t.bounds {
                                self.visit_type_param_bound(b, scope, parent, token_start(t));
                            }
                            if let Some((_, ty)) = &t.default {
                                self.visit_type(ty, scope, parent);
                            }
                        }
                        _ => {}
                    }
                }
                scope.type_params = saved_tp;
            }
            syn::Item::Impl(imp) => {
                let saved_tp = self.push_generics(&imp.generics, scope, token_start(imp));
                if let Some((trait_path, _)) = &imp.trait_ {
                    self.visit_trait_path(trait_path, scope, token_start(imp));
                }
                self.visit_type(&imp.self_ty, scope, parent);
                for ii in &imp.items {
                    match ii {
                        syn::ImplItem::Fn(f) => {
                            let saved_fn_tp =
                                self.push_generics(&f.sig.generics, scope, token_start(f));
                            let saved_vars = scope.var_kinds.clone();
                            self.visit_signature(&f.sig, scope, parent);
                            self.visit_block(&f.block, scope, parent);
                            scope.var_kinds = saved_vars;
                            scope.type_params = saved_fn_tp;
                        }
                        syn::ImplItem::Const(c) => {
                            self.visit_type(&c.ty, scope, parent);
                            self.visit_expr(&c.expr, scope, parent);
                        }
                        syn::ImplItem::Type(t) => {
                            self.visit_type(&t.ty, scope, parent);
                        }
                        _ => {}
                    }
                }
                scope.type_params = saved_tp;
            }
            syn::Item::Fn(f) => {
                let saved_tp = self.push_generics(&f.sig.generics, scope, token_start(f));
                let saved_vars = scope.var_kinds.clone();
                self.visit_signature(&f.sig, scope, parent);
                self.visit_block(&f.block, scope, parent);
                scope.var_kinds = saved_vars;
                scope.type_params = saved_tp;
            }
            syn::Item::Macro(m) => {
                self.visit_macro_tokens(&m.mac, scope, parent, token_start(m));
            }
            _ => {}
        }
    }

    fn push_generics(
        &mut self,
        generics: &syn::Generics,
        scope: &mut ScopeState,
        item_pos: usize,
    ) -> BTreeSet<String> {
        let saved = scope.type_params.clone();
        for param in &generics.params {
            if let syn::GenericParam::Type(tp) = param {
                scope.type_params.insert(tp.ident.to_string());
                for b in &tp.bounds {
                    self.visit_type_param_bound(b, scope, None, item_pos);
                }
                if let Some((_, eq_ty)) = &tp.default {
                    self.visit_type(eq_ty, scope, None);
                }
            }
        }
        if let Some(where_clause) = &generics.where_clause {
            for pred in &where_clause.predicates {
                if let syn::WherePredicate::Type(pt) = pred {
                    self.visit_type(&pt.bounded_ty, scope, None);
                    for b in &pt.bounds {
                        self.visit_type_param_bound(b, scope, None, item_pos);
                    }
                }
            }
        }
        saved
    }

    fn visit_type_param_bound(
        &mut self,
        bound: &syn::TypeParamBound,
        scope: &mut ScopeState,
        _parent: Option<&ScopeState>,
        note_pos: usize,
    ) {
        if let syn::TypeParamBound::Trait(tb) = bound {
            self.visit_trait_path(&tb.path, scope, note_pos);
        }
    }

    fn visit_trait_path(&mut self, path: &syn::Path, scope: &mut ScopeState, note_pos: usize) {
        if let Some(range) = orm_timestamp_path_ident_range(path, scope, self.is_navree_orm) {
            self.edits.replace(range, ORM_NEW_TRAIT.to_owned());
            self.rewrite_count += 1;
            return;
        }
        if is_chrono_timezone_trait(path, scope) {
            scope.kept_chrono_imports.insert("TimeZone".to_owned());
            self.emit_note_at(note_pos, notes::TIMEZONE_BOUND);
        }
        for seg in &path.segments {
            if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                for arg in &args.args {
                    if let syn::GenericArgument::Type(ty) = arg {
                        self.visit_type(ty, scope, None);
                    }
                }
            }
        }
    }

    fn visit_signature(
        &mut self,
        sig: &syn::Signature,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
    ) {
        let sig_pos = token_start(sig);
        for input in &sig.inputs {
            if let syn::FnArg::Typed(pat_type) = input {
                if let Some(kind) = classify_type(&pat_type.ty, scope)
                    && let Some(ident) = simple_pat_ident(&pat_type.pat)
                {
                    scope.var_kinds.insert(ident, kind);
                }
                self.visit_pat(&pat_type.pat, scope, parent, sig_pos);
                self.visit_type(&pat_type.ty, scope, parent);
            }
        }
        if let syn::ReturnType::Type(_, ret_ty) = &sig.output {
            self.visit_type(ret_ty, scope, parent);
        }
    }

    fn visit_block(
        &mut self,
        block: &syn::Block,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
    ) {
        let has_local_use = block
            .stmts
            .iter()
            .any(|s| matches!(s, syn::Stmt::Item(syn::Item::Use(_))));
        if has_local_use {
            let items: Vec<syn::Item> = block
                .stmts
                .iter()
                .filter_map(|s| match s {
                    syn::Stmt::Item(item) => Some(item.clone()),
                    _ => None,
                })
                .collect();
            let mut sub = self.collect_scope(&items, Some(scope));
            sub.inherits_super = true;
            if sub.chrono_uses.is_empty() {
                sub.chrono_active = scope.chrono_active;
            }
            sub.type_params = scope.type_params.clone();
            sub.var_kinds = scope.var_kinds.clone();
            for stmt in &block.stmts {
                self.visit_stmt(stmt, &mut sub, Some(scope));
            }
            self.finalize_scope_uses(&mut sub);
            if sub.chrono_uses.is_empty() && sub.existing_target_use.is_none() {
                scope
                    .needed_target_imports
                    .extend(sub.needed_target_imports);
                scope
                    .needed_aliased_imports
                    .extend(sub.needed_aliased_imports);
                scope.kept_chrono_imports.extend(sub.kept_chrono_imports);
            }
            return;
        }

        for stmt in &block.stmts {
            self.visit_stmt(stmt, scope, parent);
        }
    }

    fn visit_stmt(
        &mut self,
        stmt: &syn::Stmt,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
    ) {
        match stmt {
            syn::Stmt::Local(local) => {
                let stmt_pos = token_start(local);
                let mut explicit_kind = None;
                if let syn::Pat::Type(pt) = &local.pat {
                    explicit_kind = classify_type(&pt.ty, scope);
                    self.visit_pat(&pt.pat, scope, parent, stmt_pos);
                    self.visit_type(&pt.ty, scope, parent);
                } else {
                    self.visit_pat(&local.pat, scope, parent, stmt_pos);
                }
                let mut init_kind = None;
                if let Some(init) = &local.init {
                    init_kind = classify_expr(&init.expr, scope);
                    self.visit_expr_with_stmt(&init.expr, scope, parent, stmt_pos);
                    if let Some((_, div)) = &init.diverge {
                        self.visit_expr_with_stmt(div, scope, parent, stmt_pos);
                    }
                }
                let ident_opt = match &local.pat {
                    syn::Pat::Type(pt) => simple_pat_ident(&pt.pat),
                    other => simple_pat_ident(other),
                };
                if let Some(ident) = ident_opt
                    && let Some(kind) = explicit_kind.or(init_kind)
                {
                    scope.var_kinds.insert(ident, kind);
                }
            }
            syn::Stmt::Item(item) => {
                if !matches!(item, syn::Item::Use(_)) {
                    self.visit_item(item, scope, parent);
                }
            }
            syn::Stmt::Expr(expr, _) => {
                let stmt_pos = token_start(expr);
                self.visit_expr_with_stmt(expr, scope, parent, stmt_pos);
            }
            syn::Stmt::Macro(m) => {
                let stmt_pos = token_start(m);
                self.visit_macro_tokens(&m.mac, scope, parent, stmt_pos);
            }
        }
    }

    fn visit_pat(
        &mut self,
        pat: &syn::Pat,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        match pat {
            syn::Pat::Path(pp) => {
                if let Some(qself) = &pp.qself {
                    self.visit_type(&qself.ty, scope, parent);
                }
                self.rewrite_value_or_pat_path(
                    &pp.path,
                    pp.qself.is_none(),
                    scope,
                    parent,
                    stmt_pos,
                );
            }
            syn::Pat::TupleStruct(ts) => {
                if let Some(qself) = &ts.qself {
                    self.visit_type(&qself.ty, scope, parent);
                }
                self.rewrite_value_or_pat_path(
                    &ts.path,
                    ts.qself.is_none(),
                    scope,
                    parent,
                    stmt_pos,
                );
                for elem in &ts.elems {
                    self.visit_pat(elem, scope, parent, stmt_pos);
                }
            }
            syn::Pat::Struct(ps) => {
                if let Some(qself) = &ps.qself {
                    self.visit_type(&qself.ty, scope, parent);
                }
                self.rewrite_value_or_pat_path(
                    &ps.path,
                    ps.qself.is_none(),
                    scope,
                    parent,
                    stmt_pos,
                );
                for f in &ps.fields {
                    self.visit_pat(&f.pat, scope, parent, stmt_pos);
                }
            }
            syn::Pat::Ident(pi) => {
                if let Some((_, sub)) = &pi.subpat {
                    self.visit_pat(sub, scope, parent, stmt_pos);
                }
            }
            syn::Pat::Or(or) => {
                for case in &or.cases {
                    self.visit_pat(case, scope, parent, stmt_pos);
                }
            }
            syn::Pat::Tuple(tup) => {
                for elem in &tup.elems {
                    self.visit_pat(elem, scope, parent, stmt_pos);
                }
            }
            syn::Pat::Slice(sl) => {
                for elem in &sl.elems {
                    self.visit_pat(elem, scope, parent, stmt_pos);
                }
            }
            syn::Pat::Reference(r) => self.visit_pat(&r.pat, scope, parent, stmt_pos),
            syn::Pat::Paren(p) => self.visit_pat(&p.pat, scope, parent, stmt_pos),
            syn::Pat::Type(pt) => {
                self.visit_pat(&pt.pat, scope, parent, stmt_pos);
                self.visit_type(&pt.ty, scope, parent);
            }
            other => {
                struct ChildPat<'e, 's, 'p, 'a> {
                    engine: &'e mut Engine<'a>,
                    scope: &'s mut ScopeState,
                    parent: Option<&'p ScopeState>,
                    stmt_pos: usize,
                }
                impl<'ast> syn::visit::Visit<'ast> for ChildPat<'_, '_, '_, '_> {
                    fn visit_pat(&mut self, node: &'ast syn::Pat) {
                        self.engine
                            .visit_pat(node, self.scope, self.parent, self.stmt_pos);
                    }
                    fn visit_expr(&mut self, node: &'ast syn::Expr) {
                        self.engine.visit_expr_with_stmt(
                            node,
                            self.scope,
                            self.parent,
                            self.stmt_pos,
                        );
                    }
                    fn visit_type(&mut self, node: &'ast syn::Type) {
                        self.engine.visit_type(node, self.scope, self.parent);
                    }
                }
                syn::visit::visit_pat(
                    &mut ChildPat {
                        engine: self,
                        scope,
                        parent,
                        stmt_pos,
                    },
                    other,
                );
            }
        }
    }

    fn visit_type(&mut self, ty: &syn::Type, scope: &mut ScopeState, parent: Option<&ScopeState>) {
        match ty {
            syn::Type::Path(tp) => {
                if let Some(qself) = &tp.qself {
                    self.visit_type(&qself.ty, scope, parent);
                }
                if tp.qself.is_none()
                    && let Some(range) = token_range(tp)
                {
                    if let Some((target_name, inline, alias)) =
                        match_chrono_type_path(&tp.path, scope)
                    {
                        let replacement = self.emit_target_symbol(
                            scope,
                            parent,
                            target_name,
                            inline,
                            alias.as_deref(),
                        );
                        self.edits.replace(range, replacement);
                        self.rewrite_count += 1;
                        return;
                    }
                    if is_chrono_local_result_path(&tp.path, scope) {
                        scope.kept_chrono_imports.insert("LocalResult".to_owned());
                        self.emit_note_at(range.start, notes::LOCAL_DISAMBIGUATION);
                    }
                }
                if let Some(range) =
                    orm_timestamp_path_ident_range(&tp.path, scope, self.is_navree_orm)
                {
                    self.edits.replace(range, ORM_NEW_TRAIT.to_owned());
                    self.rewrite_count += 1;
                }
                // Recurse into generic arguments (`Option<DateTime<Utc>>`, `Result<NaiveDate, E>`, ...).
                for seg in &tp.path.segments {
                    match &seg.arguments {
                        syn::PathArguments::AngleBracketed(ab) => {
                            for arg in &ab.args {
                                match arg {
                                    syn::GenericArgument::Type(inner) => {
                                        self.visit_type(inner, scope, parent);
                                    }
                                    syn::GenericArgument::AssocType(assoc) => {
                                        self.visit_type(&assoc.ty, scope, parent);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        syn::PathArguments::Parenthesized(pa) => {
                            for input in &pa.inputs {
                                self.visit_type(&input.ty, scope, parent);
                            }
                            if let syn::ReturnType::Type(_, out) = &pa.output {
                                self.visit_type(out, scope, parent);
                            }
                        }
                        syn::PathArguments::None => {}
                    }
                }
            }
            syn::Type::Reference(r) => self.visit_type(&r.elem, scope, parent),
            syn::Type::Slice(s) => self.visit_type(&s.elem, scope, parent),
            syn::Type::Array(a) => {
                self.visit_type(&a.elem, scope, parent);
                self.visit_expr(&a.len, scope, parent);
            }
            syn::Type::Tuple(t) => {
                for elem in &t.elems {
                    self.visit_type(elem, scope, parent);
                }
            }
            syn::Type::Paren(p) => self.visit_type(&p.elem, scope, parent),
            syn::Type::Group(g) => self.visit_type(&g.elem, scope, parent),
            syn::Type::Ptr(p) => self.visit_type(&p.elem, scope, parent),
            syn::Type::ImplTrait(it) => {
                for b in &it.bounds {
                    self.visit_type_param_bound(b, scope, parent, token_start(it));
                }
            }
            syn::Type::TraitObject(to) => {
                for b in &to.bounds {
                    self.visit_type_param_bound(b, scope, parent, token_start(to));
                }
            }
            other => {
                struct ChildTypes<'e, 's, 'p, 'a> {
                    engine: &'e mut Engine<'a>,
                    scope: &'s mut ScopeState,
                    parent: Option<&'p ScopeState>,
                }
                impl<'ast> syn::visit::Visit<'ast> for ChildTypes<'_, '_, '_, '_> {
                    fn visit_type(&mut self, node: &'ast syn::Type) {
                        self.engine.visit_type(node, self.scope, self.parent);
                    }
                }
                syn::visit::visit_type(
                    &mut ChildTypes {
                        engine: self,
                        scope,
                        parent,
                    },
                    other,
                );
            }
        }
    }

    fn visit_expr(
        &mut self,
        expr: &syn::Expr,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
    ) {
        self.visit_expr_with_stmt(expr, scope, parent, token_start(expr));
    }

    fn visit_expr_with_stmt(
        &mut self,
        expr: &syn::Expr,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        match expr {
            syn::Expr::Block(b) => {
                self.visit_block(&b.block, scope, parent);
            }
            syn::Expr::If(ei) => {
                self.visit_expr_with_stmt(&ei.cond, scope, parent, stmt_pos);
                self.visit_block(&ei.then_branch, scope, parent);
                if let Some((_, else_expr)) = &ei.else_branch {
                    self.visit_expr_with_stmt(else_expr, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Match(em) => {
                self.visit_expr_with_stmt(&em.expr, scope, parent, stmt_pos);
                for arm in &em.arms {
                    self.visit_pat(&arm.pat, scope, parent, stmt_pos);
                    let arm_pos = token_start(&arm.body);
                    self.visit_expr_with_stmt(&arm.body, scope, parent, arm_pos);
                }
            }
            syn::Expr::Closure(cl) => {
                let saved_vars = scope.var_kinds.clone();
                for pat in &cl.inputs {
                    if let syn::Pat::Type(pt) = pat {
                        if let Some(kind) = classify_type(&pt.ty, scope)
                            && let Some(ident) = simple_pat_ident(&pt.pat)
                        {
                            scope.var_kinds.insert(ident, kind);
                        }
                        self.visit_pat(&pt.pat, scope, parent, stmt_pos);
                        self.visit_type(&pt.ty, scope, parent);
                    } else {
                        self.visit_pat(pat, scope, parent, stmt_pos);
                    }
                }
                if let syn::ReturnType::Type(_, ty) = &cl.output {
                    self.visit_type(ty, scope, parent);
                }
                self.visit_expr_with_stmt(&cl.body, scope, parent, stmt_pos);
                scope.var_kinds = saved_vars;
            }
            syn::Expr::Let(el) => {
                self.visit_pat(&el.pat, scope, parent, stmt_pos);
                self.visit_expr_with_stmt(&el.expr, scope, parent, stmt_pos);
            }
            syn::Expr::ForLoop(fl) => {
                self.visit_pat(&fl.pat, scope, parent, stmt_pos);
                self.visit_expr_with_stmt(&fl.expr, scope, parent, stmt_pos);
                self.visit_block(&fl.body, scope, parent);
            }
            syn::Expr::While(w) => {
                self.visit_expr_with_stmt(&w.cond, scope, parent, stmt_pos);
                self.visit_block(&w.body, scope, parent);
            }
            syn::Expr::Loop(l) => self.visit_block(&l.body, scope, parent),
            syn::Expr::Async(a) => self.visit_block(&a.block, scope, parent),
            syn::Expr::Unsafe(u) => self.visit_block(&u.block, scope, parent),
            syn::Expr::Path(ep) => {
                if let Some(qself) = &ep.qself {
                    self.visit_type(&qself.ty, scope, parent);
                    let pos = qself.position;
                    // `<Ty as Trait>::method`: check trait path segments `0..pos`.
                    if is_qself_orm_timestamp(&ep.path, pos, scope, self.is_navree_orm)
                        && let Some(seg) = ep.path.segments.get(pos - 1)
                    {
                        self.edits
                            .replace(seg.ident.span().byte_range(), ORM_NEW_TRAIT.to_owned());
                        self.rewrite_count += 1;
                    }
                } else {
                    self.rewrite_value_or_pat_path(&ep.path, true, scope, parent, stmt_pos);
                }
                for seg in &ep.path.segments {
                    if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                        for arg in &ab.args {
                            if let syn::GenericArgument::Type(ty) = arg {
                                self.visit_type(ty, scope, parent);
                            }
                        }
                    }
                }
            }
            syn::Expr::Call(call) => {
                self.visit_call(call, scope, parent, stmt_pos, false);
            }
            syn::Expr::MethodCall(mc) => {
                self.visit_method_call(mc, scope, parent, stmt_pos, false);
            }
            syn::Expr::Binary(bin) => {
                self.visit_expr_with_stmt(&bin.left, scope, parent, stmt_pos);
                self.visit_expr_with_stmt(&bin.right, scope, parent, stmt_pos);
                if matches!(
                    bin.op,
                    syn::BinOp::Add(_)
                        | syn::BinOp::Sub(_)
                        | syn::BinOp::AddAssign(_)
                        | syn::BinOp::SubAssign(_)
                ) {
                    if is_days_or_months_ctor(&bin.right, scope) {
                        self.emit_note_at(stmt_pos, notes::CALENDAR_STEP);
                    } else if classify_expr(&bin.left, scope) == Some(KnownKind::Zoned)
                        && is_day_or_week_span_ctor(&bin.right, scope)
                    {
                        self.emit_note_at(stmt_pos, notes::ZONED_SPAN_VS_CALENDAR);
                    }
                }
            }
            syn::Expr::Unary(u) => self.visit_expr_with_stmt(&u.expr, scope, parent, stmt_pos),
            syn::Expr::Try(t) => self.visit_expr_with_stmt(&t.expr, scope, parent, stmt_pos),
            syn::Expr::Await(a) => self.visit_expr_with_stmt(&a.base, scope, parent, stmt_pos),
            syn::Expr::Field(f) => self.visit_expr_with_stmt(&f.base, scope, parent, stmt_pos),
            syn::Expr::Index(i) => {
                self.visit_expr_with_stmt(&i.expr, scope, parent, stmt_pos);
                self.visit_expr_with_stmt(&i.index, scope, parent, stmt_pos);
            }
            syn::Expr::Cast(c) => {
                self.visit_expr_with_stmt(&c.expr, scope, parent, stmt_pos);
                self.visit_type(&c.ty, scope, parent);
            }
            syn::Expr::Reference(r) => self.visit_expr_with_stmt(&r.expr, scope, parent, stmt_pos),
            syn::Expr::Paren(p) => self.visit_expr_with_stmt(&p.expr, scope, parent, stmt_pos),
            syn::Expr::Group(g) => self.visit_expr_with_stmt(&g.expr, scope, parent, stmt_pos),
            syn::Expr::Tuple(t) => {
                for elem in &t.elems {
                    self.visit_expr_with_stmt(elem, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Array(a) => {
                for elem in &a.elems {
                    self.visit_expr_with_stmt(elem, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Repeat(r) => {
                self.visit_expr_with_stmt(&r.expr, scope, parent, stmt_pos);
                self.visit_expr_with_stmt(&r.len, scope, parent, stmt_pos);
            }
            syn::Expr::Struct(s) => {
                if let Some(qself) = &s.qself {
                    self.visit_type(&qself.ty, scope, parent);
                }
                for f in &s.fields {
                    self.visit_expr_with_stmt(&f.expr, scope, parent, stmt_pos);
                }
                if let Some(rest) = &s.rest {
                    self.visit_expr_with_stmt(rest, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Assign(a) => {
                self.visit_expr_with_stmt(&a.left, scope, parent, stmt_pos);
                self.visit_expr_with_stmt(&a.right, scope, parent, stmt_pos);
            }
            syn::Expr::Range(r) => {
                if let Some(start) = &r.start {
                    self.visit_expr_with_stmt(start, scope, parent, stmt_pos);
                }
                if let Some(end) = &r.end {
                    self.visit_expr_with_stmt(end, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Return(r) => {
                if let Some(val) = &r.expr {
                    self.visit_expr_with_stmt(val, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Break(b) => {
                if let Some(val) = &b.expr {
                    self.visit_expr_with_stmt(val, scope, parent, stmt_pos);
                }
            }
            syn::Expr::Macro(m) => {
                self.visit_macro_tokens(&m.mac, scope, parent, stmt_pos);
            }
            _ => {}
        }
    }

    fn rewrite_value_or_pat_path(
        &mut self,
        path: &syn::Path,
        no_qself: bool,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        if !no_qself {
            return;
        }
        let Some(range) = token_range(path) else {
            return;
        };

        if let Some((chrono_ty, member, inline, alias)) = match_chrono_two_seg(path, scope) {
            if chrono_ty == "LocalResult" {
                scope.kept_chrono_imports.insert("LocalResult".to_owned());
                self.emit_note_at(stmt_pos, notes::LOCAL_DISAMBIGUATION);
                return;
            }
            if let Some(rule) = rules::find_member_rule(&chrono_ty, &member)
                && (!rule.require_chrono_import_when_bare
                    || inline
                    || scope.chrono_bindings.contains_key(&chrono_ty)
                    || alias.is_some())
            {
                let ty_str = self.emit_target_symbol(
                    scope,
                    parent,
                    rule.target_type,
                    inline,
                    alias.as_deref(),
                );
                self.edits
                    .replace(range, format!("{ty_str}::{}", rule.target_member));
                self.rewrite_count += 1;
                return;
            }
        }

        if let Some(orm_range) = orm_timestamp_path_ident_range(path, scope, self.is_navree_orm) {
            self.edits.replace(orm_range, ORM_NEW_TRAIT.to_owned());
            self.rewrite_count += 1;
        }
    }

    fn visit_call(
        &mut self,
        call: &syn::ExprCall,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
        strip_ok_for_unwrap: bool,
    ) -> bool {
        self.visit_call_inner(call, scope, parent, stmt_pos, strip_ok_for_unwrap, false)
    }

    fn visit_call_inner(
        &mut self,
        call: &syn::ExprCall,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
        strip_ok_for_unwrap: bool,
        force_utc_timestamp: bool,
    ) -> bool {
        // First visit arguments bottom-up so `render_slice` sees any nested rewrites.
        for arg in &call.args {
            self.visit_expr_with_stmt(arg, scope, parent, stmt_pos);
        }

        let Some(call_range) = token_range(call) else {
            self.visit_expr_with_stmt(&call.func, scope, parent, stmt_pos);
            return false;
        };

        let syn::Expr::Path(func_path) = &*call.func else {
            self.visit_expr_with_stmt(&call.func, scope, parent, stmt_pos);
            return false;
        };
        if func_path.qself.is_some() {
            self.visit_expr_with_stmt(&call.func, scope, parent, stmt_pos);
            return false;
        }

        // `StrftimeItems::new(...)` / `chrono::format::StrftimeItems::new(...)`
        if is_strftime_items_new(&func_path.path, scope) {
            if !path_starts_with_chrono(&func_path.path) {
                scope.kept_chrono_imports.insert("StrftimeItems".to_owned());
            }
            self.emit_note_at(stmt_pos, notes::STRFTIME_ITEMS);
            return false;
        }

        // `Days::new(...)` / `Months::new(...)`
        if let Some(dm) = match_days_or_months_new(&func_path.path, scope) {
            if !path_starts_with_chrono(&func_path.path) {
                scope.kept_chrono_imports.insert(dm.to_owned());
            }
            self.emit_note_at(stmt_pos, notes::CALENDAR_STEP);
            return false;
        }

        // `Weekday::try_from(...)`
        if let Some((chrono_ty, fn_name, inline, alias)) =
            match_chrono_two_seg(&func_path.path, scope)
        {
            if chrono_ty == "Weekday" && fn_name == "try_from" && inline {
                let Some(fn_range) = token_range(&func_path.path) else {
                    return false;
                };
                let ty_str = self.emit_target_symbol(scope, parent, "Weekday", true, None);
                self.edits.replace(fn_range, format!("{ty_str}::try_from"));
                self.rewrite_count += 1;
                return true;
            }

            if let Some(rule) = rules::find_call_rule(&chrono_ty, &fn_name, call.args.len())
                && (!rule.require_chrono_import_when_bare
                    || inline
                    || scope.chrono_bindings.contains_key(&chrono_ty)
                    || alias.is_some())
            {
                let suffix = if strip_ok_for_unwrap { "" } else { ".ok()" };
                match rule.kind {
                    CallKind::ZeroArgConst => {
                        let ty_str = self.emit_target_symbol(
                            scope,
                            parent,
                            rule.target_type,
                            inline,
                            alias.as_deref(),
                        );
                        self.edits
                            .replace(call_range, format!("{ty_str}::{}", rule.target_fn));
                        self.rewrite_count += 1;
                        return true;
                    }
                    CallKind::Direct => {
                        let Some(fn_range) = token_range(&func_path.path) else {
                            return false;
                        };
                        let ty_str = self.emit_target_symbol(
                            scope,
                            parent,
                            rule.target_type,
                            inline,
                            alias.as_deref(),
                        );
                        self.edits
                            .replace(fn_range, format!("{ty_str}::{}", rule.target_fn));
                        self.rewrite_count += 1;
                        return true;
                    }
                    CallKind::ResultToOption => {
                        let rendered_args = self.render_call_args(&call.args);
                        let ty_str = self.emit_target_symbol(
                            scope,
                            parent,
                            rule.target_type,
                            inline,
                            alias.as_deref(),
                        );
                        self.edits.replace(
                            call_range,
                            format!("{ty_str}::{}({rendered_args}){suffix}", rule.target_fn),
                        );
                        self.rewrite_count += 1;
                        return true;
                    }
                    CallKind::FromTimestamp => {
                        let secs = self.render_expr(&call.args[0]);
                        let nanos_expr = &call.args[1];
                        let ty_str =
                            self.emit_target_symbol(scope, parent, "Timestamp", inline, None);
                        let replacement = if is_int_lit_zero(nanos_expr) {
                            format!("{ty_str}::from_secs({secs}){suffix}")
                        } else {
                            let nanos = self.render_expr(nanos_expr);
                            format!("{ty_str}::from_secs_nanos({secs}, {nanos}){suffix}")
                        };
                        self.edits.replace(call_range, replacement);
                        self.rewrite_count += 1;
                        return true;
                    }
                    CallKind::SubsecHmsOpt { nanos_per_unit } => {
                        let h = self.render_expr(&call.args[0]);
                        let m = self.render_expr(&call.args[1]);
                        let s = self.render_expr(&call.args[2]);
                        let sub_expr = &call.args[3];
                        let nanos_arg =
                            scale_subsec_arg(&self.render_expr(sub_expr), sub_expr, nanos_per_unit);
                        let ty_str = self.emit_target_symbol(
                            scope,
                            parent,
                            "Time",
                            inline,
                            alias.as_deref(),
                        );
                        self.edits.replace(
                            call_range,
                            format!(
                                "{ty_str}::new({h}, {m}, {s}).and_then(|t| t.with_nanos({nanos_arg})){suffix}"
                            ),
                        );
                        self.rewrite_count += 1;
                        return true;
                    }
                    CallKind::SwapParseArgs => {
                        let text_arg = self.render_expr(&call.args[0]);
                        let pat_expr = &call.args[1];
                        if !is_known_static_pattern(pat_expr, scope) {
                            self.emit_note_at(stmt_pos, notes::RUNTIME_PATTERN);
                        }
                        let pat_arg = self.render_expr(pat_expr);
                        let target_ty = if force_utc_timestamp && rule.target_type == "Zoned" {
                            "Timestamp"
                        } else {
                            rule.target_type
                        };
                        let ty_str = self.emit_target_symbol(
                            scope,
                            parent,
                            target_ty,
                            inline,
                            alias.as_deref(),
                        );
                        self.edits.replace(
                            call_range,
                            format!("{ty_str}::{}({pat_arg}, {text_arg})", rule.target_fn),
                        );
                        self.rewrite_count += 1;
                        return true;
                    }
                    CallKind::ParseRfc => {
                        let text_arg = self.render_expr(&call.args[0]);
                        let target_ty = if force_utc_timestamp {
                            "Timestamp"
                        } else {
                            rule.target_type
                        };
                        let ty_str = self.emit_target_symbol(
                            scope,
                            parent,
                            target_ty,
                            inline,
                            alias.as_deref(),
                        );
                        self.edits.replace(
                            call_range,
                            format!("{ty_str}::{}({text_arg})", rule.target_fn),
                        );
                        self.rewrite_count += 1;
                        return true;
                    }
                }
            }
        }

        // Visit `call.func` normally (e.g. turbofish types `DateTime::<Utc>::from(...)`).
        self.visit_expr_with_stmt(&call.func, scope, parent, stmt_pos);
        false
    }

    fn visit_method_call(
        &mut self,
        mc: &syn::ExprMethodCall,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
        strip_ok_for_unwrap: bool,
    ) {
        let method_name = mc.method.to_string();
        let Some(mc_range) = token_range(mc) else {
            return;
        };

        // 1. `.unwrap()` / `.expect(...)` wrapping an `_opt` constructor/method or `Utc.with_ymd_and_hms(..)`:
        //    strip the intermediate `.ok()` so we emit idiomatic `Date::new(..).unwrap()` instead of `.ok().unwrap()`.
        if (method_name == "unwrap" && mc.args.is_empty())
            || (method_name == "expect" && mc.args.len() == 1)
        {
            for arg in &mc.args {
                self.visit_expr_with_stmt(arg, scope, parent, stmt_pos);
            }
            let suffix = if method_name == "unwrap" {
                ".unwrap()".to_owned()
            } else {
                format!(".expect({})", self.render_expr(&mc.args[0]))
            };

            if let syn::Expr::Call(inner_call) = &*mc.receiver
                && is_result_to_option_call(inner_call, scope)
            {
                self.visit_call(inner_call, scope, parent, stmt_pos, true);
                let inner_rendered = self.render_expr(&mc.receiver);
                self.edits
                    .replace(mc_range, format!("{inner_rendered}{suffix}"));
                return;
            }
            if let syn::Expr::MethodCall(inner_mc) = &*mc.receiver {
                let inner_m = inner_mc.method.to_string();
                if inner_m == "and_hms_opt" && inner_mc.args.len() == 3 && scope.chrono_active {
                    self.visit_expr_with_stmt(&inner_mc.receiver, scope, parent, stmt_pos);
                    for arg in &inner_mc.args {
                        self.visit_expr_with_stmt(arg, scope, parent, stmt_pos);
                    }
                    let recv = self.render_expr(&inner_mc.receiver);
                    let args = self.render_call_args(&inner_mc.args);
                    let inline = !scope.has_use_slot(parent);
                    let time_ty = self.emit_target_symbol(scope, parent, "Time", inline, None);
                    self.edits.replace(
                        mc_range,
                        format!("{recv}.at({time_ty}::new({args}){suffix})"),
                    );
                    self.rewrite_count += 1;
                    return;
                }
                if inner_m == "with_nanosecond" && inner_mc.args.len() == 1 && scope.chrono_active {
                    self.visit_method_call(inner_mc, scope, parent, stmt_pos, true);
                    let inner_rendered = self.render_expr(&mc.receiver);
                    self.edits
                        .replace(mc_range, format!("{inner_rendered}{suffix}"));
                    return;
                }
            }
        }

        // 2. `.single()` / `.earliest()` on `Utc.with_ymd_and_hms(...)` / `Utc.timestamp_opt(...)` -> `.ok()`
        if matches!(method_name.as_str(), "single" | "earliest")
            && mc.args.is_empty()
            && let syn::Expr::MethodCall(inner_mc) = &*mc.receiver
            && is_utc_builder_call(inner_mc, scope)
        {
            self.visit_method_call(inner_mc, scope, parent, stmt_pos, false);
            let inner_rendered = self.render_expr(&mc.receiver);
            self.edits
                .replace(mc_range, format!("{inner_rendered}.ok()"));
            self.rewrite_count += 1;
            return;
        }

        // 3. Collapse `DateTime::parse_from_rfc3339(s)[.ok()][?|.unwrap()].with_timezone(&Utc)`
        //    and `DateTime::parse_from_rfc3339(s)[.ok()].map(|x| x.with_timezone(&Utc))` directly
        //    onto `Timestamp::parse_rfc3339(s)...`!
        if ((method_name == "with_timezone"
            && mc.args.len() == 1
            && is_utc_zone_arg(&mc.args[0], scope))
            || (method_name == "to_utc" && mc.args.is_empty()))
            && scope.chrono_active
            && self.try_collapse_rfc_parse_to_utc(
                &mc.receiver,
                mc_range.clone(),
                scope,
                parent,
                stmt_pos,
            )
        {
            return;
        }
        if method_name == "map"
            && mc.args.len() == 1
            && scope.chrono_active
            && is_utc_closure(&mc.args[0], scope)
            && self.try_collapse_rfc_parse_to_utc(
                &mc.receiver,
                mc_range.clone(),
                scope,
                parent,
                stmt_pos,
            )
        {
            return;
        }

        // Visit receiver and arguments bottom-up.
        self.visit_expr_with_stmt(&mc.receiver, scope, parent, stmt_pos);
        for arg in &mc.args {
            self.visit_expr_with_stmt(arg, scope, parent, stmt_pos);
        }
        if let Some(tf) = &mc.turbofish {
            for arg in &tf.args {
                if let syn::GenericArgument::Type(ty) = arg {
                    self.visit_type(ty, scope, parent);
                }
            }
        }

        // 4. `Utc.with_ymd_and_hms(y, mo, d, h, mi, s)` / `Utc.timestamp_opt(s, n)` / `Utc.timestamp_millis_opt(ms)`
        if let Some((inline_utc, builder_kind)) = match_utc_builder(mc, scope) {
            let ty_str = self.emit_target_symbol(scope, parent, "Timestamp", inline_utc, None);
            let replacement = match builder_kind {
                UtcBuilder::YmdHms => {
                    let args = self.render_call_args(&mc.args);
                    format!("{ty_str}::utc({args})")
                }
                UtcBuilder::TimestampOpt => {
                    let secs = self.render_expr(&mc.args[0]);
                    if is_int_lit_zero(&mc.args[1]) {
                        format!("{ty_str}::from_secs({secs})")
                    } else {
                        let nanos = self.render_expr(&mc.args[1]);
                        format!("{ty_str}::from_secs_nanos({secs}, {nanos})")
                    }
                }
                UtcBuilder::TimestampMillisOpt => {
                    let ms = self.render_expr(&mc.args[0]);
                    format!("{ty_str}::from_millis({ms})")
                }
            };
            self.edits.replace(mc_range, replacement);
            self.rewrite_count += 1;
            return;
        }

        if !scope.chrono_active {
            return;
        }

        // 5. Ambiguous / note-producing methods.
        match (method_name.as_str(), mc.args.len()) {
            (
                "from_local_datetime" | "offset_from_local_datetime" | "offset_from_local_date",
                1,
            ) => {
                if scope.chrono_bindings.contains_key("TimeZone") {
                    scope.kept_chrono_imports.insert("TimeZone".to_owned());
                }
                self.emit_note_at(stmt_pos, notes::LOCAL_DISAMBIGUATION);
                return;
            }
            (
                "checked_add_days" | "checked_sub_days" | "checked_add_months"
                | "checked_sub_months",
                1,
            ) => {
                self.emit_note_at(stmt_pos, notes::CALENDAR_STEP);
                return;
            }
            ("format_with_items", 1) => {
                self.emit_note_at(stmt_pos, notes::STRFTIME_ITEMS);
                return;
            }
            ("format", 1) => {
                if !is_known_static_pattern(&mc.args[0], scope) {
                    self.emit_note_at(stmt_pos, notes::RUNTIME_PATTERN);
                }
                return;
            }
            ("duration_trunc" | "duration_round", 1) => {
                self.emit_note_at(stmt_pos, notes::SUBSEC_TRUNCATION);
                return;
            }
            ("trunc_subsecs", 1) => {
                if is_int_lit_u64(&mc.args[0], 6) {
                    let recv = self.render_expr(&mc.receiver);
                    self.edits
                        .replace(mc_range, format!("{recv}.trunc_micros()"));
                    self.rewrite_count += 1;
                } else {
                    self.emit_note_at(stmt_pos, notes::SUBSEC_TRUNCATION);
                }
                return;
            }
            ("with_timezone", 1) => {
                if is_utc_zone_arg(&mc.args[0], scope) {
                    let recv = self.render_expr(&mc.receiver);
                    self.edits.replace(mc_range, format!("{recv}.timestamp()"));
                    self.rewrite_count += 1;
                } else {
                    self.emit_note_at(stmt_pos, notes::WITH_TIMEZONE);
                }
                return;
            }
            ("fixed_offset", 0) => {
                let recv = self.render_expr(&mc.receiver);
                let inline = !scope.has_use_slot(parent);
                let zone_ty = self.emit_target_symbol(scope, parent, "Zone", inline, None);
                let off_ty = self.emit_target_symbol(scope, parent, "Offset", inline, None);
                self.edits.replace(
                    mc_range,
                    format!("{recv}.in_zone({zone_ty}::fixed({off_ty}::UTC))"),
                );
                self.rewrite_count += 1;
                return;
            }
            ("date_naive", 0) => {
                let recv = self.render_expr(&mc.receiver);
                let replacement = if classify_expr(&mc.receiver, scope) == Some(KnownKind::Zoned) {
                    format!("{recv}.date()")
                } else {
                    format!("{recv}.date_time_utc().date()")
                };
                self.edits.replace(mc_range, replacement);
                self.rewrite_count += 1;
                return;
            }
            ("and_hms_opt", 3) => {
                let recv = self.render_expr(&mc.receiver);
                let args = self.render_call_args(&mc.args);
                let inline = !scope.has_use_slot(parent);
                let time_ty = self.emit_target_symbol(scope, parent, "Time", inline, None);
                let replacement = if strip_ok_for_unwrap {
                    format!("{recv}.at({time_ty}::new({args}).unwrap())")
                } else if is_simple_ident_expr(&mc.receiver) {
                    format!("{time_ty}::new({args}).ok().map(|t| {recv}.at(t))")
                } else {
                    format!(
                        "{{ let __date = {recv}; {time_ty}::new({args}).ok().map(|t| __date.at(t)) }}"
                    )
                };
                self.edits.replace(mc_range, replacement);
                self.rewrite_count += 1;
                return;
            }
            ("with_nanosecond", 1) => {
                let recv = self.render_expr(&mc.receiver);
                let ns = self.render_expr(&mc.args[0]);
                let suffix = if strip_ok_for_unwrap { "" } else { ".ok()" };
                self.edits
                    .replace(mc_range, format!("{recv}.with_nanos({ns}){suffix}"));
                self.rewrite_count += 1;
                return;
            }
            _ => {}
        }

        // 6. Direct 1-to-1 method rename rules (`rules::METHOD_RENAME_RULES`).
        if let Some(rule) = rules::find_method_rename(&method_name, mc.args.len()) {
            // Guard `.timestamp()`: never touch a receiver already typed `Zoned`, and only run
            // when an instant source from `chrono` was actually present in this scope.
            if rule.chrono_method == "timestamp" {
                if classify_expr(&mc.receiver, scope) == Some(KnownKind::Zoned) {
                    return;
                }
                if !scope_has_chrono_instants(scope) && !expr_mentions_chrono(&mc.receiver) {
                    return;
                }
            }
            if matches!(
                rule.chrono_method,
                "checked_add_signed" | "checked_sub_signed"
            ) && classify_expr(&mc.receiver, scope) == Some(KnownKind::Zoned)
                && is_day_or_week_span_ctor(&mc.args[0], scope)
            {
                self.emit_note_at(stmt_pos, notes::ZONED_SPAN_VS_CALENDAR);
            }
            let target_method = if rule.chrono_method == "checked_add_signed"
                && classify_expr(&mc.receiver, scope) == Some(KnownKind::DateTime)
            {
                "add"
            } else {
                rule.target_method
            };
            self.edits
                .replace(mc.method.span().byte_range(), target_method.to_owned());
            self.rewrite_count += 1;
        }
    }

    fn try_collapse_rfc_parse_to_utc(
        &mut self,
        receiver: &syn::Expr,
        outer_range: Range<usize>,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) -> bool {
        let Some(inner_call) = peel_result_adapters(receiver) else {
            return false;
        };
        let syn::Expr::Path(fp) = &*inner_call.func else {
            return false;
        };
        let Some((chrono_ty, fn_name, _, _)) = match_chrono_two_seg(&fp.path, scope) else {
            return false;
        };
        if chrono_ty != "DateTime"
            || !matches!(
                fn_name.as_str(),
                "parse_from_rfc3339" | "parse_from_rfc2822" | "parse_from_str"
            )
        {
            return false;
        }

        // Rewrite `receiver` with `force_utc_timestamp = true` on `inner_call`, then replace
        // `outer_range` with the rewritten `receiver` (dropping `.with_timezone(&Utc)` / `.map(..)`).
        self.visit_peeled_rfc_chain(receiver, scope, parent, stmt_pos);
        let rewritten_recv = self.render_expr(receiver);
        self.edits.replace(outer_range, rewritten_recv);
        true
    }

    fn visit_peeled_rfc_chain(
        &mut self,
        expr: &syn::Expr,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        match expr {
            syn::Expr::Call(call) => {
                self.visit_call_inner(call, scope, parent, stmt_pos, false, true);
            }
            syn::Expr::Try(t) => self.visit_peeled_rfc_chain(&t.expr, scope, parent, stmt_pos),
            syn::Expr::Paren(p) => self.visit_peeled_rfc_chain(&p.expr, scope, parent, stmt_pos),
            syn::Expr::MethodCall(mc) => {
                self.visit_peeled_rfc_chain(&mc.receiver, scope, parent, stmt_pos);
                for arg in &mc.args {
                    self.visit_expr_with_stmt(arg, scope, parent, stmt_pos);
                }
            }
            _ => {}
        }
    }

    fn visit_macro_tokens(
        &mut self,
        mac: &syn::Macro,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        let mac_name = mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        // `matches!(expr, pat [if guard])`
        if mac_name == "matches"
            && let Ok(parsed) = syn::parse2::<MatchesMacro>(mac.tokens.clone())
        {
            self.visit_expr_with_stmt(&parsed.expr, scope, parent, stmt_pos);
            self.visit_pat(&parsed.pat, scope, parent, stmt_pos);
            if let Some(guard) = &parsed.guard {
                self.visit_expr_with_stmt(guard, scope, parent, stmt_pos);
            }
            return;
        }

        // Standard comma-separated expression macros (`assert_eq!`, `assert!`, `vec!`, `format!`, `println!`, ...).
        if let Ok(exprs) = syn::parse::Parser::parse2(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
            mac.tokens.clone(),
        ) {
            for expr in &exprs {
                self.visit_expr_with_stmt(expr, scope, parent, stmt_pos);
            }
            return;
        }

        // Fallback for `vec![elem; len]` or structured macros (`json!({ "k": expr })`).
        self.visit_arbitrary_token_stream(mac.tokens.clone(), scope, parent, stmt_pos);
    }

    fn visit_arbitrary_token_stream(
        &mut self,
        stream: TokenStream,
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        let trees: Vec<TokenTree> = stream.into_iter().collect();
        // Split on `,`, `;`, `:` and try parsing each non-empty slice as an `Expr`; recurse into groups.
        let mut slice_start = 0usize;
        for (idx, tt) in trees.iter().enumerate() {
            let is_sep =
                matches!(tt, TokenTree::Punct(p) if matches!(p.as_char(), ',' | ';' | ':'));
            if is_sep {
                self.try_visit_token_slice(&trees[slice_start..idx], scope, parent, stmt_pos);
                slice_start = idx + 1;
            }
        }
        self.try_visit_token_slice(&trees[slice_start..], scope, parent, stmt_pos);
    }

    fn try_visit_token_slice(
        &mut self,
        slice: &[TokenTree],
        scope: &mut ScopeState,
        parent: Option<&ScopeState>,
        stmt_pos: usize,
    ) {
        if slice.is_empty() {
            return;
        }
        let ts: TokenStream = slice.iter().cloned().collect();
        if let Ok(expr) = syn::parse2::<syn::Expr>(ts)
            && !matches!(expr, syn::Expr::Block(_) | syn::Expr::Macro(_))
        {
            self.visit_expr_with_stmt(&expr, scope, parent, stmt_pos);
            return;
        }
        for tt in slice {
            if let TokenTree::Group(g) = tt {
                self.visit_arbitrary_token_stream(g.stream(), scope, parent, stmt_pos);
            }
        }
    }

    fn render_expr(&self, expr: &syn::Expr) -> String {
        token_range(expr)
            .map(|r| self.edits.render_slice(self.source, r))
            .unwrap_or_else(|| expr.to_token_stream().to_string())
    }

    fn render_call_args(
        &self,
        args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    ) -> String {
        let Some(first) = args.first() else {
            return String::new();
        };
        let Some(last) = args.last() else {
            return String::new();
        };
        if let (Some(fr), Some(lr)) = (token_range(first), token_range(last)) {
            self.edits.render_slice(self.source, fr.start..lr.end)
        } else {
            args.iter()
                .map(|a| self.render_expr(a))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

struct MatchesMacro {
    expr: syn::Expr,
    pat: syn::Pat,
    guard: Option<syn::Expr>,
}

impl syn::parse::Parse for MatchesMacro {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let expr: syn::Expr = input.parse()?;
        let _: syn::Token![,] = input.parse()?;
        let pat = syn::Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(syn::Token![if]) {
            let _: syn::Token![if] = input.parse()?;
            Some(input.parse()?)
        } else {
            None
        };
        let _ = input.parse::<Option<syn::Token![,]>>()?;
        Ok(Self { expr, pat, guard })
    }
}

#[derive(Clone, Copy)]
enum UtcBuilder {
    YmdHms,
    TimestampOpt,
    TimestampMillisOpt,
}

fn match_utc_builder(mc: &syn::ExprMethodCall, scope: &ScopeState) -> Option<(bool, UtcBuilder)> {
    let syn::Expr::Path(recv_path) = &*mc.receiver else {
        return None;
    };
    if recv_path.qself.is_some() {
        return None;
    }
    let inline = if is_inline_chrono_single(&recv_path.path, "Utc") {
        true
    } else if recv_path.path.is_ident("Utc") && scope.resolve_chrono_ident("Utc") == Some("Utc") {
        false
    } else {
        return None;
    };
    let name = mc.method.to_string();
    match (name.as_str(), mc.args.len()) {
        ("with_ymd_and_hms", 6) => Some((inline, UtcBuilder::YmdHms)),
        ("timestamp_opt", 2) => Some((inline, UtcBuilder::TimestampOpt)),
        ("timestamp_millis_opt", 1) => Some((inline, UtcBuilder::TimestampMillisOpt)),
        _ => None,
    }
}

fn is_utc_builder_call(mc: &syn::ExprMethodCall, scope: &ScopeState) -> bool {
    match_utc_builder(mc, scope).is_some()
}

fn is_result_to_option_call(call: &syn::ExprCall, scope: &ScopeState) -> bool {
    let syn::Expr::Path(fp) = &*call.func else {
        return false;
    };
    if fp.qself.is_some() {
        return false;
    }
    let Some((chrono_ty, fn_name, inline, alias)) = match_chrono_two_seg(&fp.path, scope) else {
        return false;
    };
    let Some(rule) = rules::find_call_rule(&chrono_ty, &fn_name, call.args.len()) else {
        return false;
    };
    if rule.require_chrono_import_when_bare
        && !inline
        && !scope.chrono_bindings.contains_key(&chrono_ty)
        && alias.is_none()
    {
        return false;
    }
    matches!(
        rule.kind,
        CallKind::ResultToOption | CallKind::FromTimestamp | CallKind::SubsecHmsOpt { .. }
    )
}

fn peel_result_adapters(mut expr: &syn::Expr) -> Option<&syn::ExprCall> {
    loop {
        match expr {
            syn::Expr::Call(call) => return Some(call),
            syn::Expr::Try(t) => expr = &t.expr,
            syn::Expr::Paren(p) => expr = &p.expr,
            syn::Expr::MethodCall(mc) => {
                let m = mc.method.to_string();
                if (matches!(m.as_str(), "unwrap" | "ok") && mc.args.is_empty())
                    || (m == "expect" && mc.args.len() == 1)
                {
                    expr = &mc.receiver;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

fn is_utc_closure(expr: &syn::Expr, scope: &ScopeState) -> bool {
    let syn::Expr::Closure(cl) = expr else {
        return false;
    };
    if cl.inputs.len() != 1 {
        return false;
    }
    let Some(param) = simple_pat_ident(&cl.inputs[0]) else {
        return false;
    };
    let syn::Expr::MethodCall(mc) = &*cl.body else {
        return false;
    };
    let syn::Expr::Path(rp) = &*mc.receiver else {
        return false;
    };
    if !rp.path.is_ident(&param) {
        return false;
    }
    let m = mc.method.to_string();
    (m == "with_timezone" && mc.args.len() == 1 && is_utc_zone_arg(&mc.args[0], scope))
        || (m == "to_utc" && mc.args.is_empty())
}

fn is_utc_zone_arg(expr: &syn::Expr, scope: &ScopeState) -> bool {
    let inner = match expr {
        syn::Expr::Reference(r) => &*r.expr,
        other => other,
    };
    let syn::Expr::Path(ep) = inner else {
        return false;
    };
    if ep.qself.is_some() {
        return false;
    }
    is_inline_chrono_single(&ep.path, "Utc")
        || (ep.path.is_ident("Utc") && scope.resolve_chrono_ident("Utc") == Some("Utc"))
}

fn is_inline_chrono_single(path: &syn::Path, name: &str) -> bool {
    path.segments.len() == 2 && path.segments[0].ident == "chrono" && path.segments[1].ident == name
}

fn path_starts_with_chrono(path: &syn::Path) -> bool {
    path.segments.first().is_some_and(|s| s.ident == "chrono")
}

/// Matches `chrono::Type::member` or imported `Type::member` (including `DateTime::<Utc>::member`).
/// Returns `(canonical_chrono_type, member_name, is_inline_chrono, optional_alias_used)`.
fn match_chrono_two_seg(
    path: &syn::Path,
    scope: &ScopeState,
) -> Option<(String, String, bool, Option<String>)> {
    let segs: Vec<&syn::PathSegment> = path.segments.iter().collect();
    match segs.as_slice() {
        [c, ty, member] if c.ident == "chrono" && member.arguments.is_none() => {
            Some((ty.ident.to_string(), member.ident.to_string(), true, None))
        }
        [ty, member] if member.arguments.is_none() => {
            let local_ty = ty.ident.to_string();
            let chrono_name = scope.resolve_chrono_ident(&local_ty)?;
            let alias = scope
                .chrono_bindings
                .get(&local_ty)
                .and_then(|b| b.alias.clone());
            Some((
                chrono_name.to_owned(),
                member.ident.to_string(),
                false,
                alias,
            ))
        }
        _ => None,
    }
}

/// Matches a type `syn::Path` that should be rewritten (`DateTime<Utc>`, `NaiveDate`, `TimeDelta`, ...).
/// Returns `(target_type_name, is_inline_chrono, optional_alias_used)`.
fn match_chrono_type_path(
    path: &syn::Path,
    scope: &ScopeState,
) -> Option<(&'static str, bool, Option<String>)> {
    let segs: Vec<&syn::PathSegment> = path.segments.iter().collect();
    let (ty_seg, inline, alias) = match segs.as_slice() {
        [c, ty] if c.ident == "chrono" => (*ty, true, None),
        [ty] => {
            let local = ty.ident.to_string();
            let chrono_name = scope.resolve_chrono_ident(&local)?;
            let alias = scope
                .chrono_bindings
                .get(&local)
                .and_then(|b| b.alias.clone());
            // Check `DateTime<...>`
            if chrono_name == "DateTime" {
                let target = classify_datetime_type_arg(ty, scope)?;
                return Some((target, false, None));
            }
            let rule = rules::find_type_rule(chrono_name)?;
            if rule.require_chrono_import_when_bare
                && !scope.chrono_bindings.contains_key(&local)
                && alias.is_none()
            {
                return None;
            }
            // Unqualified `Weekday` and `Month` already have the right type name in `navree-datetime`;
            // simply record that the import is needed without rewriting identical text.
            return Some((rule.target_name, false, alias));
        }
        _ => return None,
    };

    let chrono_name = ty_seg.ident.to_string();
    if chrono_name == "DateTime" {
        let target = classify_datetime_type_arg(ty_seg, scope)?;
        return Some((target, inline, None));
    }
    let rule = rules::find_type_rule(&chrono_name)?;
    Some((rule.target_name, inline, alias))
}

fn classify_datetime_type_arg(
    dt_seg: &syn::PathSegment,
    scope: &ScopeState,
) -> Option<&'static str> {
    let syn::PathArguments::AngleBracketed(ab) = &dt_seg.arguments else {
        return None;
    };
    if ab.args.len() != 1 {
        return None;
    }
    let syn::GenericArgument::Type(syn::Type::Path(arg_tp)) = &ab.args[0] else {
        return None;
    };
    if arg_tp.qself.is_some() {
        return None;
    }
    let seg_names: Vec<String> = arg_tp
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    match seg_names.as_slice() {
        [one] if one == "Utc" => Some("Timestamp"),
        [c, one] if c == "chrono" && one == "Utc" => Some("Timestamp"),
        [p, one] if p == "__private" && one == "Utc" => Some("Timestamp"),
        [_, p, one] if p == "__private" && one == "Utc" => Some("Timestamp"),
        [one] if one == "FixedOffset" => Some("Zoned"),
        [c, one] if c == "chrono" && one == "FixedOffset" => Some("Zoned"),
        [one] if one == "Tz" && !scope.type_params.contains("Tz") => Some("Zoned"),
        [.., last] if last == "Tz" => Some("Zoned"),
        _ => None,
    }
}

fn classify_type(ty: &syn::Type, scope: &ScopeState) -> Option<KnownKind> {
    let syn::Type::Path(tp) = peel_ref_type(ty) else {
        return None;
    };
    if tp.qself.is_some() {
        return None;
    }
    if let Some((target, _, _)) = match_chrono_type_path(&tp.path, scope) {
        return kind_from_target_name(target);
    }
    let last = tp.path.segments.last()?.ident.to_string();
    let segs: Vec<String> = tp
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if segs.first().is_some_and(|s| s == "navree_datetime")
        || (segs.len() >= 2 && segs[0] == "navree" && segs[1] == "datetime")
        || (segs.len() == 1 && !scope.is_foreign_collision(&last))
    {
        return kind_from_target_name(&last);
    }
    None
}

fn kind_from_target_name(name: &str) -> Option<KnownKind> {
    match name {
        "Timestamp" => Some(KnownKind::Timestamp),
        "Zoned" => Some(KnownKind::Zoned),
        "Date" => Some(KnownKind::Date),
        "Time" => Some(KnownKind::Time),
        "DateTime" => Some(KnownKind::DateTime),
        "Span" => Some(KnownKind::Span),
        "Offset" => Some(KnownKind::Offset),
        _ => None,
    }
}

fn peel_ref_type(mut ty: &syn::Type) -> &syn::Type {
    while let syn::Type::Reference(r) = ty {
        ty = &r.elem;
    }
    ty
}

fn classify_expr(expr: &syn::Expr, scope: &ScopeState) -> Option<KnownKind> {
    match expr {
        syn::Expr::Paren(p) => classify_expr(&p.expr, scope),
        syn::Expr::Reference(r) => classify_expr(&r.expr, scope),
        syn::Expr::Try(t) => classify_expr(&t.expr, scope),
        syn::Expr::Path(ep) if ep.qself.is_none() && ep.path.segments.len() == 1 => {
            let ident = ep.path.segments[0].ident.to_string();
            scope.var_kinds.get(&ident).copied()
        }
        syn::Expr::Call(call) => {
            let syn::Expr::Path(fp) = &*call.func else {
                return None;
            };
            if let Some((chrono_ty, fn_name, _, _)) = match_chrono_two_seg(&fp.path, scope)
                && let Some(rule) = rules::find_call_rule(&chrono_ty, &fn_name, call.args.len())
            {
                return kind_from_target_name(rule.target_type);
            }
            let last_two: Vec<String> = fp
                .path
                .segments
                .iter()
                .rev()
                .take(2)
                .map(|s| s.ident.to_string())
                .collect();
            if let [_, ty] = last_two.as_slice() {
                return kind_from_target_name(ty);
            }
            None
        }
        syn::Expr::MethodCall(mc) => {
            let m = mc.method.to_string();
            match m.as_str() {
                "unwrap" | "expect" | "trunc_subsecs" | "trunc_micros" | "checked_add_signed"
                | "checked_sub_signed" | "checked_add" | "checked_sub" => {
                    classify_expr(&mc.receiver, scope)
                }
                "in_zone" | "with_zone" | "fixed_offset" => Some(KnownKind::Zoned),
                "in_utc" | "to_utc" => Some(KnownKind::Timestamp),
                "with_timezone" if mc.args.len() == 1 && is_utc_zone_arg(&mc.args[0], scope) => {
                    Some(KnownKind::Timestamp)
                }
                "naive_utc" | "naive_local" | "date_time" | "date_time_utc" | "and_hms_opt"
                | "and_time" | "at" => Some(KnownKind::DateTime),
                "date_naive" => Some(KnownKind::Date),
                _ => {
                    if match_utc_builder(mc, scope).is_some() {
                        Some(KnownKind::Timestamp)
                    } else {
                        None
                    }
                }
            }
        }
        _ => None,
    }
}

fn is_chrono_local_result_path(path: &syn::Path, scope: &ScopeState) -> bool {
    let segs: Vec<&syn::PathSegment> = path.segments.iter().collect();
    match segs.as_slice() {
        [c, lr] if c.ident == "chrono" && lr.ident == "LocalResult" => true,
        [lr] if lr.ident == "LocalResult"
            && scope.resolve_chrono_ident("LocalResult") == Some("LocalResult") =>
        {
            true
        }
        _ => false,
    }
}

fn is_chrono_timezone_trait(path: &syn::Path, scope: &ScopeState) -> bool {
    let segs: Vec<&syn::PathSegment> = path.segments.iter().collect();
    match segs.as_slice() {
        [c, tz] if c.ident == "chrono" && tz.ident == "TimeZone" => true,
        [tz] if tz.ident == "TimeZone"
            && (scope.chrono_bindings.contains_key("TimeZone")
                || scope.chrono_trait_underscores.contains("TimeZone")
                || scope.has_chrono_glob) =>
        {
            true
        }
        _ => false,
    }
}

fn is_strftime_items_new(path: &syn::Path, scope: &ScopeState) -> bool {
    let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    match segs.as_slice() {
        [c, f, s, n] if c == "chrono" && f == "format" && s == "StrftimeItems" && n == "new" => {
            true
        }
        [f, s, n] if f == "format" && s == "StrftimeItems" && n == "new" && scope.chrono_active => {
            true
        }
        [s, n]
            if s == "StrftimeItems"
                && n == "new"
                && (scope.chrono_bindings.contains_key("StrftimeItems") || scope.chrono_active) =>
        {
            true
        }
        _ => false,
    }
}

fn match_days_or_months_new(path: &syn::Path, scope: &ScopeState) -> Option<&'static str> {
    let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    match segs.as_slice() {
        [c, dm, n] if c == "chrono" && n == "new" && dm == "Days" => Some("Days"),
        [c, dm, n] if c == "chrono" && n == "new" && dm == "Months" => Some("Months"),
        [dm, n]
            if n == "new" && dm == "Days" && scope.resolve_chrono_ident("Days") == Some("Days") =>
        {
            Some("Days")
        }
        [dm, n]
            if n == "new"
                && dm == "Months"
                && scope.resolve_chrono_ident("Months") == Some("Months") =>
        {
            Some("Months")
        }
        _ => None,
    }
}

fn is_days_or_months_ctor(expr: &syn::Expr, scope: &ScopeState) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(fp) = &*call.func else {
        return false;
    };
    match_days_or_months_new(&fp.path, scope).is_some()
}

fn is_day_or_week_span_ctor(expr: &syn::Expr, scope: &ScopeState) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(fp) = &*call.func else {
        return false;
    };
    if let Some((chrono_ty, fn_name, _, _)) = match_chrono_two_seg(&fp.path, scope)
        && matches!(chrono_ty.as_str(), "TimeDelta" | "Duration")
        && matches!(
            fn_name.as_str(),
            "days" | "weeks" | "try_days" | "try_weeks"
        )
    {
        return true;
    }
    let segs: Vec<String> = fp
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    matches!(
        segs.as_slice(),
        [.., s, f] if s == "Span" && matches!(f.as_str(), "days" | "weeks" | "try_days" | "try_weeks")
    )
}

fn scope_has_chrono_instants(scope: &ScopeState) -> bool {
    scope.has_chrono_glob
        || scope.chrono_bindings.values().any(|b| {
            matches!(
                b.chrono_name.as_str(),
                "DateTime" | "Utc" | "FixedOffset" | "NaiveDateTime"
            )
        })
}

fn expr_mentions_chrono(expr: &syn::Expr) -> bool {
    let text = expr.to_token_stream().to_string();
    text.contains("chrono")
}

fn is_known_static_pattern(expr: &syn::Expr, scope: &ScopeState) -> bool {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(_),
            ..
        }) => true,
        syn::Expr::Reference(r) => is_known_static_pattern(&r.expr, scope),
        syn::Expr::Paren(p) => is_known_static_pattern(&p.expr, scope),
        syn::Expr::Path(ep) if ep.qself.is_none() => {
            if ep.path.segments.len() == 1 {
                let ident = ep.path.segments[0].ident.to_string();
                scope.const_strings.contains(&ident)
                    || ident
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            } else {
                // Module-qualified `SCREAMING_SNAKE_CASE` constant.
                ep.path.segments.last().is_some_and(|last| {
                    let s = last.ident.to_string();
                    s.chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                })
            }
        }
        _ => false,
    }
}

fn orm_timestamp_path_ident_range(
    path: &syn::Path,
    scope: &ScopeState,
    is_navree_orm: bool,
) -> Option<Range<usize>> {
    let last = path.segments.last()?;
    if last.ident != ORM_OLD_TRAIT || !last.arguments.is_none() {
        return None;
    }
    if path.segments.len() == 1 {
        return scope
            .orm_timestamp_in_scope
            .then(|| last.ident.span().byte_range());
    }
    let prefix: Vec<String> = path
        .segments
        .iter()
        .take(path.segments.len() - 1)
        .map(|s| s.ident.to_string())
        .collect();
    is_orm_module_prefix(&prefix, is_navree_orm).then(|| last.ident.span().byte_range())
}

fn is_qself_orm_timestamp(
    path: &syn::Path,
    pos: usize,
    scope: &ScopeState,
    is_navree_orm: bool,
) -> bool {
    if pos == 0 || pos > path.segments.len() {
        return false;
    }
    let trait_seg = &path.segments[pos - 1];
    if trait_seg.ident != ORM_OLD_TRAIT {
        return false;
    }
    if pos == 1 {
        return scope.orm_timestamp_in_scope;
    }
    let prefix: Vec<String> = path
        .segments
        .iter()
        .take(pos - 1)
        .map(|s| s.ident.to_string())
        .collect();
    is_orm_module_prefix(&prefix, is_navree_orm)
}

fn is_orm_module_prefix(prefix: &[String], is_navree_orm: bool) -> bool {
    let strs: Vec<&str> = prefix.iter().map(String::as_str).collect();
    matches!(
        strs.as_slice(),
        ["navree_orm"] | ["navree_orm", "model"] | ["navree", "orm"] | ["navree", "orm", "model"]
    ) || (is_navree_orm
        && matches!(
            strs.as_slice(),
            ["crate"] | ["crate", "model"] | ["model"] | ["super"]
        ))
}

fn strip_leading_colon_tree(tree: &syn::UseTree) -> &syn::UseTree {
    tree
}

fn match_root_use_tree<'a>(
    mut tree: &'a syn::UseTree,
    segments: &[&str],
) -> Option<&'a syn::UseTree> {
    for &expected in segments {
        match tree {
            syn::UseTree::Path(p) if p.ident == expected => {
                tree = &p.tree;
            }
            _ => return None,
        }
    }
    Some(tree)
}

fn collect_chrono_tree(tree: &syn::UseTree, is_pub: bool, scope: &mut ScopeState) {
    match tree {
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            if name != "self" {
                scope.chrono_bindings.insert(
                    name.clone(),
                    ChronoImportBinding {
                        chrono_name: name,
                        alias: None,
                        is_pub,
                    },
                );
            }
        }
        syn::UseTree::Rename(r) => {
            let orig = r.ident.to_string();
            if r.rename == "_" {
                scope.chrono_trait_underscores.insert(orig);
            } else {
                let alias = r.rename.to_string();
                scope.chrono_bindings.insert(
                    alias.clone(),
                    ChronoImportBinding {
                        chrono_name: orig,
                        alias: Some(alias),
                        is_pub,
                    },
                );
            }
        }
        syn::UseTree::Glob(_) => {
            scope.has_chrono_glob = true;
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                collect_chrono_tree(item, is_pub, scope);
            }
        }
        syn::UseTree::Path(p) => {
            collect_chrono_tree(&p.tree, is_pub, scope);
        }
    }
}

fn collect_plain_use_names(tree: &syn::UseTree, out: &mut BTreeSet<String>) -> bool {
    match tree {
        syn::UseTree::Name(n) if n.ident != "self" => {
            out.insert(n.ident.to_string());
            true
        }
        syn::UseTree::Rename(r) if r.rename != "_" => {
            out.insert(format!("{} as {}", r.ident, r.rename));
            true
        }
        syn::UseTree::Group(g) => g
            .items
            .iter()
            .all(|item| collect_plain_use_names(item, out)),
        _ => false,
    }
}

fn collect_foreign_use_bindings(tree: &syn::UseTree, out: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Name(n) => {
            let s = n.ident.to_string();
            if DATETIME_EXPORTS.contains(&s.as_str()) {
                out.insert(s);
            }
        }
        syn::UseTree::Rename(r) => {
            if r.rename != "_" {
                let s = r.rename.to_string();
                if DATETIME_EXPORTS.contains(&s.as_str()) {
                    out.insert(s);
                }
            }
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                collect_foreign_use_bindings(item, out);
            }
        }
        syn::UseTree::Path(p) => {
            // Skip `navree::datetime::*` and `navree_datetime::*` so existing target imports
            // never register as foreign collisions.
            if p.ident == "navree_datetime" {
                return;
            }
            if p.ident == "navree"
                && matches!(&*p.tree, syn::UseTree::Path(inner) if inner.ident == "datetime")
            {
                return;
            }
            collect_foreign_use_bindings(&p.tree, out);
        }
        syn::UseTree::Glob(_) => {}
    }
}

fn format_use_statement(
    indent: &str,
    vis_prefix: &str,
    root: &str,
    specs: &BTreeSet<String>,
    nl: &str,
) -> String {
    let list: Vec<&str> = specs.iter().map(String::as_str).collect();
    if list.len() == 1 {
        return format!("{vis_prefix}use {root}::{};", list[0]);
    }
    let single = format!("{vis_prefix}use {root}::{{{}}};", list.join(", "));
    if indent.len() + single.len() <= 100 {
        return single;
    }
    let inner_indent = format!("{indent}    ");
    let mut lines = Vec::new();
    let mut current = String::new();
    for item in list {
        if current.is_empty() {
            current.push_str(item);
            current.push(',');
        } else if inner_indent.len() + current.len() + 1 + item.len() < 100 {
            current.push(' ');
            current.push_str(item);
            current.push(',');
        } else {
            lines.push(format!("{inner_indent}{current}"));
            current = format!("{item},");
        }
    }
    if !current.is_empty() {
        lines.push(format!("{inner_indent}{current}"));
    }
    format!(
        "{vis_prefix}use {root}::{{{nl}{}{nl}{indent}}};",
        lines.join(nl)
    )
}

fn is_use_super_glob(tree: &syn::UseTree) -> bool {
    matches!(
        tree,
        syn::UseTree::Path(p) if p.ident == "super" && matches!(&*p.tree, syn::UseTree::Glob(_))
    )
}

fn is_str_const(item_const: &syn::ItemConst) -> bool {
    matches!(
        &*item_const.expr,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(_),
            ..
        })
    )
}

fn items_contain_inline_chrono(items: &[syn::Item]) -> bool {
    struct Finder(bool);
    impl<'ast> syn::visit::Visit<'ast> for Finder {
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            // Inline submodules have their own scope check.
            if node.content.is_none() {
                syn::visit::visit_item_mod(self, node);
            }
        }
        fn visit_path(&mut self, node: &'ast syn::Path) {
            if node.segments.first().is_some_and(|s| s.ident == "chrono") {
                self.0 = true;
            }
            syn::visit::visit_path(self, node);
        }
        fn visit_macro(&mut self, node: &'ast syn::Macro) {
            if node.tokens.to_string().contains("chrono") {
                self.0 = true;
            }
        }
    }
    let mut finder = Finder(false);
    for item in items {
        syn::visit::Visit::visit_item(&mut finder, item);
        if finder.0 {
            return true;
        }
    }
    false
}

fn simple_pat_ident(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pi) => Some(pi.ident.to_string()),
        syn::Pat::Reference(r) => simple_pat_ident(&r.pat),
        _ => None,
    }
}

fn is_simple_ident_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Path(ep) if ep.qself.is_none() && ep.path.segments.len() == 1)
}

fn is_int_lit_zero(expr: &syn::Expr) -> bool {
    is_int_lit_u64(expr, 0)
}

fn is_int_lit_u64(expr: &syn::Expr, expected: u64) -> bool {
    let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Int(li),
        ..
    }) = expr
    else {
        return false;
    };
    li.base10_parse::<u64>().ok() == Some(expected)
}

fn scale_subsec_arg(rendered: &str, expr: &syn::Expr, scale: u32) -> String {
    if scale == 1 {
        return rendered.to_owned();
    }
    if let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Int(li),
        ..
    }) = expr
        && let Ok(val) = li.base10_parse::<u64>()
        && let Some(nanos) = val.checked_mul(u64::from(scale))
    {
        return nanos.to_string();
    }
    let scale_lit = match scale {
        1_000_000 => "1_000_000",
        1_000 => "1_000",
        _ => "1",
    };
    if is_simple_ident_expr(expr) {
        format!("{rendered} * {scale_lit}")
    } else {
        format!("({rendered}) * {scale_lit}")
    }
}

fn token_range(node: &impl ToTokens) -> Option<Range<usize>> {
    let mut iter = node.to_token_stream().into_iter();
    let first = iter.next()?;
    let start = first.span().byte_range().start;
    let end = iter.last().map_or_else(
        || first.span().byte_range().end,
        |t| t.span().byte_range().end,
    );
    (start <= end).then_some(start..end)
}

fn token_start(node: &impl ToTokens) -> usize {
    token_range(node).map_or(0, |r| r.start)
}

fn line_start_and_indent(source: &str, byte_offset: usize) -> (usize, &str) {
    let clamped = byte_offset.min(source.len());
    let line_start = source[..clamped].rfind('\n').map_or(0, |pos| pos + 1);
    let rest = &source[line_start..clamped];
    let indent_len = rest
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    (line_start, &source[line_start..line_start + indent_len])
}

fn skip_single_newline(source: &str, pos: usize) -> usize {
    let bytes = source.as_bytes();
    if bytes.get(pos) == Some(&b'\r') && bytes.get(pos + 1) == Some(&b'\n') {
        pos + 2
    } else if bytes.get(pos) == Some(&b'\n') {
        pos + 1
    } else {
        pos
    }
}

fn has_note_above(source: &str, line_start: usize, message: &str) -> bool {
    let mut end = line_start;
    while end > 0 {
        // Strip line ending of the preceding line.
        let content_end = if end >= 2 && &source[end - 2..end] == "\r\n" {
            end - 2
        } else if &source[end - 1..end] == "\n" {
            end - 1
        } else {
            end
        };
        let prev_start = source[..content_end].rfind('\n').map_or(0, |p| p + 1);
        let trimmed = source[prev_start..content_end].trim();
        if let Some(after) = trimmed.strip_prefix("//") {
            if let Some(note_body) = after.trim_start().strip_prefix("navree upgrade:")
                && note_body.trim() == message
            {
                return true;
            }
            end = prev_start;
        } else {
            break;
        }
    }
    false
}
