//! The renderer: walks a template's syntax tree with a stack of scopes.
//!
//! Scoping follows Jinja's rules. A `for` loop, a block, a `with` and an
//! imported template each get a scope; `{% set %}` writes to the innermost
//! one. A macro sees the context it was rendered with, its arguments, and
//! a closure: the names its body reads, captured when it is defined, kept
//! current with later assignments in the scope that defined it.
//!
//! Every render counts its work against [`Limits`]: nesting, loop
//! iterations, operations and the size of everything it builds, so it ends
//! with an error instead of exhausting the stack, memory or time.
//!
//! How a value prints is settled by where it is written: the mode of the
//! template, or the `{% autoescape %}` tag, around that text. What lands in
//! another template's output (an include, a block, a layout) also prints at
//! least as strictly as the text it lands in. A macro's or capture's output
//! is a string marked with the mode it was made in, and printing it anywhere
//! else escapes it again.

use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::environment::{Compiled, Environment, Limits, UndefinedBehavior};
use crate::error::{Error, ErrorKind};
use crate::escape::{Mode, write_display, write_escaped};
use crate::output::{Budget, Output};
use crate::stack::StackBudget;
use crate::syntax::ast::{
    Assign, BinOp, BlockDef, CallArg, EscapeTag, Expr, ExprKind, ForLoop, FromImport, IfCond,
    Macro, SetBlock, Span, Stmt, StmtKind, WithBlock,
};
use crate::value::ops;
use crate::value::{Kwargs, Module, Repr, Safety, Shape, Value, ValueMap};

/// Renders `template` with `ctx`.
pub(crate) fn render(
    env: &Environment,
    template: &Arc<Compiled>,
    ctx: Value,
) -> Result<String, Error> {
    let budget = Budget::new(env.limits.max_operations, env.limits.max_allocated_bytes);
    let mut state = State {
        env,
        limits: env.limits,
        root: ctx.clone(),
        frames: vec![Frame::with_ctx(ctx)],
        closures: Vec::new(),
        escaping: Escaping {
            floor: Mode::None,
            lexical: template.mode,
        },
        template: template.clone(),
        blocks: blocks_of(template),
        loaded: Vec::new(),
        pending_parent: None,
        current_block: None,
        stack: StackBudget::enter(env.limits.max_stack_bytes),
        depth: 0,
        include_depth: 0,
        loop_iterations: 0,
        budget: budget.clone(),
    };
    let mut out = Output::new(
        env.limits.max_output_bytes,
        budget,
        template.mode.has_html(),
    );
    let body = template.clone();
    state.run_body(&body.ast.body, &mut out)?;
    Ok(out.into_string())
}

fn blocks_of(template: &Arc<Compiled>) -> BTreeMap<String, Vec<BlockEntry>> {
    template
        .ast
        .blocks
        .iter()
        .map(|(name, def)| {
            (
                name.clone(),
                vec![BlockEntry {
                    def: def.clone(),
                    template: template.clone(),
                }],
            )
        })
        .collect()
}

/// A body of a block and the template that defines it.
#[derive(Clone)]
struct BlockEntry {
    def: Arc<BlockDef>,
    template: Arc<Compiled>,
}

/// How values print at this point of a render.
#[derive(Clone, Copy)]
struct Escaping {
    /// What the text around this template's output asks for.
    floor: Mode,
    /// What the text being run says: its template's mode, or the
    /// `{% autoescape %}` tag around it.
    lexical: Mode,
}

impl Escaping {
    fn mode(self) -> Mode {
        self.floor.and(self.lexical)
    }
}

/// A scope.
#[derive(Default)]
struct Frame {
    locals: BTreeMap<String, Value>,
    /// The render's context, in the scopes at the bottom of the stack.
    ctx: Option<Value>,
    current_loop: Option<Arc<LoopObject>>,
    /// The closure macros defined in this scope share: assignments here
    /// also go there.
    closure: Option<usize>,
    /// The closure of the macro this scope is the body of.
    closure_context: Option<usize>,
}

impl Frame {
    fn with_ctx(ctx: Value) -> Frame {
        Frame {
            ctx: Some(ctx),
            ..Frame::default()
        }
    }
}

/// How a statement list ended.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Break,
    Continue,
}

/// A macro as a value: a declaration and what it captured.
pub(crate) struct MacroValue {
    decl: Arc<Macro>,
    template: Arc<Compiled>,
    closure: Option<usize>,
}

impl MacroValue {
    pub(crate) fn name(&self) -> &str {
        &self.decl.name
    }

    /// `name`, `arguments` and `caller`, what a template may read of a macro.
    pub(crate) fn attr(&self, key: &str) -> Option<Value> {
        match key {
            "name" => Some(Value::from(self.decl.name.as_str())),
            "arguments" => Some(Value::list(
                self.decl
                    .args
                    .iter()
                    .map(|arg| Value::from(arg.as_str()))
                    .collect(),
            )),
            "caller" => Some(Value::from(self.decl.uses_caller)),
            _ => None,
        }
    }
}

/// The `loop` variable.
pub(crate) struct LoopObject {
    items: Arc<Vec<Value>>,
    index: AtomicUsize,
    last_changed: Mutex<Option<Vec<Value>>>,
    shape: Shape,
}

impl LoopObject {
    fn new(items: Arc<Vec<Value>>, shape: Shape) -> LoopObject {
        LoopObject {
            items,
            index: AtomicUsize::new(usize::MAX),
            last_changed: Mutex::new(None),
            shape,
        }
    }

    pub(crate) fn shape(&self) -> Shape {
        self.shape
    }

    pub(crate) fn attr(&self, key: &str) -> Option<Value> {
        let index = self.index.load(AtomicOrdering::Relaxed);
        if index == usize::MAX {
            return Some(Value::UNDEFINED);
        }
        let len = self.items.len();
        Some(match key {
            "index" => Value::from(index.saturating_add(1)),
            "index0" => Value::from(index),
            "length" => Value::from(len),
            "revindex" => Value::from(len.saturating_sub(index)),
            "revindex0" => Value::from(len.saturating_sub(index).saturating_sub(1)),
            "first" => Value::from(index == 0),
            "last" => Value::from(len == 0 || index == len - 1),
            "depth" => Value::from(1),
            "depth0" => Value::from(0),
            "previtem" => index
                .checked_sub(1)
                .and_then(|previous| self.items.get(previous).cloned())
                .unwrap_or_default(),
            "nextitem" => self.items.get(index + 1).cloned().unwrap_or_default(),
            _ => return None,
        })
    }

    pub(crate) fn render(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<loop {}/{}>",
            self.index.load(AtomicOrdering::Relaxed),
            self.items.len()
        )
    }

    fn call_method(&self, state: &State<'_>, name: &str, args: &[Value]) -> Result<Value, Error> {
        match name {
            "changed" => {
                // Comparing with the last arguments walks them.
                state.charge_items(args.len())?;
                state.charge(Value::iterable_shape(args).weight)?;
                let mut last = self
                    .last_changed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let changed = last.as_deref() != Some(args);
                if changed {
                    *last = Some(args.to_vec());
                }
                Ok(Value::from(changed))
            }
            "cycle" => {
                if args.iter().any(Value::is_kwargs) {
                    return Err(Error::invalid("loop.cycle takes no keyword arguments"));
                }
                let index = self.index.load(AtomicOrdering::Relaxed);
                Ok(match index.checked_rem(args.len()) {
                    Some(at) => args.get(at).cloned().unwrap_or_default(),
                    None => Value::UNDEFINED,
                })
            }
            _ => Err(unknown_method(ValueKindName::Loop, name)),
        }
    }
}

