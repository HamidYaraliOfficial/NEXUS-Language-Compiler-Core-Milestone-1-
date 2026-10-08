//! nexus-parser
//!
//! A hand-written recursive-descent parser with Pratt-style precedence
//! climbing for expressions and **panic-mode error recovery**: a syntax
//! error inside one statement or item does not stop the whole parse — the
//! parser resynchronizes at the next statement/item boundary and keeps
//! going, so `nexus check` can report many syntax errors from a single run
//! (mirrors the "Error Recovery" requirement of the project spec).

use nexus_ast::*;
use nexus_diagnostics::{Diagnostic, DiagnosticBag, Span};
use nexus_lexer::{Lexer, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: DiagnosticBag,
    /// Disables parsing `Ident { ... }` as a struct literal; set while
    /// parsing an `if`/`while` condition so `if x { ... }` parses as a
    /// condition followed by a block, not a struct literal (same fix Rust
    /// uses).
    no_struct_lit: bool,
}

type PResult<T> = Result<T, ()>;

/// Parses `source` end-to-end and returns the (possibly partial) program
/// together with every diagnostic collected while lexing and parsing.
pub fn parse(source: &str) -> (Program, DiagnosticBag) {
    let (tokens, lex_diags) = Lexer::new(source).tokenize();
    let mut parser = Parser {
        tokens,
        pos: 0,
        diagnostics: DiagnosticBag::new(),
        no_struct_lit: false,
    };
    let program = parser.parse_program();
    let mut diags = lex_diags;
    diags.extend(parser.diagnostics);
    (program, diags)
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind)
    }

    fn eat(&mut self, kind: &TokenKind) -> Option<Token> {
        if self.check(kind) {
            Some(self.advance())
        } else {
            None
        }
    }

    fn expect(&mut self, kind: TokenKind, ctx: &str) -> PResult<Token> {
        if self.check(&kind) {
            Ok(self.advance())
        } else {
            let found = self.peek().kind.describe();
            let span = self.peek().span;
            self.diagnostics.push(
                Diagnostic::error(
                    "E0100",
                    format!("expected {} while parsing {}, found {}", kind.describe(), ctx, found),
                    span,
                )
                .with_help(format!("insert `{}` here", kind.describe())),
            );
            Err(())
        }
    }

    fn err_unexpected(&mut self, ctx: &str) {
        let found = self.peek().kind.describe();
        let span = self.peek().span;
        self.diagnostics
            .push(Diagnostic::error("E0101", format!("unexpected {} while parsing {}", found, ctx), span));
    }

    // ---- Program / items -------------------------------------------------

    fn parse_program(&mut self) -> Program {
        let mut items = Vec::new();
        while !self.at_eof() {
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(()) => self.synchronize_item(),
            }
        }
        Program { items }
    }

    fn synchronize_item(&mut self) {
        // Always make progress.
        if !self.at_eof() {
            self.advance();
        }
        while !self.at_eof() {
            if matches!(self.peek_kind(), TokenKind::KwFn | TokenKind::KwStruct) {
                break;
            }
            self.advance();
        }
    }

    fn parse_item(&mut self) -> PResult<Item> {
        match self.peek_kind() {
            TokenKind::KwFn => self.parse_function().map(Item::Function),
            TokenKind::KwStruct => self.parse_struct().map(Item::Struct),
            _ => {
                self.err_unexpected("a top-level item (expected `fn` or `struct`)");
                Err(())
            }
        }
    }

    fn parse_function(&mut self) -> PResult<FunctionDecl> {
        let start = self.expect(TokenKind::KwFn, "function declaration")?.span;
        let name = self.parse_ident("function name")?;
        self.expect(TokenKind::LParen, "function parameters")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                let pspan_start = self.peek().span;
                let pname = self.parse_ident("parameter name")?;
                self.expect(TokenKind::Colon, "parameter type")?;
                let ty = self.parse_type()?;
                params.push(Param { name: pname, ty, span: pspan_start });
                if self.eat(&TokenKind::Comma).is_none() {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen, "function parameters")?;
        let return_type = if self.eat(&TokenKind::Arrow).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        let span = start.to(body.span);
        Ok(FunctionDecl { name, params, return_type, body, span })
    }

    fn parse_struct(&mut self) -> PResult<StructDecl> {
        let start = self.expect(TokenKind::KwStruct, "struct declaration")?.span;
        let name = self.parse_ident("struct name")?;
        self.expect(TokenKind::LBrace, "struct body")?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            let fspan = self.peek().span;
            let fname = self.parse_ident("field name")?;
            self.expect(TokenKind::Colon, "field type")?;
            let ty = self.parse_type()?;
            fields.push(FieldDecl { name: fname, ty, span: fspan });
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RBrace, "struct body")?.span;
        Ok(StructDecl { name, fields, span: start.to(end) })
    }

    fn parse_ident(&mut self, ctx: &str) -> PResult<String> {
        match self.peek_kind().clone() {
            TokenKind::Ident(s) => {
                self.advance();
                Ok(s)
            }
            _ => {
                self.err_unexpected(ctx);
                Err(())
            }
        }
    }

    fn parse_type(&mut self) -> PResult<TypeExpr> {
        match self.peek_kind().clone() {
            TokenKind::KwInt => {
                self.advance();
                Ok(TypeExpr::Int)
            }
            TokenKind::KwFloat => {
                self.advance();
                Ok(TypeExpr::Float)
            }
            TokenKind::KwBool => {
                self.advance();
                Ok(TypeExpr::Bool)
            }
            TokenKind::KwString => {
                self.advance();
                Ok(TypeExpr::String)
            }
            TokenKind::KwUnit => {
                self.advance();
                Ok(TypeExpr::Unit)
            }
            TokenKind::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(TokenKind::RBracket, "array type")?;
                Ok(TypeExpr::Array(Box::new(inner)))
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(TypeExpr::Named(name))
            }
            _ => {
                self.err_unexpected("a type");
                Err(())
            }
        }
    }

    // ---- Statements --------------------------------------------------

    fn parse_block(&mut self) -> PResult<Block> {
        let start = self.expect(TokenKind::LBrace, "block")?.span;
        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.parse_stmt() {
                Ok(s) => stmts.push(s),
                Err(()) => self.synchronize_stmt(),
            }
        }
        let end = self.expect(TokenKind::RBrace, "block")?.span;
        Ok(Block { stmts, span: start.to(end) })
    }

    fn synchronize_stmt(&mut self) {
        if !self.at_eof() {
            self.advance();
        }
        while !self.at_eof() {
            if matches!(
                self.peek_kind(),
                TokenKind::RBrace
                    | TokenKind::KwLet
                    | TokenKind::KwReturn
                    | TokenKind::KwIf
                    | TokenKind::KwWhile
                    | TokenKind::KwFor
                    | TokenKind::KwBreak
                    | TokenKind::KwContinue
            ) {
                return;
            }
            if let TokenKind::Semicolon = self.peek_kind() {
                self.advance();
                return;
            }
            self.advance();
        }
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        match self.peek_kind() {
            TokenKind::KwLet => self.parse_let(),
            TokenKind::KwReturn => self.parse_return(),
            TokenKind::KwIf => self.parse_if(),
            TokenKind::KwWhile => self.parse_while(),
            TokenKind::KwFor => self.parse_for(),
            TokenKind::KwBreak => {
                let span = self.advance().span;
                self.expect(TokenKind::Semicolon, "`break`")?;
                Ok(Stmt::Break(span))
            }
            TokenKind::KwContinue => {
                let span = self.advance().span;
                self.expect(TokenKind::Semicolon, "`continue`")?;
                Ok(Stmt::Continue(span))
            }
            TokenKind::LBrace => Ok(Stmt::Block(self.parse_block()?)),
            _ => {
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon, "expression statement")?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_let(&mut self) -> PResult<Stmt> {
        let start = self.expect(TokenKind::KwLet, "let statement")?.span;
        let mutable = self.eat(&TokenKind::KwMut).is_some();
        let name = self.parse_ident("let binding name")?;
        let ty = if self.eat(&TokenKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(TokenKind::Eq, "let statement")?;
        let value = self.parse_expr()?;
        let end = self.expect(TokenKind::Semicolon, "let statement")?.span;
        Ok(Stmt::Let { name, mutable, ty, value, span: start.to(end) })
    }

    fn parse_return(&mut self) -> PResult<Stmt> {
        let start = self.expect(TokenKind::KwReturn, "return statement")?.span;
        let value = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        let end = self.expect(TokenKind::Semicolon, "return statement")?.span;
        Ok(Stmt::Return { value, span: start.to(end) })
    }

    fn parse_if(&mut self) -> PResult<Stmt> {
        let start = self.expect(TokenKind::KwIf, "if statement")?.span;
        self.no_struct_lit = true;
        let cond = self.parse_expr()?;
        self.no_struct_lit = false;
        let then_branch = self.parse_block()?;
        let mut span = start.to(then_branch.span);
        let else_branch = if self.eat(&TokenKind::KwElse).is_some() {
            if self.check(&TokenKind::KwIf) {
                let s = self.parse_if()?;
                span = span.to(s.span());
                Some(Box::new(ElseBranch::If(Box::new(s))))
            } else {
                let b = self.parse_block()?;
                span = span.to(b.span);
                Some(Box::new(ElseBranch::Block(b)))
            }
        } else {
            None
        };
        Ok(Stmt::If { cond, then_branch, else_branch, span })
    }

    fn parse_while(&mut self) -> PResult<Stmt> {
        let start = self.expect(TokenKind::KwWhile, "while statement")?.span;
        self.no_struct_lit = true;
        let cond = self.parse_expr()?;
        self.no_struct_lit = false;
        let body = self.parse_block()?;
        let span = start.to(body.span);
        Ok(Stmt::While { cond, body, span })
    }

    fn parse_for(&mut self) -> PResult<Stmt> {
        let start = self.expect(TokenKind::KwFor, "for statement")?.span;
        let var = self.parse_ident("loop variable")?;
        self.expect(TokenKind::KwIn, "for statement")?;
        self.no_struct_lit = true;
        let range_start = self.parse_expr()?;
        self.expect(TokenKind::DotDot, "for range (`start..end`)")?;
        let range_end = self.parse_expr()?;
        self.no_struct_lit = false;
        let body = self.parse_block()?;
        let span = start.to(body.span);
        Ok(Stmt::For { var, start: range_start, end: range_end, body, span })
    }

    // ---- Expressions (precedence climbing) ----------------------------

    fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> PResult<Expr> {
        let lhs = self.parse_or()?;
        if self.check(&TokenKind::Eq) {
            let span = self.advance().span;
            if !matches!(lhs, Expr::Ident(..) | Expr::Index { .. } | Expr::FieldAccess { .. }) {
                self.diagnostics.push(Diagnostic::error(
                    "E0102",
                    "invalid assignment target",
                    lhs.span(),
                ).with_help("only variables, array elements and struct fields can be assigned to"));
                return Err(());
            }
            let value = self.parse_assignment()?;
            let span = lhs.span().to(span).to(value.span());
            return Ok(Expr::Assign { target: Box::new(lhs), value: Box::new(value), span });
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_and()?;
        while self.check(&TokenKind::OrOr) {
            self.advance();
            let rhs = self.parse_and()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op: BinaryOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_equality()?;
        while self.check(&TokenKind::AndAnd) {
            self.advance();
            let rhs = self.parse_equality()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op: BinaryOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_comparison()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::NotEq => BinaryOp::NotEq,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_comparison()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_comparison(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_term()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::LtEq => BinaryOp::LtEq,
                TokenKind::GtEq => BinaryOp::GtEq,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_term()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_factor()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_factor()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_factor(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        match self.peek_kind() {
            TokenKind::Minus => {
                let start = self.advance().span;
                let expr = self.parse_unary()?;
                let span = start.to(expr.span());
                Ok(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr), span })
            }
            TokenKind::Not => {
                let start = self.advance().span;
                let expr = self.parse_unary()?;
                let span = start.to(expr.span());
                Ok(Expr::Unary { op: UnaryOp::Not, expr: Box::new(expr), span })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::LParen if matches!(expr, Expr::Ident(..)) => {
                    let callee = if let Expr::Ident(n, _) = &expr { n.clone() } else { unreachable!() };
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.eat(&TokenKind::Comma).is_none() {
                                break;
                            }
                        }
                    }
                    let end = self.expect(TokenKind::RParen, "call arguments")?.span;
                    let span = expr.span().to(end);
                    expr = Expr::Call { callee, args, span };
                }
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    let end = self.expect(TokenKind::RBracket, "index expression")?.span;
                    let span = expr.span().to(end);
                    expr = Expr::Index { base: Box::new(expr), index: Box::new(index), span };
                }
                TokenKind::Dot => {
                    self.advance();
                    let field = self.parse_ident("field name")?;
                    let span = expr.span();
                    expr = Expr::FieldAccess { base: Box::new(expr), field, span };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Ok(Expr::IntLit(v, tok.span))
            }
            TokenKind::Float(v) => {
                self.advance();
                Ok(Expr::FloatLit(v, tok.span))
            }
            TokenKind::Str(ref s) => {
                self.advance();
                Ok(Expr::StringLit(s.clone(), tok.span))
            }
            TokenKind::KwTrue => {
                self.advance();
                Ok(Expr::BoolLit(true, tok.span))
            }
            TokenKind::KwFalse => {
                self.advance();
                Ok(Expr::BoolLit(false, tok.span))
            }
            TokenKind::Ident(ref name) => {
                self.advance();
                if !self.no_struct_lit && self.check(&TokenKind::LBrace) {
                    return self.parse_struct_lit(name.clone(), tok.span);
                }
                Ok(Expr::Ident(name.clone(), tok.span))
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen, "parenthesized expression")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                let start = self.advance().span;
                let mut elements = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        elements.push(self.parse_expr()?);
                        if self.eat(&TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                }
                let end = self.expect(TokenKind::RBracket, "array literal")?.span;
                Ok(Expr::ArrayLit { elements, span: start.to(end) })
            }
            _ => {
                self.err_unexpected("an expression");
                Err(())
            }
        }
    }

    fn parse_struct_lit(&mut self, name: String, start: Span) -> PResult<Expr> {
        self.expect(TokenKind::LBrace, "struct literal")?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            let fname = self.parse_ident("struct field name")?;
            self.expect(TokenKind::Colon, "struct field value")?;
            let value = self.parse_expr()?;
            fields.push((fname, value));
            if self.eat(&TokenKind::Comma).is_none() {
                break;
            }
        }
        let end = self.expect(TokenKind::RBrace, "struct literal")?.span;
        Ok(Expr::StructLit { name, fields, span: start.to(end) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_parse(src: &str) -> Program {
        let (program, diags) = parse(src);
        assert!(!diags.has_errors(), "unexpected errors: {:#?}", diags.into_vec());
        program
    }

    #[test]
    fn parses_simple_function() {
        let p = ok_parse("fn add(a: int, b: int) -> int { return a + b; }");
        assert_eq!(p.items.len(), 1);
        match &p.items[0] {
            Item::Function(f) => {
                assert_eq!(f.name, "add");
                assert_eq!(f.params.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parses_struct() {
        let p = ok_parse("struct Point { x: int, y: int }");
        match &p.items[0] {
            Item::Struct(s) => assert_eq!(s.fields.len(), 2),
            _ => panic!("expected struct"),
        }
    }

    #[test]
    fn parses_if_else_and_while_and_for() {
        ok_parse(
            r#"
            fn main() -> unit {
                let mut i = 0;
                while i < 10 {
                    if i == 5 { i = i + 1; } else { i = i + 2; }
                }
                for j in 0..10 {
                    print(j);
                }
            }
            "#,
        );
    }

    #[test]
    fn parses_struct_literal_and_field_access_and_index() {
        ok_parse(
            r#"
            struct Point { x: int, y: int }
            fn main() -> unit {
                let p = Point { x: 1, y: 2 };
                let a = [1, 2, 3];
                let v = a[0] + p.x;
            }
            "#,
        );
    }

    #[test]
    fn if_condition_does_not_swallow_struct_literal() {
        // `if flag { ... }` must parse `flag` as a plain identifier
        // condition, not as `flag { }` struct literal syntax.
        ok_parse(
            r#"
            fn main() -> unit {
                let flag = true;
                if flag {
                    print("yes");
                }
            }
            "#,
        );
    }

    #[test]
    fn recovers_from_multiple_statement_errors_in_one_function() {
        // Two malformed statements inside `broken`, each missing its
        // right-hand-side expression. Statement-level panic-mode recovery
        // should report *both* errors and still successfully parse the
        // rest of `broken` plus the following `good` function, instead of
        // stopping at the first problem.
        let src = r#"
        fn broken() -> int {
            let x = ;
            let y = ;
            return 1;
        }

        fn good() -> int {
            return 1;
        }
        "#;
        let (program, diags) = parse(src);
        assert!(diags.has_errors());
        assert_eq!(diags.error_count(), 2, "expected exactly 2 recovered errors, got: {:#?}", diags.into_vec());
        assert!(program.items.iter().any(|i| matches!(i, Item::Function(f) if f.name == "broken")));
        assert!(program.items.iter().any(|i| matches!(i, Item::Function(f) if f.name == "good")));
    }

    #[test]
    fn recovers_at_item_level_from_malformed_function_signature() {
        // A badly-formed function signature aborts parsing of `broken`
        // itself, but item-level recovery must still find `good` right
        // after it.
        let src = r#"
        fn broken( -> int {
            return 1;
        }

        fn good() -> int {
            return 1;
        }
        "#;
        let (program, diags) = parse(src);
        assert!(diags.has_errors());
        assert!(program.items.iter().any(|i| matches!(i, Item::Function(f) if f.name == "good")));
    }

    #[test]
    fn operator_precedence() {
        let p = ok_parse("fn f() -> int { return 1 + 2 * 3; }");
        if let Item::Function(f) = &p.items[0] {
            if let Stmt::Return { value: Some(Expr::Binary { op: BinaryOp::Add, rhs, .. }), .. } = &f.body.stmts[0] {
                assert!(matches!(**rhs, Expr::Binary { op: BinaryOp::Mul, .. }));
            } else {
                panic!("expected addition at top level");
            }
        }
    }
}
