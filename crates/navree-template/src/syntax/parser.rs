//! The parser: tokens into a syntax tree.
//!
//! Nesting is capped: expressions, statements and their combination may
//! nest at most [`MAX_NESTING`] deep, so neither parsing nor rendering a
//! template can exhaust the stack.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use super::ast::{
    Assign, Ast, BinOp, BlockDef, CallArg, EscapeTag, Expr, ExprKind, ForLoop, FromImport, IfCond,
    Macro, SetBlock, Span, Stmt, StmtKind, WithBlock,
};
use super::lexer::{Lexer, Loc, Token, Whitespace};
use crate::error::{Error, ErrorKind};
use crate::stack::StackBudget;
use crate::value::{Value, ValueKind};

/// How deep expressions and statements may nest.
pub(crate) const MAX_NESTING: usize = 150;

const RESERVED_NAMES: [&str; 8] = [
    "true", "True", "false", "False", "none", "None", "loop", "self",
];

/// Parses `source`, named `name` in errors.
pub(crate) fn parse(
    source: &str,
    name: &str,
    ws: Whitespace,
    max_stack_bytes: usize,
) -> Result<Ast, Error> {
    let mut parser = Parser::new(source, name, ws, max_stack_bytes)?;
    let result = parser.subparse(&|_| false);
    match result {
        Ok(body) => Ok(Ast {
            body,
            blocks: parser.blocks,
        }),
        Err(error) => Err(parser.locate(error)),
    }
}

fn syntax_error(message: impl Into<std::borrow::Cow<'static, str>>) -> Error {
    Error::new(ErrorKind::SyntaxError, message)
}

fn unexpected(token: &Token<'_>, expected: &str) -> Error {
    syntax_error(format!("unexpected {token}, expected {expected}"))
}

fn unexpected_eof(expected: &str) -> Error {
    syntax_error(format!("unexpected end of input, expected {expected}"))
}

fn span(loc: Loc) -> Span {
    Span {
        line: loc.line,
        col: loc.col,
    }
}

struct Parser<'s> {
    lexer: Lexer<'s>,
    current: Option<(Token<'s>, Loc)>,
    last: Loc,
    blocks: BTreeMap<String, Arc<BlockDef>>,
    in_loop: bool,
    in_macro: bool,
    depth: usize,
    stack: StackBudget,
    /// The `{% autoescape %}` tag around what is being parsed.
    escape: EscapeTag,
}

