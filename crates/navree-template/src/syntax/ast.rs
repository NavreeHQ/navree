//! The syntax tree of a template.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::value::Value;

/// Where a node starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) line: u32,
    pub(crate) col: u32,
}

/// A parsed template: its statements and its blocks by name.
#[derive(Debug, Default)]
pub(crate) struct Ast {
    pub(crate) body: Vec<Stmt>,
    pub(crate) blocks: BTreeMap<String, Arc<BlockDef>>,
}

/// The body of `{% block name %}`.
#[derive(Debug)]
pub(crate) struct BlockDef {
    pub(crate) body: Vec<Stmt>,
    pub(crate) required: bool,
}

#[derive(Debug)]
pub(crate) struct Stmt {
    pub(crate) kind: StmtKind,
    pub(crate) span: Span,
}

#[derive(Debug)]
pub(crate) enum StmtKind {
    /// Template data, printed as it is.
    Raw(String),
    /// `{{ expr }}`.
    Emit(Expr),
    For(Box<ForLoop>),
    If(Box<IfCond>),
    With(Box<WithBlock>),
    Set(Box<(Assign, Expr)>),
    SetBlock(Box<SetBlock>),
    AutoEscape(Box<(Expr, Vec<Stmt>)>),
    FilterBlock(Box<(Expr, Vec<Stmt>)>),
    /// `{% block name %}`: renders the most derived body of the block.
    Block(String),
    Extends(Expr),
    Include {
        name: Expr,
        ignore_missing: bool,
    },
    Import(Box<(Expr, Assign)>),
    FromImport(Box<FromImport>),
    Macro(Arc<Macro>),
    CallBlock(Box<(Expr, Arc<Macro>)>),
    Continue,
    Break,
    Do(Expr),
}

/// An assignment target: a name, a dotted name (`ns.attr`), or several to
/// unpack into.
#[derive(Debug)]
pub(crate) enum Assign {
    Name(String),
    Attr(Box<Expr>, String),
    Unpack(Vec<Assign>, Span),
}

#[derive(Debug)]
pub(crate) struct ForLoop {
    pub(crate) target: Assign,
    pub(crate) iter: Expr,
    pub(crate) filter: Option<Expr>,
    pub(crate) body: Vec<Stmt>,
    pub(crate) else_body: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct IfCond {
    pub(crate) test: Expr,
    pub(crate) body: Vec<Stmt>,
    pub(crate) else_body: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct WithBlock {
    pub(crate) assignments: Vec<(Assign, Expr)>,
    pub(crate) body: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct SetBlock {
    pub(crate) target: Assign,
    pub(crate) filter: Option<Expr>,
    pub(crate) body: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct FromImport {
    pub(crate) template: Expr,
    pub(crate) names: Vec<(String, Option<String>)>,
}

/// A macro, or the body of a call block (named `caller`).
#[derive(Debug)]
pub(crate) struct Macro {
    pub(crate) name: String,
    pub(crate) args: Vec<String>,
    /// Defaults of the last arguments.
    pub(crate) defaults: Vec<Expr>,
    pub(crate) body: Vec<Stmt>,
    /// Names the body reads that it does not assign: captured when the
    /// macro is defined.
    pub(crate) closure: Vec<String>,
    /// Whether the body refers to `caller`.
    pub(crate) uses_caller: bool,
}

#[derive(Debug)]
pub(crate) struct Expr {
    pub(crate) kind: ExprKind,
    pub(crate) span: Span,
}

#[derive(Debug)]
pub(crate) enum ExprKind {
    Var(String),
    Const(Value),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    BinOp(BinOp, Box<Expr>, Box<Expr>),
    /// A chain of comparisons: `a < b <= c`.
    Compare(Box<Expr>, Vec<(BinOp, Expr)>),
    IfExpr {
        test: Box<Expr>,
        then: Box<Expr>,
        otherwise: Option<Box<Expr>>,
    },
    Filter {
        name: String,
        expr: Option<Box<Expr>>,
        args: Vec<CallArg>,
    },
    Test {
        name: String,
        expr: Box<Expr>,
        args: Vec<CallArg>,
    },
    GetAttr(Box<Expr>, String),
    GetItem(Box<Expr>, Box<Expr>),
    Slice {
        expr: Box<Expr>,
        start: Option<Box<Expr>>,
        stop: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
    Call(Box<Expr>, Vec<CallArg>),
    List(Vec<Expr>),
    Tuple(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    NotIn,
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Rem,
    Pow,
    Concat,
}

#[derive(Debug)]
pub(crate) enum CallArg {
    Pos(Expr),
    Kwarg(String, Expr),
    PosSplat(Expr),
    KwargSplat(Expr),
}