enum ValueKindName<'a> {
    Loop,
    Value(&'a Value),
}

fn unknown_method(owner: ValueKindName<'_>, name: &str) -> Error {
    let owner = match owner {
        ValueKindName::Loop => "loop".to_owned(),
        ValueKindName::Value(value) => value.kind().to_string(),
    };
    Error::new(
        ErrorKind::UnknownMethod,
        format!("{owner} has no method named {name}"),
    )
}

/// The state of one render.
pub(crate) struct State<'env> {
    env: &'env Environment,
    limits: Limits,
    root: Value,
    frames: Vec<Frame>,
    closures: Vec<BTreeMap<String, Value>>,
    escaping: Escaping,
    template: Arc<Compiled>,
    blocks: BTreeMap<String, Vec<BlockEntry>>,
    loaded: Vec<Arc<str>>,
    pending_parent: Option<Arc<Compiled>>,
    /// The block being rendered and which of its bodies, for `super()`.
    current_block: Option<(String, usize)>,
    stack: StackBudget,
    depth: usize,
    include_depth: usize,
    loop_iterations: u64,
    budget: Rc<Budget>,
}

/// What a string held as a value costs beyond its bytes.
const STRING_OVERHEAD: u64 = 32;

/// The most text one item of a container prints as, beyond its strings' bytes,
/// each of which prints as at most 4: a number, or the punctuation around it.
const PRINTED_ITEM_BYTES: u64 = 48 + 4 * 16;