impl<'s> Parser<'s> {
    fn new(
        source: &'s str,
        name: &'s str,
        ws: Whitespace,
        max_stack_bytes: usize,
    ) -> Result<Parser<'s>, Error> {
        let mut lexer = Lexer::new(source, name, ws);
        let current = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current,
            last: Loc { line: 1, col: 1 },
            blocks: BTreeMap::new(),
            in_loop: false,
            in_macro: false,
            depth: 0,
            stack: StackBudget::enter(max_stack_bytes),
            escape: EscapeTag::Inherit,
        })
    }

    /// Names the template and the last token in an error that has no
    /// location yet.
    fn locate(&self, mut error: Error) -> Error {
        error.locate(
            self.lexer.name(),
            self.last.line as usize,
            self.last.col as usize,
        );
        error
    }

    fn next(&mut self) -> Result<Option<(Token<'s>, Loc)>, Error> {
        let following = self.lexer.next_token()?;
        let token = std::mem::replace(&mut self.current, following);
        if let Some((_, loc)) = &token {
            self.last = *loc;
        }
        Ok(token)
    }

    fn peek(&self) -> Option<&Token<'s>> {
        self.current.as_ref().map(|(token, _)| token)
    }

    fn current_loc(&self) -> Loc {
        self.current.as_ref().map_or(self.last, |(_, loc)| *loc)
    }

    fn is(&self, check: impl Fn(&Token<'s>) -> bool) -> bool {
        self.peek().is_some_and(check)
    }

    fn is_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token::Ident(ident)) if *ident == name)
    }

    fn skip(&mut self, check: impl Fn(&Token<'s>) -> bool) -> Result<bool, Error> {
        if self.is(check) {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn skip_ident(&mut self, name: &str) -> Result<bool, Error> {
        if self.is_ident(name) {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect(
        &mut self,
        check: impl Fn(&Token<'s>) -> bool,
        expected: &str,
    ) -> Result<(Token<'s>, Loc), Error> {
        match self.next()? {
            Some((token, loc)) if check(&token) => Ok((token, loc)),
            Some((token, _)) => Err(unexpected(&token, expected)),
            None => Err(unexpected_eof(expected)),
        }
    }

    fn expect_ident(&mut self, expected: &str) -> Result<(&'s str, Loc), Error> {
        match self.next()? {
            Some((Token::Ident(name), loc)) => Ok((name, loc)),
            Some((token, _)) => Err(unexpected(&token, expected)),
            None => Err(unexpected_eof(expected)),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), Error> {
        match self.next()? {
            Some((Token::Ident(name), _)) if name == keyword => Ok(()),
            Some((token, _)) => Err(unexpected(&token, keyword)),
            None => Err(unexpected_eof(keyword)),
        }
    }

    fn expect_block_end(&mut self) -> Result<(), Error> {
        self.expect(|t| *t == Token::BlockEnd, "end of block")
            .map(|_| ())
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_NESTING || self.stack.exceeded() {
            return Err(syntax_error("template exceeds maximum recursion limits"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Counts one more level of a left-nested chain (`a + b + c`,
    /// `x|f|g`, `a.b.c`) against the nesting cap.
    fn deepen(&mut self, chain: &mut usize) -> Result<(), Error> {
        self.enter()?;
        *chain += 1;
        Ok(())
    }

    /// Releases the levels of a finished chain.
    fn release(&mut self, chain: usize) {
        self.depth -= chain;
    }

    // Statements -------------------------------------------------------

    fn subparse(&mut self, end: &dyn Fn(&Token<'s>) -> bool) -> Result<Vec<Stmt>, Error> {
        let mut body = Vec::new();
        while let Some((token, loc)) = self.next()? {
            match token {
                Token::TemplateData(raw) => body.push(Stmt {
                    kind: StmtKind::Raw(raw.to_owned()),
                    span: span(loc),
                }),
                Token::VariableStart => {
                    let expr = self.parse_expr()?;
                    body.push(Stmt {
                        kind: StmtKind::Emit(expr),
                        span: span(loc),
                    });
                    self.expect(|t| *t == Token::VariableEnd, "end of variable block")?;
                }
                Token::BlockStart => {
                    match self.peek() {
                        Some(token) if end(token) => return Ok(body),
                        Some(_) => {}
                        None => {
                            return Err(syntax_error("unexpected end of input, expected keyword"));
                        }
                    }
                    body.push(self.parse_stmt()?);
                    self.expect_block_end()?;
                }
                _ => return Err(syntax_error("unexpected token")),
            }
        }
        Ok(body)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Error> {
        self.enter()?;
        let result = self.parse_stmt_inner();
        self.leave();
        result
    }

    fn parse_stmt_inner(&mut self) -> Result<Stmt, Error> {
        let (token, loc) = match self.next()? {
            Some(next) => next,
            None => return Err(unexpected_eof("block keyword")),
        };
        let Token::Ident(keyword) = token else {
            return Err(syntax_error(format!("unknown {token}, expected statement")));
        };
        let kind = match keyword {
            "for" => StmtKind::For(Box::new(self.parse_for()?)),
            "if" => StmtKind::If(Box::new(self.parse_if()?)),
            "with" => StmtKind::With(Box::new(self.parse_with()?)),
            "set" => self.parse_set()?,
            "autoescape" => {
                let tag = self.parse_escape_tag()?;
                self.expect_block_end()?;
                let outer = std::mem::replace(&mut self.escape, tag);
                let body = self.subparse(&|t| matches!(t, Token::Ident("endautoescape")))?;
                self.escape = outer;
                self.next()?;
                StmtKind::AutoEscape(Box::new((tag, body)))
            }
            "filter" => {
                let filter = self.parse_filter_chain()?;
                self.expect_block_end()?;
                let body = self.subparse(&|t| matches!(t, Token::Ident("endfilter")))?;
                self.next()?;
                StmtKind::FilterBlock(Box::new((filter, body)))
            }
            "block" => self.parse_block()?,
            "extends" => StmtKind::Extends(self.parse_expr()?),
            "include" => self.parse_include()?,
            "import" => {
                let template = self.parse_expr()?;
                self.expect_keyword("as")?;
                let name = self.parse_assign_name(false)?;
                self.skip_context_marker()?;
                StmtKind::Import(Box::new((template, name)))
            }
            "from" => StmtKind::FromImport(Box::new(self.parse_from_import()?)),
            "macro" => StmtKind::Macro(Arc::new(self.parse_macro()?)),
            "call" => self.parse_call_block()?,
            "continue" => {
                if !self.in_loop {
                    return Err(syntax_error("'continue' must be placed inside a loop"));
                }
                StmtKind::Continue
            }
            "break" => {
                if !self.in_loop {
                    return Err(syntax_error("'break' must be placed inside a loop"));
                }
                StmtKind::Break
            }
            "do" => {
                let call = self.parse_expr()?;
                if !matches!(call.kind, ExprKind::Call(..)) {
                    return Err(syntax_error("expected call expression in call block"));
                }
                StmtKind::Do(call)
            }
            name => return Err(syntax_error(format!("unknown statement {name}"))),
        };
        Ok(Stmt {
            kind,
            span: span(loc),
        })
    }

    fn parse_for(&mut self) -> Result<ForLoop, Error> {
        let old_in_loop = std::mem::replace(&mut self.in_loop, true);
        let target = self.parse_assignment(false)?;
        self.expect_keyword("in")?;
        let iter = self.parse_expr_noif()?;
        let filter = if self.skip_ident("if")? {
            Some(self.parse_expr()?)
        } else {
            None
        };
        if self.is_ident("recursive") {
            return Err(syntax_error("recursive loops are not supported"));
        }
        self.expect_block_end()?;
        let body = self.subparse(&|t| matches!(t, Token::Ident("endfor" | "else")))?;
        let else_body = if self.skip_ident("else")? {
            self.expect_block_end()?;
            self.subparse(&|t| matches!(t, Token::Ident("endfor")))?
        } else {
            Vec::new()
        };
        self.next()?;
        self.in_loop = old_in_loop;
        Ok(ForLoop {
            target,
            iter,
            filter,
            body,
            else_body,
        })
    }

    fn parse_if(&mut self) -> Result<IfCond, Error> {
        let test = self.parse_expr_noif()?;
        self.expect_block_end()?;
        let body = self.subparse(&|t| matches!(t, Token::Ident("endif" | "else" | "elif")))?;
        let else_body = match self.next()? {
            Some((Token::Ident("else"), _)) => {
                self.expect_block_end()?;
                let body = self.subparse(&|t| matches!(t, Token::Ident("endif")))?;
                self.next()?;
                body
            }
            Some((Token::Ident("elif"), loc)) => {
                self.enter()?;
                let nested = self.parse_if();
                self.leave();
                vec![Stmt {
                    kind: StmtKind::If(Box::new(nested?)),
                    span: span(loc),
                }]
            }
            _ => Vec::new(),
        };
        Ok(IfCond {
            test,
            body,
            else_body,
        })
    }

    fn parse_with(&mut self) -> Result<WithBlock, Error> {
        let mut assignments = Vec::new();
        while !self.is(|t| *t == Token::BlockEnd) {
            if !assignments.is_empty() {
                self.expect(|t| *t == Token::Comma, "comma")?;
            }
            let target = if self.skip(|t| *t == Token::ParenOpen)? {
                let target = self.parse_assignment(false)?;
                self.expect(|t| *t == Token::ParenClose, "`)`")?;
                target
            } else {
                self.parse_assign_name(false)?
            };
            self.expect(|t| *t == Token::Assign, "assignment operator")?;
            let expr = self.parse_expr()?;
            assignments.push((target, expr));
        }
        self.expect_block_end()?;
        let body = self.subparse(&|t| matches!(t, Token::Ident("endwith")))?;
        self.next()?;
        Ok(WithBlock { assignments, body })
    }

    fn parse_set(&mut self) -> Result<StmtKind, Error> {
        let target = self.parse_assignment(true)?;
        if self.is(|t| matches!(t, Token::BlockEnd | Token::Pipe)) {
            let filter = if self.skip(|t| *t == Token::Pipe)? {
                Some(self.parse_filter_chain()?)
            } else {
                None
            };
            self.expect_block_end()?;
            let body = self.subparse(&|t| matches!(t, Token::Ident("endset")))?;
            self.next()?;
            return Ok(StmtKind::SetBlock(Box::new(SetBlock {
                target,
                filter,
                body,
            })));
        }
        self.expect(|t| *t == Token::Assign, "assignment operator")?;
        let first = self.parse_expr()?;
        let expr = if self.skip(|t| *t == Token::Comma)? {
            let loc = self.current_loc();
            let mut items = vec![first];
            while !self.is(|t| *t == Token::BlockEnd) {
                items.push(self.parse_expr()?);
                if !self.skip(|t| *t == Token::Comma)? {
                    break;
                }
            }
            Expr {
                kind: ExprKind::Tuple(items),
                span: span(loc),
            }
        } else {
            first
        };
        Ok(StmtKind::Set(Box::new((target, expr))))
    }

    fn parse_block(&mut self) -> Result<StmtKind, Error> {
        if self.in_macro {
            return Err(syntax_error("block tags in macros are not allowed"));
        }
        let old_in_loop = std::mem::replace(&mut self.in_loop, false);
        let (name, _) = self.expect_ident("identifier")?;
        self.skip_ident("scoped")?;
        let required = self.skip_ident("required")?;
        if self.blocks.contains_key(name) {
            return Err(syntax_error(format!("block '{name}' defined twice")));
        }
        self.expect_block_end()?;
        let body = self.subparse(&|t| matches!(t, Token::Ident("endblock")))?;
        self.next()?;
        if required
            && !body
                .iter()
                .all(|stmt| matches!(&stmt.kind, StmtKind::Raw(raw) if raw.trim().is_empty()))
        {
            return Err(syntax_error(
                "Required blocks can only contain comments or whitespace",
            ));
        }
        if let Some(Token::Ident(trailing)) = self.peek() {
            if *trailing != name {
                return Err(syntax_error(format!(
                    "mismatching name on block. Got `{trailing}`, expected `{name}`"
                )));
            }
            self.next()?;
        }
        self.in_loop = old_in_loop;
        self.blocks.insert(
            name.to_owned(),
            Arc::new(BlockDef {
                body,
                required,
                escape: self.escape,
            }),
        );
        Ok(StmtKind::Block(name.to_owned()))
    }

    /// The argument of `{% autoescape %}`: settled here, so it can't depend
    /// on what a render is given.
    fn parse_escape_tag(&mut self) -> Result<EscapeTag, Error> {
        let argument = self.parse_expr()?;
        let tag = match &argument.kind {
            ExprKind::Const(value) => match (value.as_str(), value.kind()) {
                (Some("html"), _) => Some(EscapeTag::Html),
                (Some("json"), _) => Some(EscapeTag::Json),
                (Some("none"), _) => Some(EscapeTag::Off),
                (None, ValueKind::Bool) => Some(if value.is_true() {
                    EscapeTag::On
                } else {
                    EscapeTag::Off
                }),
                _ => None,
            },
            _ => None,
        };
        tag.ok_or_else(|| {
            syntax_error("autoescape takes true, false, \"html\", \"json\" or \"none\"")
        })
    }

    fn parse_include(&mut self) -> Result<StmtKind, Error> {
        let name = self.parse_expr()?;
        let skipped_context = self.skip_context_marker()?;
        let ignore_missing = if self.skip_ident("ignore")? {
            self.expect_keyword("missing")?;
            if !skipped_context {
                self.skip_context_marker()?;
            }
            true
        } else {
            false
        };
        Ok(StmtKind::Include {
            name,
            ignore_missing,
        })
    }

    fn parse_from_import(&mut self) -> Result<FromImport, Error> {
        let template = self.parse_expr()?;
        self.expect_keyword("import")?;
        let mut names = Vec::new();
        loop {
            if self.skip_context_marker()? || self.is(|t| *t == Token::BlockEnd) {
                break;
            }
            if !names.is_empty() {
                self.expect(|t| *t == Token::Comma, "`,`")?;
            }
            if self.skip_context_marker()? || self.is(|t| *t == Token::BlockEnd) {
                break;
            }
            let Assign::Name(name) = self.parse_assign_name(false)? else {
                return Err(syntax_error("expected a name to import"));
            };
            if name.starts_with('_') {
                return Err(syntax_error(
                    "names starting with an underline can not be imported",
                ));
            }
            let alias = if self.skip_ident("as")? {
                match self.parse_assign_name(false)? {
                    Assign::Name(alias) => Some(alias),
                    _ => return Err(syntax_error("expected a name to import as")),
                }
            } else {
                None
            };
            names.push((name, alias));
        }
        Ok(FromImport { template, names })
    }

    /// Skips `with context` / `without context`, which change nothing.
    fn skip_context_marker(&mut self) -> Result<bool, Error> {
        if self.is_ident("with") || self.is_ident("without") {
            self.next()?;
            self.expect_keyword("context")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn parse_macro_args(&mut self) -> Result<(Vec<String>, Vec<Expr>), Error> {
        let mut args = Vec::new();
        let mut defaults = Vec::new();
        loop {
            if self.skip(|t| *t == Token::ParenClose)? {
                break;
            }
            if !args.is_empty() {
                self.expect(|t| *t == Token::Comma, "`,`")?;
                if self.skip(|t| *t == Token::ParenClose)? {
                    break;
                }
            }
            let Assign::Name(name) = self.parse_assign_name(false)? else {
                return Err(syntax_error("expected an argument name"));
            };
            args.push(name);
            if self.skip(|t| *t == Token::Assign)? {
                defaults.push(self.parse_expr()?);
            } else if !defaults.is_empty() {
                self.expect(|t| *t == Token::Assign, "`=`")?;
            }
        }
        Ok((args, defaults))
    }

    fn parse_macro_body(
        &mut self,
        name: Option<&str>,
        args: Vec<String>,
        defaults: Vec<Expr>,
    ) -> Result<Macro, Error> {
        self.expect_block_end()?;
        let old_in_loop = std::mem::replace(&mut self.in_loop, false);
        let old_in_macro = std::mem::replace(&mut self.in_macro, true);
        let is_named = name.is_some();
        let body = self.subparse(&|t| match t {
            Token::Ident("endmacro") => is_named,
            Token::Ident("endcall") => !is_named,
            _ => false,
        })?;
        self.in_macro = old_in_macro;
        self.in_loop = old_in_loop;
        self.next()?;
        let mut decl = Macro {
            name: name.unwrap_or("caller").to_owned(),
            args,
            defaults,
            body,
            closure: Vec::new(),
            uses_caller: false,
            escape: self.escape,
        };
        let mut closure = closure::find(&decl);
        decl.uses_caller = closure.remove("caller");
        decl.closure = closure.into_iter().collect();
        Ok(decl)
    }

    fn parse_macro(&mut self) -> Result<Macro, Error> {
        let (name, _) = self.expect_ident("identifier")?;
        self.expect(|t| *t == Token::ParenOpen, "`(`")?;
        let (args, defaults) = self.parse_macro_args()?;
        self.parse_macro_body(Some(name), args, defaults)
    }

    fn parse_call_block(&mut self) -> Result<StmtKind, Error> {
        let (args, defaults) = if self.skip(|t| *t == Token::ParenOpen)? {
            self.parse_macro_args()?
        } else {
            (Vec::new(), Vec::new())
        };
        let call = self.parse_expr()?;
        if !matches!(call.kind, ExprKind::Call(..)) {
            return Err(syntax_error("expected call expression in call block"));
        }
        let caller = self.parse_macro_body(None, args, defaults)?;
        Ok(StmtKind::CallBlock(Box::new((call, Arc::new(caller)))))
    }

    fn parse_assign_name(&mut self, dotted: bool) -> Result<Assign, Error> {
        let (name, _) = self.expect_ident("identifier")?;
        if RESERVED_NAMES.contains(&name) {
            return Err(syntax_error(format!(
                "cannot assign to reserved variable name {name}"
            )));
        }
        let mut target = Assign::Name(name.to_owned());
        if dotted {
            let mut expr_loc = None;
            while self.skip(|t| *t == Token::Dot)? {
                let (attr, loc) = self.expect_ident("identifier")?;
                let base = match target {
                    Assign::Name(name) => Expr {
                        kind: ExprKind::Var(name),
                        span: span(*expr_loc.get_or_insert(loc)),
                    },
                    Assign::Attr(expr, prev) => Expr {
                        kind: ExprKind::GetAttr(expr, prev),
                        span: span(loc),
                    },
                    Assign::Unpack(..) => return Err(syntax_error("invalid assignment")),
                };
                target = Assign::Attr(Box::new(base), attr.to_owned());
            }
        }
        Ok(target)
    }

    fn parse_assignment(&mut self, dotted: bool) -> Result<Assign, Error> {
        let loc = self.current_loc();
        let mut items = Vec::new();
        let mut is_tuple = false;
        loop {
            if !items.is_empty() {
                self.expect(|t| *t == Token::Comma, "`,`")?;
            }
            if self.is(|t| {
                matches!(
                    t,
                    Token::ParenClose | Token::VariableEnd | Token::BlockEnd | Token::Ident("in")
                )
            }) {
                break;
            }
            items.push(if self.skip(|t| *t == Token::ParenOpen)? {
                self.enter()?;
                let target = self.parse_assignment(dotted);
                self.leave();
                let target = target?;
                self.expect(|t| *t == Token::ParenClose, "`)`")?;
                target
            } else {
                self.parse_assign_name(dotted)?
            });
            if self.is(|t| *t == Token::Comma) {
                is_tuple = true;
            } else {
                break;
            }
        }
        if !is_tuple && items.len() == 1 {
            Ok(items.remove(0))
        } else {
            Ok(Assign::Unpack(items, span(loc)))
        }
    }

    fn parse_filter_chain(&mut self) -> Result<Expr, Error> {
        let mut filter: Option<Expr> = None;
        let mut chain = 0;
        while !self.is(|t| *t == Token::BlockEnd) {
            self.deepen(&mut chain)?;
            if filter.is_some() {
                self.expect(|t| *t == Token::Pipe, "`|`")?;
            }
            let (name, loc) = self.parse_filter_test_name()?;
            let args = if self.is(|t| *t == Token::ParenOpen) {
                self.parse_args()?
            } else {
                Vec::new()
            };
            filter = Some(Expr {
                kind: ExprKind::Filter {
                    name,
                    expr: filter.map(Box::new),
                    args,
                },
                span: span(loc),
            });
        }
        self.release(chain);
        filter.ok_or_else(|| syntax_error("expected a filter"))
    }

    // Expressions -------------------------------------------------------

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.enter()?;
        let result = self.parse_ifexpr();
        self.leave();
        result
    }

    fn parse_expr_noif(&mut self) -> Result<Expr, Error> {
        self.parse_or()
    }

    fn parse_ifexpr(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_or()?;
        let mut chain = 0;
        while self.skip_ident("if")? {
            self.deepen(&mut chain)?;
            let test = self.parse_or()?;
            let otherwise = if self.skip_ident("else")? {
                self.enter()?;
                let otherwise = self.parse_ifexpr();
                self.leave();
                Some(Box::new(otherwise?))
            } else {
                None
            };
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::IfExpr {
                    test: Box::new(test),
                    then: Box::new(expr),
                    otherwise,
                },
                span,
            };
        }
        self.release(chain);
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_and()?;
        let mut chain = 0;
        while self.skip_ident("or")? {
            self.deepen(&mut chain)?;
            let right = self.parse_and()?;
            left = binop(BinOp::Or, left, right);
        }
        self.release(chain);
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_not()?;
        let mut chain = 0;
        while self.skip_ident("and")? {
            self.deepen(&mut chain)?;
            let right = self.parse_not()?;
            left = binop(BinOp::And, left, right);
        }
        self.release(chain);
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, Error> {
        let loc = self.current_loc();
        if self.skip_ident("not")? {
            self.enter()?;
            let operand = self.parse_not();
            self.leave();
            let operand = operand?;
            return Ok(Expr {
                span: Span {
                    line: operand.span.line,
                    col: loc.col,
                },
                kind: ExprKind::Not(Box::new(operand)),
            });
        }
        self.parse_compare()
    }

    fn parse_compare(&mut self) -> Result<Expr, Error> {
        let first = self.parse_math1()?;
        let mut ops = Vec::new();
        loop {
            let op = match self.peek() {
                Some(Token::Eq) => BinOp::Eq,
                Some(Token::Ne) => BinOp::Ne,
                Some(Token::Lt) => BinOp::Lt,
                Some(Token::Lte) => BinOp::Lte,
                Some(Token::Gt) => BinOp::Gt,
                Some(Token::Gte) => BinOp::Gte,
                Some(Token::Ident("in")) => BinOp::In,
                Some(Token::Ident("not")) => {
                    self.next()?;
                    self.expect_keyword("in")?;
                    BinOp::NotIn
                }
                _ => break,
            };
            if op != BinOp::NotIn {
                self.next()?;
            }
            ops.push((op, self.parse_math1()?));
        }
        if ops.len() > 1 {
            let span = first.span;
            return Ok(Expr {
                kind: ExprKind::Compare(Box::new(first), ops),
                span,
            });
        }
        Ok(match ops.pop() {
            None => first,
            Some((BinOp::NotIn, right)) => {
                let contains = binop(BinOp::In, first, right);
                Expr {
                    span: contains.span,
                    kind: ExprKind::Not(Box::new(contains)),
                }
            }
            Some((op, right)) => binop(op, first, right),
        })
    }

    fn parse_math1(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_concat()?;
        let mut chain = 0;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.next()?;
            self.deepen(&mut chain)?;
            let right = self.parse_concat()?;
            left = binop(op, left, right);
        }
        self.release(chain);
        Ok(left)
    }

    fn parse_concat(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_math2()?;
        let mut chain = 0;
        while self.skip(|t| *t == Token::Tilde)? {
            self.deepen(&mut chain)?;
            let right = self.parse_math2()?;
            left = binop(BinOp::Concat, left, right);
        }
        self.release(chain);
        Ok(left)
    }

    fn parse_math2(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_pow()?;
        let mut chain = 0;
        loop {
            let op = match self.peek() {
                Some(Token::Mul) => BinOp::Mul,
                Some(Token::Div) => BinOp::Div,
                Some(Token::FloorDiv) => BinOp::FloorDiv,
                Some(Token::Mod) => BinOp::Rem,
                _ => break,
            };
            self.next()?;
            self.deepen(&mut chain)?;
            let right = self.parse_pow()?;
            left = binop(op, left, right);
        }
        self.release(chain);
        Ok(left)
    }

    fn parse_pow(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_unary()?;
        let mut chain = 0;
        while self.skip(|t| *t == Token::Pow)? {
            self.deepen(&mut chain)?;
            let right = self.parse_unary()?;
            left = binop(BinOp::Pow, left, right);
        }
        self.release(chain);
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        let expr = self.parse_unary_only()?;
        let expr = self.parse_postfix(expr)?;
        self.parse_filter_expr(expr)
    }

    /// A primary expression with optional negation; the negation applies
    /// to the operand with its postfix operators (`-foo.bar` is
    /// `-(foo.bar)`).
    fn parse_unary_only(&mut self) -> Result<Expr, Error> {
        let loc = self.current_loc();
        if !self.is(|t| *t == Token::Minus) {
            return self.parse_primary();
        }
        self.next()?;
        self.enter()?;
        let operand = self.parse_unary_only().and_then(|e| self.parse_postfix(e));
        self.leave();
        let operand = operand?;
        Ok(Expr {
            span: Span {
                line: operand.span.line,
                col: loc.col,
            },
            kind: ExprKind::Neg(Box::new(operand)),
        })
    }

    fn parse_postfix(&mut self, mut expr: Expr) -> Result<Expr, Error> {
        let mut chain = 0;
        loop {
            let span = expr.span;
            if self.is(|t| matches!(t, Token::Dot | Token::BracketOpen | Token::ParenOpen)) {
                self.deepen(&mut chain)?;
            }
            match self.peek() {
                Some(Token::Dot) => {
                    self.next()?;
                    match self.next()? {
                        Some((Token::Ident(name), _)) => {
                            expr = Expr {
                                kind: ExprKind::GetAttr(Box::new(expr), name.to_owned()),
                                span,
                            };
                        }
                        Some((Token::Int(index), loc)) => {
                            expr = Expr {
                                kind: ExprKind::GetItem(
                                    Box::new(expr),
                                    Box::new(Expr {
                                        kind: ExprKind::Const(Value::from(index)),
                                        span: span_of(loc),
                                    }),
                                ),
                                span: Span {
                                    line: loc.line,
                                    col: span.col,
                                },
                            };
                        }
                        Some((token, _)) => {
                            return Err(unexpected(&token, "identifier or integer"));
                        }
                        None => return Err(unexpected_eof("identifier or integer")),
                    }
                }
                Some(Token::BracketOpen) => {
                    self.next()?;
                    let mut start = None;
                    let mut stop = None;
                    let mut step = None;
                    let mut is_slice = false;
                    if !self.is(|t| *t == Token::Colon) {
                        start = Some(Box::new(self.parse_expr()?));
                    }
                    if self.skip(|t| *t == Token::Colon)? {
                        is_slice = true;
                        if !self.is(|t| matches!(t, Token::BracketClose | Token::Colon)) {
                            stop = Some(Box::new(self.parse_expr()?));
                        }
                        if self.skip(|t| *t == Token::Colon)?
                            && !self.is(|t| *t == Token::BracketClose)
                        {
                            step = Some(Box::new(self.parse_expr()?));
                        }
                    }
                    self.expect(|t| *t == Token::BracketClose, "`]`")?;
                    expr = if is_slice {
                        Expr {
                            kind: ExprKind::Slice {
                                expr: Box::new(expr),
                                start,
                                stop,
                                step,
                            },
                            span,
                        }
                    } else {
                        let subscript = start.ok_or_else(|| syntax_error("empty subscript"))?;
                        let span = Span {
                            line: subscript.span.line,
                            col: span.col,
                        };
                        Expr {
                            kind: ExprKind::GetItem(Box::new(expr), subscript),
                            span,
                        }
                    };
                }
                Some(Token::ParenOpen) => {
                    let args = self.parse_args()?;
                    let span = Span {
                        line: last_line(&args, span.line),
                        col: span.col,
                    };
                    expr = Expr {
                        kind: ExprKind::Call(Box::new(expr), args),
                        span,
                    };
                }
                _ => break,
            }
        }
        self.release(chain);
        Ok(expr)
    }

    fn parse_filter_test_name(&mut self) -> Result<(String, Loc), Error> {
        let (first, loc) = self.expect_ident("identifier")?;
        let mut name = first.to_owned();
        while self.skip(|t| *t == Token::Dot)? {
            let (segment, _) = self.expect_ident("identifier")?;
            name.push('.');
            name.push_str(segment);
        }
        Ok((name, loc))
    }

    fn parse_filter_expr(&mut self, mut expr: Expr) -> Result<Expr, Error> {
        let mut chain = 0;
        loop {
            if self.is(|t| *t == Token::Pipe || matches!(t, Token::Ident("is"))) {
                self.deepen(&mut chain)?;
            }
            if self.skip(|t| *t == Token::Pipe)? {
                let (name, loc) = self.parse_filter_test_name()?;
                let args = if self.is(|t| *t == Token::ParenOpen) {
                    self.parse_args()?
                } else {
                    Vec::new()
                };
                let span = Span {
                    line: last_line(&args, expr.span.line),
                    col: loc.col,
                };
                expr = Expr {
                    kind: ExprKind::Filter {
                        name,
                        expr: Some(Box::new(expr)),
                        args,
                    },
                    span,
                };
            } else if self.skip_ident("is")? {
                let negated = self.skip_ident("not")?;
                let (name, loc) = self.parse_filter_test_name()?;
                let args = if self.is(|t| *t == Token::ParenOpen) {
                    self.parse_args()?
                } else if self.is(|t| {
                    matches!(
                        t,
                        Token::Ident(_)
                            | Token::Str(_)
                            | Token::Int(_)
                            | Token::Float(_)
                            | Token::Plus
                            | Token::Minus
                            | Token::BracketOpen
                            | Token::BraceOpen
                    ) && !matches!(t, Token::Ident("and" | "or" | "else" | "is"))
                }) {
                    let arg = self.parse_unary_only()?;
                    vec![CallArg::Pos(self.parse_postfix(arg)?)]
                } else {
                    Vec::new()
                };
                let span = Span {
                    line: last_line(&args, expr.span.line),
                    col: loc.col,
                };
                expr = Expr {
                    kind: ExprKind::Test {
                        name,
                        expr: Box::new(expr),
                        args,
                    },
                    span,
                };
                if negated {
                    expr = Expr {
                        kind: ExprKind::Not(Box::new(expr)),
                        span,
                    };
                }
            } else {
                break;
            }
        }
        self.release(chain);
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<CallArg>, Error> {
        let mut args = Vec::new();
        let mut has_kwargs = false;
        self.expect(|t| *t == Token::ParenOpen, "`(`")?;
        loop {
            if self.skip(|t| *t == Token::ParenClose)? {
                break;
            }
            if !args.is_empty() || has_kwargs {
                self.expect(|t| *t == Token::Comma, "`,`")?;
                if self.skip(|t| *t == Token::ParenClose)? {
                    break;
                }
            }
            let kwargs_splat = self.skip(|t| *t == Token::Pow)?;
            let splat = !kwargs_splat && self.skip(|t| *t == Token::Mul)?;
            let expr = self.parse_expr()?;
            if kwargs_splat {
                args.push(CallArg::KwargSplat(expr));
                has_kwargs = true;
            } else if splat {
                args.push(CallArg::PosSplat(expr));
            } else {
                match expr.kind {
                    ExprKind::Var(name) if self.is(|t| *t == Token::Assign) => {
                        self.next()?;
                        has_kwargs = true;
                        args.push(CallArg::Kwarg(name, self.parse_expr()?));
                    }
                    kind if has_kwargs => {
                        drop(kind);
                        return Err(syntax_error("non-keyword arg after keyword arg"));
                    }
                    kind => args.push(CallArg::Pos(Expr {
                        kind,
                        span: expr.span,
                    })),
                }
            }
            if args.len() > 2000 {
                return Err(syntax_error("Too many arguments in function call"));
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        self.enter()?;
        let result = self.parse_primary_inner();
        self.leave();
        result
    }

    fn parse_primary_inner(&mut self) -> Result<Expr, Error> {
        let (token, loc) = match self.next()? {
            Some(next) => next,
            None => return Err(unexpected_eof("expression")),
        };
        let constant = |value: Value| Expr {
            kind: ExprKind::Const(value),
            span: span(loc),
        };
        Ok(match token {
            Token::Ident("true" | "True") => constant(Value::from(true)),
            Token::Ident("false" | "False") => constant(Value::from(false)),
            Token::Ident("none" | "None") => constant(Value::NONE),
            Token::Ident(name) => Expr {
                kind: ExprKind::Var(name.to_owned()),
                span: span(loc),
            },
            Token::Str(text) => {
                // Adjacent string literals join: `"a" "b"` is `"ab"`.
                let mut text = text.into_owned();
                while let Some(Token::Str(more)) = self.peek() {
                    text.push_str(more);
                    self.next()?;
                }
                constant(Value::from(text))
            }
            Token::Int(value) => constant(Value::from(value)),
            Token::Float(value) => constant(Value::from(value)),
            Token::ParenOpen => self.parse_tuple_or_expression(loc)?,
            Token::BracketOpen => {
                let mut items = Vec::new();
                loop {
                    if self.skip(|t| *t == Token::BracketClose)? {
                        break;
                    }
                    if !items.is_empty() {
                        self.expect(|t| *t == Token::Comma, "`,`")?;
                        if self.skip(|t| *t == Token::BracketClose)? {
                            break;
                        }
                    }
                    items.push(self.parse_expr()?);
                }
                Expr {
                    kind: ExprKind::List(items),
                    span: span(loc),
                }
            }
            Token::BraceOpen => {
                let mut pairs = Vec::new();
                loop {
                    if self.skip(|t| *t == Token::BraceClose)? {
                        break;
                    }
                    if !pairs.is_empty() {
                        self.expect(|t| *t == Token::Comma, "`,`")?;
                        if self.skip(|t| *t == Token::BraceClose)? {
                            break;
                        }
                    }
                    let key = self.parse_expr()?;
                    self.expect(|t| *t == Token::Colon, "`:`")?;
                    let value = self.parse_expr()?;
                    pairs.push((key, value));
                }
                Expr {
                    kind: ExprKind::Map(pairs),
                    span: span(loc),
                }
            }
            token => return Err(syntax_error(format!("unexpected {token}"))),
        })
    }

    fn parse_tuple_or_expression(&mut self, loc: Loc) -> Result<Expr, Error> {
        if self.skip(|t| *t == Token::ParenClose)? {
            return Ok(Expr {
                kind: ExprKind::Tuple(Vec::new()),
                span: span(loc),
            });
        }
        let expr = self.parse_expr()?;
        if !self.is(|t| *t == Token::Comma) {
            self.expect(|t| *t == Token::ParenClose, "`)`")?;
            return Ok(expr);
        }
        let mut items = vec![expr];
        loop {
            if self.skip(|t| *t == Token::ParenClose)? {
                break;
            }
            self.expect(|t| *t == Token::Comma, "`,`")?;
            if self.skip(|t| *t == Token::ParenClose)? {
                break;
            }
            items.push(self.parse_expr()?);
        }
        Ok(Expr {
            kind: ExprKind::Tuple(items),
            span: span(loc),
        })
    }
}

fn span_of(loc: Loc) -> Span {
    span(loc)
}

fn binop(op: BinOp, left: Expr, right: Expr) -> Expr {
    let span = Span {
        line: right.span.line,
        col: left.span.col,
    };
    Expr {
        kind: ExprKind::BinOp(op, Box::new(left), Box::new(right)),
        span,
    }
}

/// The line an error in a call-like node reports: that of what is
/// evaluated last for it, the last argument or else `fallback`.
fn last_line(args: &[CallArg], fallback: u32) -> u32 {
    args.last().map_or(fallback, |arg| match arg {
        CallArg::Pos(expr)
        | CallArg::Kwarg(_, expr)
        | CallArg::PosSplat(expr)
        | CallArg::KwargSplat(expr) => expr.span.line,
    })
}

/// The names a macro body reads before (or without) assigning them: what
/// its closure captures when the macro is defined.
mod closure {
    use super::*;

    struct Tracker {
        out: BTreeSet<String>,
        assigned: Vec<HashSet<String>>,
    }

    impl Tracker {
        fn is_assigned(&self, name: &str) -> bool {
            self.assigned.iter().any(|scope| scope.contains(name))
        }

        fn assign(&mut self, name: &str) {
            if let Some(scope) = self.assigned.last_mut() {
                scope.insert(name.to_owned());
            }
        }

        fn push(&mut self) {
            self.assigned.push(HashSet::new());
        }

        fn pop(&mut self) {
            self.assigned.pop();
        }
    }

    pub(super) fn find(decl: &Macro) -> BTreeSet<String> {
        let mut tracker = Tracker {
            out: BTreeSet::new(),
            assigned: vec![HashSet::new()],
        };
        visit_macro(decl, &mut tracker, false);
        tracker.out
    }

    fn visit_macro(decl: &Macro, tracker: &mut Tracker, declare_caller: bool) {
        if declare_caller {
            tracker.assign("caller");
        }
        for arg in &decl.args {
            tracker.assign(arg);
        }
        for default in &decl.defaults {
            visit_expr(default, tracker);
        }
        for stmt in &decl.body {
            walk(stmt, tracker);
        }
    }

    fn assign(target: &Assign, tracker: &mut Tracker) {
        match target {
            Assign::Name(name) => tracker.assign(name),
            Assign::Unpack(items, _) => items.iter().for_each(|item| assign(item, tracker)),
            Assign::Attr(..) => {}
        }
    }

    fn visit_args(args: &[CallArg], tracker: &mut Tracker) {
        for arg in args {
            match arg {
                CallArg::Pos(expr)
                | CallArg::Kwarg(_, expr)
                | CallArg::PosSplat(expr)
                | CallArg::KwargSplat(expr) => visit_expr(expr, tracker),
            }
        }
    }

    fn visit_expr(expr: &Expr, tracker: &mut Tracker) {
        match &expr.kind {
            ExprKind::Var(name) => {
                if !tracker.is_assigned(name) {
                    tracker.out.insert(name.clone());
                    tracker.assign(name);
                }
            }
            ExprKind::Const(_) => {}
            ExprKind::Not(inner) | ExprKind::Neg(inner) => visit_expr(inner, tracker),
            ExprKind::BinOp(_, left, right) => {
                visit_expr(left, tracker);
                visit_expr(right, tracker);
            }
            ExprKind::Compare(first, ops) => {
                visit_expr(first, tracker);
                ops.iter().for_each(|(_, e)| visit_expr(e, tracker));
            }
            ExprKind::IfExpr {
                test,
                then,
                otherwise,
            } => {
                visit_expr(test, tracker);
                visit_expr(then, tracker);
                if let Some(otherwise) = otherwise {
                    visit_expr(otherwise, tracker);
                }
            }
            ExprKind::Filter { expr, args, .. } => {
                if let Some(expr) = expr {
                    visit_expr(expr, tracker);
                }
                visit_args(args, tracker);
            }
            ExprKind::Test { expr, args, .. } => {
                visit_expr(expr, tracker);
                visit_args(args, tracker);
            }
            ExprKind::GetAttr(inner, _) => visit_expr(inner, tracker),
            ExprKind::GetItem(inner, subscript) => {
                visit_expr(inner, tracker);
                visit_expr(subscript, tracker);
            }
            // The sliced value itself is not captured, only the bounds.
            ExprKind::Slice {
                start, stop, step, ..
            } => {
                for bound in [start, stop, step].into_iter().flatten() {
                    visit_expr(bound, tracker);
                }
            }
            ExprKind::Call(callee, args) => {
                visit_expr(callee, tracker);
                visit_args(args, tracker);
            }
            ExprKind::List(items) | ExprKind::Tuple(items) => {
                items.iter().for_each(|item| visit_expr(item, tracker));
            }
            ExprKind::Map(pairs) => {
                for (key, value) in pairs {
                    visit_expr(key, tracker);
                    visit_expr(value, tracker);
                }
            }
        }
    }

    fn walk(stmt: &Stmt, tracker: &mut Tracker) {
        match &stmt.kind {
            StmtKind::Raw(_)
            | StmtKind::Block(_)
            | StmtKind::Extends(_)
            | StmtKind::Include { .. }
            | StmtKind::Continue
            | StmtKind::Break => {}
            StmtKind::Emit(expr) => visit_expr(expr, tracker),
            StmtKind::For(for_loop) => {
                tracker.push();
                tracker.assign("loop");
                visit_expr(&for_loop.iter, tracker);
                assign(&for_loop.target, tracker);
                if let Some(filter) = &for_loop.filter {
                    visit_expr(filter, tracker);
                }
                for_loop.body.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
                tracker.push();
                for_loop.else_body.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
            }
            StmtKind::If(cond) => {
                visit_expr(&cond.test, tracker);
                tracker.push();
                cond.body.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
                tracker.push();
                cond.else_body.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
            }
            StmtKind::With(with) => {
                tracker.push();
                for (target, expr) in &with.assignments {
                    assign(target, tracker);
                    visit_expr(expr, tracker);
                }
                with.body.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
            }
            StmtKind::Set(set) => {
                assign(&set.0, tracker);
                visit_expr(&set.1, tracker);
            }
            StmtKind::AutoEscape(inner) => {
                tracker.push();
                inner.1.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
            }
            StmtKind::FilterBlock(inner) => {
                tracker.push();
                inner.1.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
            }
            StmtKind::SetBlock(set) => {
                assign(&set.target, tracker);
                tracker.push();
                set.body.iter().for_each(|s| walk(s, tracker));
                tracker.pop();
            }
            StmtKind::Import(import) => assign(&import.1, tracker),
            StmtKind::FromImport(from) => {
                for (name, alias) in &from.names {
                    tracker.assign(alias.as_deref().unwrap_or(name));
                }
            }
            StmtKind::Macro(decl) => {
                tracker.assign(&decl.name);
                tracker.push();
                visit_macro(decl, tracker, true);
                tracker.pop();
            }
            StmtKind::CallBlock(call) => {
                if let ExprKind::Call(callee, args) = &call.0.kind {
                    visit_expr(callee, tracker);
                    visit_args(args, tracker);
                }
                tracker.push();
                visit_macro(&call.1, tracker, true);
                tracker.pop();
            }
            StmtKind::Do(expr) => {
                if let ExprKind::Call(callee, args) = &expr.kind {
                    visit_expr(callee, tracker);
                    visit_args(args, tracker);
                }
            }
        }
    }
}
