//! Template syntax: the lexer, the parser and the syntax tree.

pub(crate) mod ast;
mod lexer;
mod parser;

pub(crate) use ast::Ast;
pub(crate) use lexer::Whitespace;
pub(crate) use parser::{parse, stack_address};