impl State<'_> {
    pub(crate) fn env(&self) -> &Environment {
        self.env
    }

    /// How a value prints here.
    pub(crate) fn mode(&self) -> Mode {
        self.escaping.mode()
    }

    pub(crate) fn max_output_bytes(&self) -> usize {
        self.limits.max_output_bytes
    }

    /// The escaping `|escape` uses: the current one, else the template's,
    /// else HTML.
    pub(crate) fn escape_mode(&self) -> Mode {
        match (self.mode(), self.template.mode) {
            (Mode::None, Mode::None) => Mode::Html,
            (Mode::None, own) => own,
            (mode, _) => mode,
        }
    }

    pub(crate) fn undefined_behavior(&self) -> UndefinedBehavior {
        self.env.undefined
    }

    /// Whether undefined values are errors when used.
    pub(crate) fn strict_undefined(&self) -> bool {
        matches!(
            self.env.undefined,
            UndefinedBehavior::SemiStrict | UndefinedBehavior::Strict
        )
    }

    /// Fails on an undefined value where one is an error.
    pub(crate) fn assert_not_undefined(&self, value: &Value) -> Result<(), Error> {
        if self.strict_undefined() && value.is_undefined() && !value.is_silent_undefined() {
            return Err(Error::undefined());
        }
        Ok(())
    }

    /// A condition's truth: testing an undefined value is an error only
    /// under the strict behavior.
    fn is_true(&self, value: &Value) -> Result<bool, Error> {
        if self.env.undefined == UndefinedBehavior::Strict
            && value.is_undefined()
            && !value.is_silent_undefined()
        {
            return Err(Error::undefined());
        }
        Ok(value.is_true())
    }

    /// Charges work against the budget.
    pub(crate) fn charge(&self, operations: u64) -> Result<(), Error> {
        self.budget.charge(operations)
    }

    /// Charges a scan of `bytes` of text.
    pub(crate) fn charge_scan(&self, bytes: usize) -> Result<(), Error> {
        self.charge((bytes / 64) as u64)
    }

    /// The most items a sequence a template builds may hold.
    pub(crate) fn max_items(&self) -> usize {
        self.limits.max_output_bytes / 16
    }

    /// Fails when a value a template built nests too deeply or is too much
    /// to walk.
    pub(crate) fn check_shape(&self, value: Value) -> Result<Value, Error> {
        if matches!(value.0, Repr::Seq(..) | Repr::Map(..))
            && value.shape().weight > self.max_items() as u64
        {
            return Err(Error::limit("a value holds too many items"));
        }
        value.check_depth()
    }

    /// Charges bytes about to be allocated.
    pub(crate) fn charge_allocation(&self, bytes: u64) -> Result<(), Error> {
        self.budget.charge_allocation(bytes)
    }

    fn check_string_size(&self, bytes: usize) -> Result<(), Error> {
        if bytes > self.limits.max_output_bytes {
            return Err(Error::limit("a string is too large"));
        }
        Ok(())
    }

    /// Charges one copy of `bytes` of text about to be made, and caps it.
    /// Copying is work too, in cache lines.
    pub(crate) fn charge_copy(&self, bytes: usize) -> Result<(), Error> {
        self.check_string_size(bytes)?;
        self.charge_allocation(bytes as u64)?;
        self.charge_scan(bytes)
    }

    /// Charges a string about to be built at its full size and then copied
    /// into a value.
    pub(crate) fn check_size(&self, bytes: usize) -> Result<(), Error> {
        self.charge_copy(bytes)?;
        self.charge_allocation(bytes as u64)
    }

    /// Charges a string about to be built from `input` bytes by something
    /// that grows its buffer as it goes (case mapping), ending at `output`.
    pub(crate) fn check_growing_size(&self, input: usize, output: usize) -> Result<(), Error> {
        self.check_size(output)?;
        self.charge_allocation(input.max(output).saturating_mul(2) as u64)
    }

    /// Charges a sequence of `items` values the render builds.
    pub(crate) fn charge_items(&self, items: usize) -> Result<(), Error> {
        let count = items as u64;
        self.charge(count)?;
        self.charge_allocation(count.saturating_mul(std::mem::size_of::<Value>() as u64))
    }

    /// Charges `count` strings of `bytes` in all, each its own value.
    pub(crate) fn charge_strings(&self, count: usize, bytes: usize) -> Result<(), Error> {
        self.charge_scan(bytes)?;
        self.charge(count as u64)?;
        self.charge_allocation(
            (count as u64)
                .saturating_mul(STRING_OVERHEAD)
                .saturating_add(bytes as u64),
        )
    }

    /// Charges for the arguments of a filter or test that takes text: a value
    /// that is not a string converts to text before the function runs, and
    /// this charges the most that can come to.
    pub(crate) fn charge_text_args(&self, values: &[Value]) -> Result<(), Error> {
        for value in values {
            match &value.0 {
                Repr::Str(..) => {}
                Repr::Kwargs(..) => {
                    if let Some(map) = value.as_map() {
                        for (_, item) in map.iter() {
                            self.charge_printed(item)?;
                        }
                    }
                }
                _ => self.charge_printed(value)?,
            }
        }
        Ok(())
    }

    fn charge_printed(&self, value: &Value) -> Result<(), Error> {
        let weight = value.shape().weight;
        self.charge(weight)?;
        self.charge_allocation(weight.saturating_mul(PRINTED_ITEM_BYTES))
    }

    /// The items iterating over `value` gives, charged before they are made.
    pub(crate) fn iter_items(&self, value: &Value) -> Result<Vec<Value>, Error> {
        if let Repr::Str(text, _) = &value.0 {
            self.charge_scan(text.len())?;
            let count = text.chars().count();
            self.charge_strings(count, text.len())?;
            self.charge_items(count)?;
        } else if let Some(count) = value.iter_len() {
            self.charge_items(count)?;
        }
        value.iter_items()
    }

    /// An empty buffer for a string, holding `capacity` bytes without growing.
    pub(crate) fn string_builder(&self, capacity: usize) -> Result<Output, Error> {
        let mut out = Output::for_string(self.limits.max_output_bytes, self.budget.clone());
        out.reserve(capacity)?;
        Ok(out)
    }

    /// A buffer for what the render captures in the current mode.
    fn capture_buffer(&self) -> Output {
        Output::new(
            self.limits.max_output_bytes,
            self.budget.clone(),
            self.mode().has_html(),
        )
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            if let Some(value) = frame.locals.get(name) {
                return Some(value.clone());
            }
            if name == "loop"
                && let Some(current) = &frame.current_loop
            {
                return Some(Value(Repr::Loop(current.clone())));
            }
            if let Some(closure) = frame.closure_context
                && let Some(value) = self.closures.get(closure).and_then(|c| c.get(name))
            {
                return Some(value.clone());
            }
            if let Some(ctx) = &frame.ctx
                && let Some(value) = ctx.get_attr_fast(name)
            {
                return Some(value);
            }
        }
        self.env.globals.get(name).cloned()
    }

    fn store(&mut self, name: &str, value: Value) {
        let Some(top) = self.frames.last_mut() else {
            return;
        };
        if let Some(closure) = top.closure
            && let Some(map) = self.closures.get_mut(closure)
        {
            map.insert(name.to_owned(), value.clone());
        }
        top.locals.insert(name.to_owned(), value);
    }

    /// Fails when the render has used up its share of the stack.
    #[inline]
    fn check_stack(&self) -> Result<(), Error> {
        if self.stack.exceeded() {
            return Err(Error::limit("the template nests too deeply"));
        }
        Ok(())
    }

    fn enter_depth(&mut self) -> Result<(), Error> {
        if self.depth >= self.limits.max_depth {
            return Err(Error::limit(format!(
                "macro calls, blocks and includes nest deeper than {}",
                self.limits.max_depth
            )));
        }
        self.depth += 1;
        Ok(())
    }

    fn leave_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn locate(&self, mut error: Error, span: Span) -> Error {
        error.locate(&self.template.name, span.line as usize, span.col as usize);
        error
    }

    // Statements -------------------------------------------------------

    /// Runs a template's (or included template's) statements, then those
    /// of the parents it extends.
    fn run_body(&mut self, body: &[Stmt], out: &mut Output) -> Result<Flow, Error> {
        let flow = self.run(body, out)?;
        while let Some(parent) = self.pending_parent.take() {
            out.end_discarding();
            // The layout prints in its own mode, and no less strictly than
            // the template that extends it.
            self.escaping = Escaping {
                floor: self.mode(),
                lexical: parent.mode,
            };
            if self.mode() == Mode::Html {
                out.mark_html();
            }
            self.template = parent.clone();
            self.run(&parent.ast.body, out)?;
        }
        Ok(flow)
    }

    fn run(&mut self, stmts: &[Stmt], out: &mut Output) -> Result<Flow, Error> {
        for stmt in stmts {
            match self.run_stmt(stmt, out) {
                Ok(Flow::Normal) => {}
                Ok(flow) => return Ok(flow),
                Err(error) => return Err(self.locate(error, stmt.span)),
            }
        }
        Ok(Flow::Normal)
    }

    fn run_stmt(&mut self, stmt: &Stmt, out: &mut Output) -> Result<Flow, Error> {
        self.charge(1)?;
        self.check_stack()?;
        match &stmt.kind {
            StmtKind::Raw(text) => out.write_str(text)?,
            StmtKind::Emit(expr) => self.emit(expr, out)?,
            StmtKind::For(for_loop) => return self.run_for(for_loop, out),
            StmtKind::If(cond) => return self.run_if(cond, out),
            StmtKind::With(with) => return self.run_with(with, out),
            StmtKind::Set(set) => self.run_set(&set.0, &set.1)?,
            StmtKind::SetBlock(set) => self.run_set_block(set)?,
            StmtKind::AutoEscape(inner) => return self.run_auto_escape(inner.0, &inner.1, out),
            StmtKind::FilterBlock(inner) => self.run_filter_block(&inner.0, &inner.1, out)?,
            StmtKind::Block(name) => {
                if self.pending_parent.is_none() && !out.is_discarding() {
                    self.call_block(name, out)?;
                }
            }
            StmtKind::Extends(name) => self.run_extends(name, out)?,
            StmtKind::Include {
                name,
                ignore_missing,
            } => self.run_include(name, *ignore_missing, out)?,
            StmtKind::Import(import) => self.run_import(&import.0, &import.1)?,
            StmtKind::FromImport(from) => self.run_from_import(from)?,
            StmtKind::Macro(decl) => {
                let value = self.define_macro(decl)?;
                self.store(&decl.name, value);
            }
            StmtKind::CallBlock(call) => self.run_call_block(&call.0, &call.1, out)?,
            StmtKind::Continue => return Ok(Flow::Continue),
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Do(expr) => self.run_do(expr)?,
        }
        Ok(Flow::Normal)
    }

    fn run_set(&mut self, target: &Assign, expr: &Expr) -> Result<(), Error> {
        let value = self.eval(expr)?;
        self.assign(target, value)
    }

    fn run_include(
        &mut self,
        name: &Expr,
        ignore_missing: bool,
        out: &mut Output,
    ) -> Result<(), Error> {
        let name = self.eval(name)?;
        self.include(&name, ignore_missing, out)
    }

    fn run_call_block(
        &mut self,
        call: &Expr,
        caller: &Arc<Macro>,
        out: &mut Output,
    ) -> Result<(), Error> {
        let value = self.eval_call(call, Some(caller))?;
        self.write_value(&value, out)
    }

    fn run_do(&mut self, expr: &Expr) -> Result<(), Error> {
        self.eval_call(expr, None).map(|_| ())
    }

    fn emit(&mut self, expr: &Expr, out: &mut Output) -> Result<(), Error> {
        let value = self.eval(expr)?;
        self.write_value(&value, out)
            .map_err(|error| self.locate(error, expr.span))
    }

    fn write_value(&self, value: &Value, out: &mut Output) -> Result<(), Error> {
        self.assert_not_undefined(value)?;
        self.charge(value.shape().weight)?;
        write_escaped(out, self.mode(), value)
    }

    fn run_if(&mut self, cond: &IfCond, out: &mut Output) -> Result<Flow, Error> {
        let test = self.eval(&cond.test)?;
        if self
            .is_true(&test)
            .map_err(|e| self.locate(e, cond.test.span))?
        {
            self.run(&cond.body, out)
        } else {
            self.run(&cond.else_body, out)
        }
    }

    fn run_with(&mut self, with: &WithBlock, out: &mut Output) -> Result<Flow, Error> {
        self.frames.push(Frame::default());
        let frames = self.frames.len();
        for (target, expr) in &with.assignments {
            let value = self.eval(expr)?;
            self.assign(target, value)?;
        }
        let flow = self.run(&with.body, out)?;
        self.frames.truncate(frames - 1);
        Ok(flow)
    }

    /// Captures the output of `body`, as a string safe for the mode it ran in.
    fn capture(&mut self, body: &[Stmt]) -> Result<Value, Error> {
        let mut out = self.capture_buffer();
        self.run(body, &mut out)?;
        out.finish(self.mode().safety())
    }

    fn run_set_block(&mut self, set: &SetBlock) -> Result<(), Error> {
        let mut value = self.capture(&set.body)?;
        if let Some(filter) = &set.filter {
            value = self.apply_filter_chain(filter, value)?;
        }
        self.assign(&set.target, value)
    }

    fn run_filter_block(
        &mut self,
        filter: &Expr,
        body: &[Stmt],
        out: &mut Output,
    ) -> Result<(), Error> {
        let value = self.capture(body)?;
        let value = self.apply_filter_chain(filter, value)?;
        self.write_value(&value, out)
    }

    fn run_auto_escape(
        &mut self,
        tag: EscapeTag,
        body: &[Stmt],
        out: &mut Output,
    ) -> Result<Flow, Error> {
        let lexical = self.template.mode.under(tag);
        let saved = std::mem::replace(&mut self.escaping.lexical, lexical);
        let flow = self.run(body, out);
        self.escaping.lexical = saved;
        flow
    }

    fn run_for(&mut self, for_loop: &ForLoop, out: &mut Output) -> Result<Flow, Error> {
        let iterable = self.eval(&for_loop.iter)?;
        let items = self
            .loop_items(&iterable)
            .map_err(|error| self.locate(error, for_loop.iter.span))?;
        // A sequence's shape is known; working out another's goes through
        // items already charged.
        let (items, shape) = match (&for_loop.filter, &iterable.0) {
            (Some(filter), _) => {
                let passing = self.filter_items(for_loop, filter, &items)?;
                let shape = Value::iterable_shape(&passing);
                (passing, shape)
            }
            (None, Repr::Seq(..)) => (items, iterable.shape()),
            (None, _) => {
                let shape = Value::iterable_shape(&items);
                (items, shape)
            }
        };
        let object = Arc::new(LoopObject::new(items.clone(), shape));
        self.frames.push(Frame {
            current_loop: Some(object.clone()),
            ..Frame::default()
        });
        let frames = self.frames.len();
        // The `else` body runs when the loop gave no item, or was left on
        // its first one.
        let mut run_else = items.is_empty();
        for (index, item) in items.iter().enumerate() {
            self.count_iteration()?;
            object.index.store(index, AtomicOrdering::Relaxed);
            self.assign(&for_loop.target, item.clone())?;
            match self.run(&for_loop.body, out)? {
                Flow::Break => {
                    run_else = index == 0;
                    break;
                }
                Flow::Normal | Flow::Continue => {}
            }
        }
        self.frames.truncate(frames - 1);
        if run_else {
            return self.run(&for_loop.else_body, out);
        }
        Ok(Flow::Normal)
    }

    /// What a `for` loop goes through.
    fn loop_items(&self, iterable: &Value) -> Result<Arc<Vec<Value>>, Error> {
        self.assert_not_undefined(iterable)?;
        if let Repr::Seq(items, ..) = &iterable.0 {
            return Ok(items.clone());
        }
        Ok(Arc::new(self.iter_items(iterable)?))
    }

    /// The items passing a loop's `if` filter, tested in a scope of their
    /// own without `loop`.
    fn filter_items(
        &mut self,
        for_loop: &ForLoop,
        filter: &Expr,
        items: &[Value],
    ) -> Result<Arc<Vec<Value>>, Error> {
        self.frames.push(Frame::default());
        let frames = self.frames.len();
        let mut passing = Vec::new();
        for item in items {
            self.count_iteration()?;
            self.assign(&for_loop.target, item.clone())?;
            let test = self.eval(filter)?;
            if self.is_true(&test)? {
                passing.push(item.clone());
            }
        }
        self.charge_items(passing.len())?;
        self.frames.truncate(frames - 1);
        Ok(Arc::new(passing))
    }

    fn count_iteration(&mut self) -> Result<(), Error> {
        self.loop_iterations += 1;
        if self.loop_iterations > self.limits.max_loop_iterations {
            return Err(Error::limit(format!(
                "loops ran more than {} iterations",
                self.limits.max_loop_iterations
            )));
        }
        self.charge(1)
    }

    fn assign(&mut self, target: &Assign, value: Value) -> Result<(), Error> {
        match target {
            Assign::Name(name) => {
                self.store(name, value);
                Ok(())
            }
            Assign::Attr(base, _) => {
                let base = self.eval(base)?;
                Err(Error::invalid(format!(
                    "can only assign to namespaces, not {}",
                    base.kind()
                )))
            }
            Assign::Unpack(targets, span) => {
                let count = match &value.0 {
                    Repr::Seq(..) | Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_) => {
                        value.iter_len().unwrap_or(0)
                    }
                    _ => {
                        return Err(self.locate(
                            Error::new(ErrorKind::CannotUnpack, "value is not iterable"),
                            *span,
                        ));
                    }
                };
                if count != targets.len() {
                    return Err(self.locate(
                        Error::new(
                            ErrorKind::CannotUnpack,
                            format!(
                                "sequence of wrong length (expected {}, got {count})",
                                targets.len(),
                            ),
                        ),
                        *span,
                    ));
                }
                for (target, item) in targets.iter().zip(value.iter_items()?) {
                    self.assign(target, item)?;
                }
                Ok(())
            }
        }
    }

    fn call_block(&mut self, name: &str, out: &mut Output) -> Result<(), Error> {
        let Some(stack) = self.blocks.get(name) else {
            return Err(Error::new(
                ErrorKind::UnknownBlock,
                format!("block '{name}' not found"),
            ));
        };
        let Some(entry) = stack.first().cloned() else {
            return Err(Error::new(
                ErrorKind::UnknownBlock,
                format!("block '{name}' not found"),
            ));
        };
        if stack.len() == 1 && entry.def.required {
            return Err(Error::invalid(format!("Required block '{name}' not found")));
        }
        self.run_block_entry(name, 0, &entry, out)
    }

    fn run_block_entry(
        &mut self,
        name: &str,
        index: usize,
        entry: &BlockEntry,
        out: &mut Output,
    ) -> Result<(), Error> {
        self.enter_depth()?;
        let frames = self.frames.len();
        let saved_block = self.current_block.replace((name.to_owned(), index));
        let saved = std::mem::replace(&mut self.template, entry.template.clone());
        self.frames.push(Frame::default());
        // A block prints as it was written, and no less strictly than the
        // text it lands in.
        let saved_escaping = self.escaping;
        self.escaping = Escaping {
            floor: self.mode(),
            lexical: entry.template.mode.under(entry.def.escape),
        };
        let result = self.run_body(&entry.def.body, out);
        self.escaping = saved_escaping;
        self.frames.truncate(frames);
        self.template = saved;
        self.current_block = saved_block;
        self.leave_depth();
        result.map(|_| ())
    }

    /// `super()`: renders the body of the current block that the current
    /// body replaced.
    fn call_super(&mut self, args: &[Value]) -> Result<Value, Error> {
        if !args.is_empty() {
            return Err(Error::invalid("super() takes no arguments"));
        }
        let Some((name, index)) = self.current_block.clone() else {
            return Err(Error::invalid("cannot super outside of block"));
        };
        let parent = self
            .blocks
            .get(&name)
            .and_then(|stack| stack.get(index + 1))
            .cloned();
        let Some(parent) = parent else {
            return Err(Error::invalid("no parent block exists"));
        };
        let mode = self
            .mode()
            .and(parent.template.mode.under(parent.def.escape));
        let mut out = self.capture_buffer();
        self.run_block_entry(&name, index + 1, &parent, &mut out)?;
        out.finish(mode.safety())
    }

    fn run_extends(&mut self, name: &Expr, out: &mut Output) -> Result<(), Error> {
        let value = self.eval(name)?;
        let Some(name) = value.as_str() else {
            return Err(Error::invalid("template name was not a string"));
        };
        if self.pending_parent.is_some() {
            return Err(Error::invalid(
                "tried to extend a second time in a template",
            ));
        }
        if self.loaded.iter().any(|loaded| &**loaded == name) {
            return Err(Error::invalid(format!(
                "cycle in template inheritance. {name:?} was referenced more than once"
            )));
        }
        if self.loaded.len() >= self.limits.max_include_depth {
            return Err(Error::limit(format!(
                "templates extend deeper than {}",
                self.limits.max_include_depth
            )));
        }
        let parent = self.env.lookup_template(name)?;
        self.loaded.push(parent.name.clone());
        for (block, def) in &parent.ast.blocks {
            self.blocks
                .entry(block.clone())
                .or_default()
                .push(BlockEntry {
                    def: def.clone(),
                    template: parent.clone(),
                });
        }
        self.pending_parent = Some(parent);
        out.begin_discarding();
        Ok(())
    }

    /// Renders the first of `name` (a name, or a list of names) that
    /// exists, in the current scope.
    fn include(
        &mut self,
        name: &Value,
        ignore_missing: bool,
        out: &mut Output,
    ) -> Result<(), Error> {
        let choices = match &name.0 {
            Repr::Seq(..) | Repr::Map(..) | Repr::Module(_) => self.iter_items(name)?,
            _ => vec![name.clone()],
        };
        let mut tried = Vec::new();
        for choice in choices {
            let Some(choice_name) = choice.as_str() else {
                return Err(Error::invalid("template name was not a string"));
            };
            match self.env.lookup_template(choice_name) {
                Ok(template) => return self.render_included(&template, out),
                Err(error) if error.kind() == ErrorKind::TemplateNotFound => tried.push(choice),
                Err(error) => return Err(error),
            }
        }
        if tried.is_empty() || ignore_missing {
            return Ok(());
        }
        Err(Error::new(
            ErrorKind::TemplateNotFound,
            if let [only] = tried.as_slice() {
                format!("tried to include non-existing template {only:?}")
            } else {
                format!(
                    "tried to include one of multiple templates, none of which existed {}",
                    Value::list(tried)
                )
            },
        ))
    }

    fn render_included(&mut self, template: &Arc<Compiled>, out: &mut Output) -> Result<(), Error> {
        if self.include_depth >= self.limits.max_include_depth {
            return Err(Error::limit(format!(
                "includes and imports nest deeper than {}",
                self.limits.max_include_depth
            )));
        }
        self.enter_depth()?;
        self.include_depth += 1;
        let frames = self.frames.len();
        let saved_template = std::mem::replace(&mut self.template, template.clone());
        // An include prints in its own mode, and no less strictly than the
        // text it lands in.
        let saved_escaping = self.escaping;
        self.escaping = Escaping {
            floor: self.mode(),
            lexical: template.mode,
        };
        if self.mode() == Mode::Html {
            out.mark_html();
        }
        self.charge_items(template.ast.blocks.len())?;
        let saved_blocks = std::mem::replace(&mut self.blocks, blocks_of(template));
        let saved_loaded = self.loaded.clone();
        let saved_parent = self.pending_parent.take();
        let saved_closure = self.frames.last_mut().and_then(|top| top.closure.take());

        let result = self.run_body(&template.ast.body, out);

        self.frames.truncate(frames);
        if let Some(top) = self.frames.last_mut() {
            top.closure = saved_closure;
        }
        self.pending_parent = saved_parent;
        self.loaded = saved_loaded;
        self.blocks = saved_blocks;
        self.escaping = saved_escaping;
        self.template = saved_template;
        self.include_depth -= 1;
        self.leave_depth();
        result.map(|_| ()).map_err(|error| {
            Error::new(
                ErrorKind::BadInclude,
                format!("error in \"{}\"", template.name),
            )
            .with_source(error)
        })
    }

    /// Runs a template for what it defines. Its output is not printed here: it
    /// keeps its own mode, and printing the module escapes it like any value.
    fn import_into(&mut self, name: &Value, out: &mut Output) -> Result<(), Error> {
        let saved = std::mem::replace(
            &mut self.escaping,
            Escaping {
                floor: Mode::None,
                lexical: Mode::None,
            },
        );
        let result = self.include(name, false, out);
        self.escaping = saved;
        result
    }

    fn run_import(&mut self, template: &Expr, target: &Assign) -> Result<(), Error> {
        let mut captured = Output::new(self.limits.max_output_bytes, self.budget.clone(), false);
        let frames = self.frames.len();
        self.frames.push(Frame::default());
        let name = self.eval(template)?;
        self.import_into(&name, &mut captured)?;
        let held = self.frames.last().map_or(0, |frame| frame.locals.len());
        self.charge_items(held.saturating_mul(3))?;
        let locals = self
            .frames
            .last()
            .map(|frame| frame.locals.clone())
            .unwrap_or_default();
        self.frames.truncate(frames);
        let values: ValueMap = locals
            .into_iter()
            .filter(|(name, _)| !name.starts_with('_'))
            .map(|(name, value)| (Value::from(name), value))
            .collect();
        let module = Value(Repr::Module(Arc::new(Module::new(
            values,
            captured.into_string(),
        ))));
        self.assign(target, module.check_depth()?)
    }

    fn run_from_import(&mut self, from: &FromImport) -> Result<(), Error> {
        let mut discarded = Output::discarding(self.limits.max_output_bytes, self.budget.clone());
        let frames = self.frames.len();
        self.frames.push(Frame::default());
        let name = self.eval(&from.template)?;
        self.import_into(&name, &mut discarded)?;
        let values: Vec<Value> = from
            .names
            .iter()
            .map(|(name, _)| self.lookup(name).unwrap_or_default())
            .collect();
        self.frames.truncate(frames);
        for ((name, alias), value) in from.names.iter().zip(values) {
            self.store(alias.as_deref().unwrap_or(name), value);
        }
        Ok(())
    }

    // Macros -----------------------------------------------------------

    /// A macro value capturing what its body reads.
    fn define_macro(&mut self, decl: &Arc<Macro>) -> Result<Value, Error> {
        // A closure outlives the scope that made it, for its imported macros.
        self.charge_items(decl.closure.len().saturating_add(1))?;
        if !decl.closure.is_empty() {
            let closure = match self.frames.last().and_then(|top| top.closure) {
                Some(closure) => closure,
                None => {
                    self.closures.push(BTreeMap::new());
                    let closure = self.closures.len() - 1;
                    if let Some(top) = self.frames.last_mut() {
                        top.closure = Some(closure);
                    }
                    closure
                }
            };
            for name in &decl.closure {
                let captured = self
                    .closures
                    .get(closure)
                    .is_some_and(|map| map.contains_key(name));
                if !captured {
                    let value = self.lookup(name).unwrap_or_default();
                    if let Some(map) = self.closures.get_mut(closure) {
                        map.insert(name.clone(), value);
                    }
                }
            }
        }
        let closure = self.frames.last().and_then(|top| top.closure);
        Ok(Value(Repr::Macro(Arc::new(MacroValue {
            decl: decl.clone(),
            template: self.template.clone(),
            closure,
        }))))
    }

    fn call_macro(&mut self, object: &MacroValue, args: &[Value]) -> Result<Value, Error> {
        let (values, caller) = prepare_macro_args(&object.decl, args)?;
        self.enter_depth()?;
        let mut frames = vec![
            Frame::with_ctx(self.root.clone()),
            Frame {
                closure_context: object.closure,
                ..Frame::default()
            },
        ];
        if let (Some(caller), Some(top)) = (caller, frames.last_mut()) {
            top.locals.insert("caller".to_owned(), caller);
        }
        let saved_frames = std::mem::replace(&mut self.frames, frames);
        let saved_template = std::mem::replace(&mut self.template, object.template.clone());
        let saved_parent = self.pending_parent.take();
        let saved_block = self.current_block.take();
        // A macro prints as it was written, into its own output: where that
        // output lands decides by the mark it carries.
        let saved_escaping = std::mem::replace(
            &mut self.escaping,
            Escaping {
                floor: Mode::None,
                lexical: object.template.mode.under(object.decl.escape),
            },
        );
        let safety = self.mode().safety();
        let mut out = self.capture_buffer();
        let result = self.run_macro_body(&object.decl, values, &mut out);
        self.escaping = saved_escaping;
        self.current_block = saved_block;
        self.pending_parent = saved_parent;
        self.template = saved_template;
        self.frames = saved_frames;
        self.leave_depth();
        result?;
        out.finish(safety)
    }

    fn run_macro_body(
        &mut self,
        decl: &Macro,
        values: Vec<Value>,
        out: &mut Output,
    ) -> Result<(), Error> {
        let first_default = decl.args.len().saturating_sub(decl.defaults.len());
        // Later arguments first: a default sees the arguments after it.
        for (index, (name, value)) in decl.args.iter().zip(values).enumerate().rev() {
            let value = match index
                .checked_sub(first_default)
                .and_then(|at| decl.defaults.get(at))
            {
                Some(default) if value.is_undefined() => self.eval(default)?,
                _ => value,
            };
            self.store(name, value);
        }
        self.run_body(&decl.body, out).map(|_| ())
    }

    // Expressions ------------------------------------------------------

    pub(crate) fn eval(&mut self, expr: &Expr) -> Result<Value, Error> {
        self.charge(1)?;
        self.check_stack()?;
        self.eval_inner(expr)
            .map_err(|error| self.locate(error, expr.span))
    }

    /// Dispatches on the expression. Each kind has its own function, so
    /// that a level of recursion here takes little stack in any build.
    fn eval_inner(&mut self, expr: &Expr) -> Result<Value, Error> {
        match &expr.kind {
            ExprKind::Var(name) => Ok(self.lookup(name).unwrap_or_default()),
            ExprKind::Const(value) => Ok(value.clone()),
            ExprKind::Not(inner) => self.eval_not(inner),
            ExprKind::Neg(inner) => self.eval_neg(inner),
            ExprKind::BinOp(op, left, right) => self.eval_binop(*op, left, right),
            ExprKind::Compare(first, rest) => self.eval_compare(first, rest),
            ExprKind::IfExpr {
                test,
                then,
                otherwise,
            } => self.eval_if(test, then, otherwise.as_deref()),
            ExprKind::Filter { name, expr, args } => self.eval_filter(name, expr.as_deref(), args),
            ExprKind::Test { name, expr, args } => self.eval_test(name, expr, args),
            ExprKind::GetAttr(inner, name) => self.eval_attr(inner, name),
            ExprKind::GetItem(inner, subscript) => self.eval_item(inner, subscript),
            ExprKind::Slice {
                expr,
                start,
                stop,
                step,
            } => self.eval_slice(expr, start.as_deref(), stop.as_deref(), step.as_deref()),
            ExprKind::Call(..) => self.eval_call(expr, None),
            ExprKind::List(items) => self.eval_list(items),
            ExprKind::Tuple(items) => self.eval_tuple(items),
            ExprKind::Map(pairs) => self.eval_map(pairs),
        }
    }

    fn eval_not(&mut self, inner: &Expr) -> Result<Value, Error> {
        let value = self.eval(inner)?;
        Ok(Value::from(!self.is_true(&value)?))
    }

    fn eval_neg(&mut self, inner: &Expr) -> Result<Value, Error> {
        let value = self.eval(inner)?;
        ops::neg(&value)
    }

    fn eval_if(
        &mut self,
        test: &Expr,
        then: &Expr,
        otherwise: Option<&Expr>,
    ) -> Result<Value, Error> {
        let condition = self.eval(test)?;
        if self.is_true(&condition)? {
            self.eval(then)
        } else if let Some(otherwise) = otherwise {
            self.eval(otherwise)
        } else {
            Ok(Value::SILENT_UNDEFINED)
        }
    }

    fn eval_filter(
        &mut self,
        name: &str,
        expr: Option<&Expr>,
        args: &[CallArg],
    ) -> Result<Value, Error> {
        let input = match expr {
            Some(inner) => self.eval(inner)?,
            None => Value::UNDEFINED,
        };
        self.apply_filter(name, input, args)
    }

    fn eval_attr(&mut self, inner: &Expr, name: &str) -> Result<Value, Error> {
        let value = self.eval(inner)?;
        match value.get_attr_fast(name) {
            Some(attr) => Ok(attr),
            None => self.undefined_lookup(&value),
        }
    }

    fn eval_item(&mut self, inner: &Expr, subscript: &Expr) -> Result<Value, Error> {
        let value = self.eval(inner)?;
        let key = self.eval(subscript)?;
        self.charge(ops::lookup_cost(&value, &key))?;
        match value.get_item_opt(&key) {
            Some(item) => Ok(item),
            None => self.undefined_lookup(&value),
        }
    }

    fn eval_list(&mut self, items: &[Expr]) -> Result<Value, Error> {
        let items = self.eval_items(items)?;
        self.check_shape(Value::list(items))
    }

    fn eval_tuple(&mut self, items: &[Expr]) -> Result<Value, Error> {
        let items = self.eval_items(items)?;
        self.check_shape(Value::tuple(items))
    }

    fn eval_map(&mut self, pairs: &[(Expr, Expr)]) -> Result<Value, Error> {
        let mut map = ValueMap::new();
        for (key, value) in pairs {
            let key = self.eval(key)?;
            let value = self.eval(value)?;
            // Inserting compares keys.
            self.charge(key.shape().weight)?;
            self.charge_items(3)?;
            map.insert(key, value);
        }
        self.check_shape(Value::map(map))
    }

    /// What looking something up that does not exist gives.
    fn undefined_lookup(&self, parent: &Value) -> Result<Value, Error> {
        if parent.is_undefined() && self.env.undefined != UndefinedBehavior::Chainable {
            return Err(Error::undefined());
        }
        Ok(Value::UNDEFINED)
    }

    fn eval_items(&mut self, items: &[Expr]) -> Result<Vec<Value>, Error> {
        self.charge_items(items.len())?;
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(self.eval(item)?);
        }
        Ok(values)
    }

    fn eval_binop(&mut self, op: BinOp, left: &Expr, right: &Expr) -> Result<Value, Error> {
        let lhs = self.eval(left)?;
        match op {
            BinOp::And => {
                if !self.is_true(&lhs)? {
                    return Ok(lhs);
                }
                return self.eval(right);
            }
            BinOp::Or => {
                if self.is_true(&lhs)? {
                    return Ok(lhs);
                }
                return self.eval(right);
            }
            _ => {}
        }
        let rhs = self.eval(right)?;
        self.apply_binop(op, &lhs, &rhs)
    }

    fn apply_binop(&self, op: BinOp, lhs: &Value, rhs: &Value) -> Result<Value, Error> {
        match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                Ok(Value::from(self.compare(op, lhs, rhs)?))
            }
            BinOp::In => Ok(Value::from(self.compare(BinOp::In, lhs, rhs)?)),
            BinOp::NotIn => Ok(Value::from(self.compare(BinOp::NotIn, lhs, rhs)?)),
            BinOp::Add => {
                self.charge_sum(lhs, rhs)?;
                let value = ops::add(lhs, rhs)?;
                self.check_shape(value)
            }
            BinOp::Sub => ops::sub(lhs, rhs),
            BinOp::Mul => {
                self.charge_repeat(lhs, rhs)?;
                let value = ops::mul(lhs, rhs, self.limits.max_output_bytes, self.max_items())?;
                self.check_shape(value)
            }
            BinOp::Div => ops::div(lhs, rhs),
            BinOp::FloorDiv => ops::int_div(lhs, rhs),
            BinOp::Rem => ops::rem(lhs, rhs),
            BinOp::Pow => ops::pow(lhs, rhs),
            BinOp::Concat => {
                self.assert_not_undefined(lhs)?;
                self.assert_not_undefined(rhs)?;
                let length = |value: &Value| value.as_str().map_or(0, str::len);
                let mut text = self.string_builder(
                    length(lhs)
                        .saturating_add(length(rhs))
                        .min(self.limits.max_output_bytes.saturating_add(1)),
                )?;
                for value in [lhs, rhs] {
                    if value.as_str().is_none() {
                        self.charge(value.shape().weight)?;
                    }
                    write_display(&mut text, value)?;
                }
                text.finish(Safety::Plain)
            }
            BinOp::And | BinOp::Or => Ok(Value::UNDEFINED),
        }
    }

    /// Charges for what `a + b` is about to build.
    fn charge_sum(&self, lhs: &Value, rhs: &Value) -> Result<(), Error> {
        match (&lhs.0, &rhs.0) {
            (Repr::Seq(a, ..), Repr::Seq(b, ..)) => {
                let total = a.len().saturating_add(b.len());
                if total > self.max_items() {
                    return Err(Error::limit("a sequence is too large"));
                }
                self.charge_items(total)
            }
            (Repr::Str(a, _), Repr::Str(b, _)) => self.check_size(a.len().saturating_add(b.len())),
            _ => Ok(()),
        }
    }

    /// Charges for what `a * b` is about to build, when it is within the caps
    /// the operator itself checks.
    fn charge_repeat(&self, lhs: &Value, rhs: &Value) -> Result<(), Error> {
        let (repeated, count) = match (&lhs.0, &rhs.0) {
            (Repr::Str(..) | Repr::Seq(..), _) => (lhs, rhs),
            (_, Repr::Str(..) | Repr::Seq(..)) => (rhs, lhs),
            _ => return Ok(()),
        };
        let Some(count) = count.as_usize() else {
            return Ok(());
        };
        match &repeated.0 {
            Repr::Str(text, _) => match text.len().checked_mul(count) {
                Some(bytes) if bytes <= self.limits.max_output_bytes => self.check_size(bytes),
                _ => Ok(()),
            },
            Repr::Seq(items, ..) => match items.len().checked_mul(count) {
                Some(total) if total <= self.max_items() => self.charge_items(total),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    fn compare(&self, op: BinOp, lhs: &Value, rhs: &Value) -> Result<bool, Error> {
        match op {
            BinOp::In | BinOp::NotIn => {
                self.assert_not_undefined(rhs)?;
                self.assert_not_undefined(lhs)?;
                self.charge(ops::contains_cost(rhs, lhs))?;
                if rhs.as_str().is_some() {
                    self.charge_text_args(std::slice::from_ref(lhs))?;
                }
                let found = ops::contains(rhs, lhs)?;
                Ok(if op == BinOp::In { found } else { !found })
            }
            _ => {
                self.assert_not_undefined(lhs)?;
                self.assert_not_undefined(rhs)?;
                self.charge(ops::compare_cost(lhs, rhs))?;
                Ok(match op {
                    BinOp::Eq => lhs == rhs,
                    BinOp::Ne => lhs != rhs,
                    BinOp::Lt => lhs < rhs,
                    BinOp::Lte => lhs <= rhs,
                    BinOp::Gt => lhs > rhs,
                    _ => lhs >= rhs,
                })
            }
        }
    }

    fn eval_compare(&mut self, first: &Expr, rest: &[(BinOp, Expr)]) -> Result<Value, Error> {
        let mut lhs = self.eval(first)?;
        for (op, right) in rest {
            let rhs = self.eval(right)?;
            if !self.compare(*op, &lhs, &rhs)? {
                return Ok(Value::from(false));
            }
            lhs = rhs;
        }
        Ok(Value::from(true))
    }

    fn eval_slice(
        &mut self,
        expr: &Expr,
        start: Option<&Expr>,
        stop: Option<&Expr>,
        step: Option<&Expr>,
    ) -> Result<Value, Error> {
        let value = self.eval(expr)?;
        let mut bound = |bound: Option<&Expr>| match bound {
            Some(bound) => self.eval(bound),
            None => Ok(Value::NONE),
        };
        let start = bound(start)?;
        let stop = bound(stop)?;
        let step = bound(step)?;
        if value.is_undefined() && self.env.undefined == UndefinedBehavior::Strict {
            return Err(Error::undefined());
        }
        // A slice copies at most what it slices; going backwards it holds an
        // index for each item, and for a string each character too.
        let backwards = step.as_i64().is_some_and(|step| step < 0);
        match &value.0 {
            Repr::Str(text, _) => {
                self.check_size(text.len())?;
                let extra = if backwards { 12 } else { 1 };
                self.charge_allocation((text.len() as u64).saturating_mul(extra))?;
            }
            _ => {
                let items = value.len().unwrap_or(0);
                self.charge_items(items)?;
                if backwards {
                    self.charge_allocation((items as u64).saturating_mul(8))?;
                }
            }
        }
        ops::slice(&value, &start, &stop, &step)
    }

    // Calls ------------------------------------------------------------

    /// Evaluates call arguments: positional values, then the keyword
    /// arguments (with the call block's `caller`) as one last value.
    fn eval_args(
        &mut self,
        args: &[CallArg],
        caller: Option<&Arc<Macro>>,
    ) -> Result<Vec<Value>, Error> {
        let mut values = Vec::with_capacity(args.len());
        let mut has_kwargs = caller.is_some();
        for arg in args {
            match arg {
                CallArg::Pos(expr) => values.push(self.eval(expr)?),
                CallArg::PosSplat(expr) => {
                    let splat = self.eval(expr)?;
                    values.extend(self.iter_items(&splat)?);
                }
                CallArg::Kwarg(..) | CallArg::KwargSplat(_) => has_kwargs = true,
            }
        }
        if has_kwargs {
            let mut kwargs = ValueMap::new();
            for arg in args {
                match arg {
                    CallArg::Kwarg(name, expr) => {
                        let value = self.eval(expr)?;
                        kwargs.insert(Value::from(name.as_str()), value);
                    }
                    CallArg::KwargSplat(expr) => {
                        let splat = self.eval(expr)?;
                        self.assert_not_undefined(&splat)?;
                        let Some(map) = splat.as_map() else {
                            return Err(Error::invalid(format!(
                                "attempted to apply keyword arguments from non map (got {})",
                                splat.kind()
                            )));
                        };
                        self.charge_items(map.len().saturating_mul(3))?;
                        kwargs.extend(map.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    CallArg::Pos(_) | CallArg::PosSplat(_) => {}
                }
            }
            if let Some(caller) = caller {
                let caller = self.define_macro(caller)?;
                kwargs.insert(Value::from("caller"), caller);
            }
            values.push(Value::kwargs(kwargs).check_depth()?);
        }
        Ok(values)
    }

    fn eval_call(&mut self, expr: &Expr, caller: Option<&Arc<Macro>>) -> Result<Value, Error> {
        let ExprKind::Call(callee, args) = &expr.kind else {
            return Err(Error::invalid("expected a call"));
        };
        let result = match &callee.kind {
            ExprKind::Var(name) if name == "super" => {
                let args = self.eval_args(args, caller)?;
                self.call_super(&args)
            }
            ExprKind::Var(name) => {
                let args = self.eval_args(args, caller)?;
                match self.lookup(name) {
                    Some(function) => self.call_value(&function, &args),
                    None => Err(Error::new(
                        ErrorKind::UnknownFunction,
                        format!("{name} is unknown"),
                    )),
                }
            }
            ExprKind::GetAttr(object, name) => {
                let object = self.eval(object)?;
                let args = self.eval_args(args, caller)?;
                self.call_method(&object, name, &args)
            }
            _ => {
                let function = self.eval(callee)?;
                let args = self.eval_args(args, caller)?;
                self.call_value(&function, &args)
            }
        };
        self.check_shape(result?)
    }

    pub(crate) fn call_value(&mut self, function: &Value, args: &[Value]) -> Result<Value, Error> {
        match &function.0 {
            Repr::Function(function) => (function.call)(self, args),
            Repr::Macro(object) => {
                let object = object.clone();
                self.call_macro(&object, args)
            }
            _ => Err(Error::invalid(format!(
                "value of type {} is not callable",
                function.kind()
            ))),
        }
    }

    fn call_method(&mut self, object: &Value, name: &str, args: &[Value]) -> Result<Value, Error> {
        if let Repr::Loop(current) = &object.0 {
            return current.call_method(self, name, args);
        }
        match &object.0 {
            Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_) | Repr::Macro(_) => {
                match object.get_attr_fast(name) {
                    Some(attr) => self.call_value(&attr, args),
                    None => Err(unknown_method(ValueKindName::Value(object), name)),
                }
            }
            _ => Err(unknown_method(ValueKindName::Value(object), name)),
        }
    }

    fn apply_filter(&mut self, name: &str, input: Value, args: &[CallArg]) -> Result<Value, Error> {
        let mut values = vec![input];
        values.extend(self.eval_args(args, None)?);
        let Some(filter) = self.env.filters.get(name).cloned() else {
            return Err(Error::new(
                ErrorKind::UnknownFilter,
                format!("filter {name} is unknown"),
            ));
        };
        let value = filter(self, &values)?;
        self.check_shape(value)
    }

    /// Applies a `{% filter %}` or `{% set x | filter %}` chain to `input`.
    fn apply_filter_chain(&mut self, chain: &Expr, input: Value) -> Result<Value, Error> {
        let ExprKind::Filter { name, expr, args } = &chain.kind else {
            return Err(Error::invalid("expected a filter"));
        };
        let input = match expr {
            Some(inner) => self.apply_filter_chain(inner, input)?,
            None => input,
        };
        self.apply_filter(name, input, args)
            .map_err(|error| self.locate(error, chain.span))
    }

    fn eval_test(&mut self, name: &str, expr: &Expr, args: &[CallArg]) -> Result<Value, Error> {
        let mut values = vec![self.eval(expr)?];
        values.extend(self.eval_args(args, None)?);
        let Some(test) = self.env.tests.get(name).cloned() else {
            return Err(Error::new(
                ErrorKind::UnknownTest,
                format!("test {name} is unknown"),
            ));
        };
        Ok(Value::from(test(self, &values)?.is_true()))
    }
}

/// Binds a macro call's arguments: a value (or undefined) for each
/// parameter, and `caller` when the body refers to it.
fn prepare_macro_args(decl: &Macro, args: &[Value]) -> Result<(Vec<Value>, Option<Value>), Error> {
    let (args, kwargs) = match args.split_last() {
        Some((last, rest)) => match Kwargs::extract(last) {
            Some(kwargs) => (rest, Some(kwargs)),
            None => (args, None),
        },
        None => (args, None),
    };
    if args.len() > decl.args.len() {
        return Err(Error::from(ErrorKind::TooManyArguments));
    }
    let mut used = Vec::new();
    let mut values = Vec::with_capacity(decl.args.len());
    for (index, name) in decl.args.iter().enumerate() {
        let keyword = kwargs
            .as_ref()
            .and_then(|kwargs| kwargs.values().get(&Value::from(name.as_str())));
        values.push(match (args.get(index), keyword) {
            (Some(_), Some(_)) => {
                return Err(Error::new(
                    ErrorKind::TooManyArguments,
                    format!("duplicate argument `{name}`"),
                ));
            }
            (Some(value), None) => value.clone(),
            (None, Some(value)) => {
                used.push(name.as_str());
                value.clone()
            }
            (None, None) => Value::UNDEFINED,
        });
    }
    let caller = decl.uses_caller.then(|| {
        used.push("caller");
        kwargs
            .as_ref()
            .and_then(|kwargs| kwargs.values().get(&Value::from("caller")).cloned())
            .unwrap_or_default()
    });
    if let Some(kwargs) = &kwargs {
        for name in kwargs.args() {
            if !used.contains(&name) {
                return Err(Error::new(
                    ErrorKind::TooManyArguments,
                    format!("unknown keyword argument `{name}`"),
                ));
            }
        }
    }
    Ok((values, caller))
}
