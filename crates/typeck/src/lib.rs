//! nexus-typeck
//!
//! Static semantic analysis for NEXUS: name resolution, a symbol table for
//! structs/functions/locals, and a structural type checker that reports
//! type mismatches, undefined names, missing returns, invalid struct
//! literals/field access, out-of-loop `break`/`continue`, and more — each
//! with a precise span and (where useful) a `help` suggestion, mirroring
//! the "Error Diagnostic Engine" requirement of the project spec.
//!
//! `check_program` never stops at the first problem: every function and
//! every statement is still visited even after an error, so a single run
//! reports as many real issues as possible (bounded only by avoiding
//! diagnostic cascades from already-broken sub-expressions, which are
//! typed `Type::Unknown` and silently skipped in further comparisons).

use std::collections::HashMap;

use nexus_ast::*;
use nexus_diagnostics::{Diagnostic, DiagnosticBag, Span};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Unit,
    Array(Box<Type>),
    Struct(String),
    /// Produced after an error has already been reported for this
    /// expression; comparisons against `Unknown` never produce further
    /// diagnostics, which is what stops one mistake from cascading into a
    /// wall of misleading follow-up errors.
    Unknown,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Unit => write!(f, "unit"),
            Type::Array(t) => write!(f, "[{}]", t),
            Type::Struct(n) => write!(f, "{}", n),
            Type::Unknown => write!(f, "<unknown>"),
        }
    }
}

#[derive(Debug, Clone)]
struct StructInfo {
    fields: Vec<(String, Type)>,
    span: Span,
}

#[derive(Debug, Clone)]
struct FuncSig {
    params: Vec<(String, Type)>,
    return_type: Type,
    span: Span,
}

#[derive(Debug, Clone)]
struct VarInfo {
    ty: Type,
    mutable: bool,
    #[allow(dead_code)]
    span: Span,
}

pub struct TypeChecker {
    diagnostics: DiagnosticBag,
    structs: HashMap<String, StructInfo>,
    functions: HashMap<String, FuncSig>,
    scopes: Vec<HashMap<String, VarInfo>>,
    current_return_type: Type,
    current_fn_name: String,
    loop_depth: u32,
}

const BUILTIN_NAMES: &[&str] = &["print", "assert", "len"];

/// Runs full semantic analysis over `program` and returns every diagnostic
/// found. An empty, non-error-containing bag means the program is safe to
/// hand to the interpreter.
pub fn check_program(program: &Program) -> DiagnosticBag {
    let mut tc = TypeChecker {
        diagnostics: DiagnosticBag::new(),
        structs: HashMap::new(),
        functions: HashMap::new(),
        scopes: vec![HashMap::new()],
        current_return_type: Type::Unit,
        current_fn_name: String::new(),
        loop_depth: 0,
    };
    tc.run(program);
    tc.diagnostics
}

impl TypeChecker {
    fn run(&mut self, program: &Program) {
        self.collect_struct_names(program);
        self.resolve_struct_fields(program);
        self.collect_function_sigs(program);
        self.check_functions(program);

        if !self.functions.contains_key("main") {
            self.diagnostics.push(
                Diagnostic::warning("W100", "no `main` function found", Span::default())
                    .with_help("`nexus run` looks for a parameterless `fn main()` as the entry point"),
            );
        }
    }

    // ---- Pass 1: struct names -----------------------------------------

    fn collect_struct_names(&mut self, program: &Program) {
        for item in &program.items {
            if let Item::Struct(s) = item {
                if let Some(prev) = self.structs.get(&s.name) {
                    self.diagnostics.push(
                        Diagnostic::error("E0200", format!("struct `{}` is defined more than once", s.name), s.span)
                            .with_related(prev.span, "previous definition here"),
                    );
                    continue;
                }
                self.structs.insert(s.name.clone(), StructInfo { fields: Vec::new(), span: s.span });
            }
        }
    }

    // ---- Pass 2: struct fields (structs may reference each other) -----

    fn resolve_struct_fields(&mut self, program: &Program) {
        for item in &program.items {
            if let Item::Struct(s) = item {
                let mut fields = Vec::new();
                let mut seen: HashMap<String, Span> = HashMap::new();
                for f in &s.fields {
                    if let Some(prev_span) = seen.get(&f.name) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0201",
                                format!("field `{}` is defined more than once in struct `{}`", f.name, s.name),
                                f.span,
                            )
                            .with_related(*prev_span, "previous field here"),
                        );
                        continue;
                    }
                    seen.insert(f.name.clone(), f.span);
                    let ty = self.resolve_type_expr(&f.ty, f.span);
                    fields.push((f.name.clone(), ty));
                }
                if let Some(info) = self.structs.get_mut(&s.name) {
                    info.fields = fields;
                }
            }
        }
    }

    fn resolve_type_expr(&mut self, ty: &TypeExpr, span: Span) -> Type {
        match ty {
            TypeExpr::Int => Type::Int,
            TypeExpr::Float => Type::Float,
            TypeExpr::Bool => Type::Bool,
            TypeExpr::String => Type::String,
            TypeExpr::Unit => Type::Unit,
            TypeExpr::Array(inner) => Type::Array(Box::new(self.resolve_type_expr(inner, span))),
            TypeExpr::Named(name) => {
                if self.structs.contains_key(name) {
                    Type::Struct(name.clone())
                } else {
                    self.diagnostics.push(
                        Diagnostic::error("E0202", format!("undefined type `{}`", name), span)
                            .with_help("declare it with `struct` before using it, or check for a typo"),
                    );
                    Type::Unknown
                }
            }
        }
    }

    // ---- Pass 3: function signatures -----------------------------------

    fn collect_function_sigs(&mut self, program: &Program) {
        for item in &program.items {
            if let Item::Function(f) = item {
                if BUILTIN_NAMES.contains(&f.name.as_str()) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0203",
                        format!("function name `{}` conflicts with a built-in function", f.name),
                        f.span,
                    ));
                    continue;
                }
                if let Some(prev) = self.functions.get(&f.name) {
                    self.diagnostics.push(
                        Diagnostic::error("E0204", format!("function `{}` is defined more than once", f.name), f.span)
                            .with_related(prev.span, "previous definition here"),
                    );
                    continue;
                }
                let params = f
                    .params
                    .iter()
                    .map(|p| (p.name.clone(), self.resolve_type_expr(&p.ty, p.span)))
                    .collect();
                let return_type = match &f.return_type {
                    Some(t) => self.resolve_type_expr(t, f.span),
                    None => Type::Unit,
                };
                self.functions.insert(f.name.clone(), FuncSig { params, return_type, span: f.span });
            }
        }
    }

    // ---- Pass 4: function bodies ----------------------------------------

    fn check_functions(&mut self, program: &Program) {
        for item in &program.items {
            if let Item::Function(f) = item {
                if !self.functions.contains_key(&f.name) {
                    // Duplicate/invalid signature already reported; skip body.
                    continue;
                }
                self.check_function_body(f);
            }
        }
    }

    fn check_function_body(&mut self, f: &FunctionDecl) {
        let sig = self.functions.get(&f.name).unwrap().clone();
        self.current_return_type = sig.return_type.clone();
        self.current_fn_name = f.name.clone();
        self.loop_depth = 0;

        self.push_scope();
        for (name, ty) in &sig.params {
            self.declare_var(name.clone(), ty.clone(), false, f.span);
        }
        self.check_block(&f.body);
        self.pop_scope();

        if sig.return_type != Type::Unit && !block_returns(&f.body) {
            self.diagnostics.push(
                Diagnostic::error(
                    "E0300",
                    format!(
                        "function `{}` has return type `{}` but does not return on all code paths",
                        f.name, sig.return_type
                    ),
                    f.span,
                )
                .with_help("add a `return` at the end of the function, or on every branch"),
            );
        }
    }

    // ---- Scope helpers ---------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare_var(&mut self, name: String, ty: Type, mutable: bool, span: Span) {
        self.scopes.last_mut().unwrap().insert(name, VarInfo { ty, mutable, span });
    }

    fn lookup_var(&self, name: &str) -> Option<VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    // ---- Statements --------------------------------------------------

    fn check_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, mutable, ty, value, span } => {
                let value_ty = self.check_expr(value);
                let final_ty = if let Some(declared) = ty {
                    let declared_ty = self.resolve_type_expr(declared, *span);
                    if !types_compatible(&declared_ty, &value_ty) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0301",
                                format!(
                                    "type mismatch in `let {}`: expected `{}`, found `{}`",
                                    name, declared_ty, value_ty
                                ),
                                value.span(),
                            )
                            .with_help("change the initializer or the declared type so they match"),
                        );
                    }
                    declared_ty
                } else {
                    value_ty
                };
                self.declare_var(name.clone(), final_ty, *mutable, *span);
            }
            Stmt::Return { value, span } => {
                let actual = match value {
                    Some(e) => self.check_expr(e),
                    None => Type::Unit,
                };
                if !types_compatible(&self.current_return_type, &actual) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0302",
                        format!(
                            "mismatched return type in `{}`: expected `{}`, found `{}`",
                            self.current_fn_name, self.current_return_type, actual
                        ),
                        value.as_ref().map(|e| e.span()).unwrap_or(*span),
                    ));
                }
            }
            Stmt::Expr(e) => {
                self.check_expr(e);
            }
            Stmt::If { cond, then_branch, else_branch, .. } => {
                let cond_ty = self.check_expr(cond);
                if !matches!(cond_ty, Type::Bool | Type::Unknown) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0303",
                        format!("`if` condition must be `bool`, found `{}`", cond_ty),
                        cond.span(),
                    ));
                }
                self.push_scope();
                self.check_block(then_branch);
                self.pop_scope();
                match else_branch.as_deref() {
                    Some(ElseBranch::Block(b)) => {
                        self.push_scope();
                        self.check_block(b);
                        self.pop_scope();
                    }
                    Some(ElseBranch::If(s)) => self.check_stmt(s),
                    None => {}
                }
            }
            Stmt::While { cond, body, .. } => {
                let cond_ty = self.check_expr(cond);
                if !matches!(cond_ty, Type::Bool | Type::Unknown) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0303",
                        format!("`while` condition must be `bool`, found `{}`", cond_ty),
                        cond.span(),
                    ));
                }
                self.loop_depth += 1;
                self.push_scope();
                self.check_block(body);
                self.pop_scope();
                self.loop_depth -= 1;
            }
            Stmt::For { var, start, end, body, span } => {
                let start_ty = self.check_expr(start);
                let end_ty = self.check_expr(end);
                if !matches!(start_ty, Type::Int | Type::Unknown) || !matches!(end_ty, Type::Int | Type::Unknown) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0304",
                        "`for` range bounds must be `int`",
                        *span,
                    ));
                }
                self.loop_depth += 1;
                self.push_scope();
                self.declare_var(var.clone(), Type::Int, false, *span);
                self.check_block(body);
                self.pop_scope();
                self.loop_depth -= 1;
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(Diagnostic::error("E0305", "`break` outside of a loop", *span));
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(Diagnostic::error("E0305", "`continue` outside of a loop", *span));
                }
            }
            Stmt::Block(b) => {
                self.push_scope();
                self.check_block(b);
                self.pop_scope();
            }
        }
    }

    // ---- Expressions --------------------------------------------------

    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::IntLit(..) => Type::Int,
            Expr::FloatLit(..) => Type::Float,
            Expr::StringLit(..) => Type::String,
            Expr::BoolLit(..) => Type::Bool,
            Expr::Ident(name, span) => match self.lookup_var(name) {
                Some(v) => v.ty,
                None => {
                    self.diagnostics.push(
                        Diagnostic::error("E0400", format!("undefined variable `{}`", name), *span)
                            .with_help("declare it first with `let`, or check for a typo"),
                    );
                    Type::Unknown
                }
            },
            Expr::Unary { op, expr, span } => {
                let t = self.check_expr(expr);
                match op {
                    UnaryOp::Neg => {
                        if matches!(t, Type::Int | Type::Float | Type::Unknown) {
                            t
                        } else {
                            self.diagnostics.push(Diagnostic::error(
                                "E0401",
                                format!("cannot negate a value of type `{}`", t),
                                *span,
                            ));
                            Type::Unknown
                        }
                    }
                    UnaryOp::Not => {
                        if matches!(t, Type::Bool | Type::Unknown) {
                            Type::Bool
                        } else {
                            self.diagnostics.push(Diagnostic::error(
                                "E0401",
                                format!("cannot apply `!` to a value of type `{}`", t),
                                *span,
                            ));
                            Type::Unknown
                        }
                    }
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.check_binary(op, lhs, rhs, *span),
            Expr::Assign { target, value, span } => {
                let value_ty = self.check_expr(value);
                let target_ty = self.check_assign_target(target);
                if !types_compatible(&target_ty, &value_ty) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0402",
                        format!("cannot assign a value of type `{}` to target of type `{}`", value_ty, target_ty),
                        *span,
                    ));
                }
                value_ty
            }
            Expr::Call { callee, args, span } => self.check_call(callee, args, *span),
            Expr::Index { base, index, span } => {
                let base_ty = self.check_expr(base);
                let index_ty = self.check_expr(index);
                if !matches!(index_ty, Type::Int | Type::Unknown) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0403",
                        format!("array index must be `int`, found `{}`", index_ty),
                        index.span(),
                    ));
                }
                match base_ty {
                    Type::Array(elem) => *elem,
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.diagnostics.push(Diagnostic::error(
                            "E0404",
                            format!("cannot index into a value of type `{}`", other),
                            *span,
                        ));
                        Type::Unknown
                    }
                }
            }
            Expr::FieldAccess { base, field, span } => {
                let base_ty = self.check_expr(base);
                match base_ty {
                    Type::Struct(name) => {
                        let info = self.structs.get(&name).cloned();
                        match info.and_then(|i| i.fields.iter().find(|(n, _)| n == field).cloned()) {
                            Some((_, ty)) => ty,
                            None => {
                                self.diagnostics.push(Diagnostic::error(
                                    "E0405",
                                    format!("struct `{}` has no field `{}`", name, field),
                                    *span,
                                ));
                                Type::Unknown
                            }
                        }
                    }
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.diagnostics.push(Diagnostic::error(
                            "E0406",
                            format!("cannot access field `{}` on non-struct type `{}`", field, other),
                            *span,
                        ));
                        Type::Unknown
                    }
                }
            }
            Expr::ArrayLit { elements, span } => {
                if elements.is_empty() {
                    return Type::Array(Box::new(Type::Unknown));
                }
                let first_ty = self.check_expr(&elements[0]);
                for el in &elements[1..] {
                    let t = self.check_expr(el);
                    if !types_compatible(&first_ty, &t) {
                        self.diagnostics.push(Diagnostic::error(
                            "E0407",
                            format!("array elements must all share one type: expected `{}`, found `{}`", first_ty, t),
                            el.span(),
                        ));
                    }
                }
                let _ = span;
                Type::Array(Box::new(first_ty))
            }
            Expr::StructLit { name, fields, span } => self.check_struct_lit(name, fields, *span),
        }
    }

    fn check_assign_target(&mut self, target: &Expr) -> Type {
        if let Expr::Ident(name, span) = target {
            match self.lookup_var(name) {
                Some(v) => {
                    if !v.mutable {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0408",
                                format!("cannot assign to immutable variable `{}`", name),
                                *span,
                            )
                            .with_help(format!("declare it as `let mut {}` instead", name)),
                        );
                    }
                    v.ty
                }
                None => {
                    self.diagnostics.push(Diagnostic::error("E0400", format!("undefined variable `{}`", name), *span));
                    Type::Unknown
                }
            }
        } else {
            // Index and field-access targets: element/field mutability is
            // governed by their container's own type; NEXUS arrays and
            // structs use reference semantics at runtime, so any
            // reachable array/struct value may have its contents updated.
            self.check_expr(target)
        }
    }

    fn check_binary(&mut self, op: &BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> Type {
        let lt = self.check_expr(lhs);
        let rt = self.check_expr(rhs);
        use BinaryOp::*;
        match op {
            Add | Sub | Mul | Div | Mod => {
                if matches!(op, Add) && lt == Type::String && rt == Type::String {
                    return Type::String;
                }
                match (&lt, &rt) {
                    (Type::Int, Type::Int) => Type::Int,
                    (Type::Float, Type::Float) => Type::Float,
                    (Type::Unknown, _) | (_, Type::Unknown) => Type::Unknown,
                    _ => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "E0409",
                                format!("cannot apply `{}` to `{}` and `{}`", bin_op_symbol(op), lt, rt),
                                span,
                            )
                            .with_help("both sides of an arithmetic operator must be the same numeric type"),
                        );
                        Type::Unknown
                    }
                }
            }
            Lt | Gt | LtEq | GtEq => {
                match (&lt, &rt) {
                    (Type::Int, Type::Int) | (Type::Float, Type::Float) => {}
                    (Type::Unknown, _) | (_, Type::Unknown) => {}
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            "E0409",
                            format!("cannot compare `{}` and `{}` with `{}`", lt, rt, bin_op_symbol(op)),
                            span,
                        ));
                    }
                }
                Type::Bool
            }
            Eq | NotEq => {
                if !types_compatible(&lt, &rt) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0409",
                        format!("cannot compare `{}` and `{}` for equality", lt, rt),
                        span,
                    ));
                }
                Type::Bool
            }
            And | Or => {
                if !matches!(lt, Type::Bool | Type::Unknown) || !matches!(rt, Type::Bool | Type::Unknown) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0409",
                        format!("`{}` requires `bool` operands, found `{}` and `{}`", bin_op_symbol(op), lt, rt),
                        span,
                    ));
                }
                Type::Bool
            }
        }
    }

    fn check_call(&mut self, callee: &str, args: &[Expr], span: Span) -> Type {
        match callee {
            "print" => {
                if args.len() != 1 {
                    self.diagnostics.push(Diagnostic::error(
                        "E0500",
                        format!("`print` expects 1 argument, found {}", args.len()),
                        span,
                    ));
                }
                for a in args {
                    self.check_expr(a);
                }
                Type::Unit
            }
            "assert" => {
                if args.is_empty() || args.len() > 2 {
                    self.diagnostics.push(Diagnostic::error(
                        "E0501",
                        format!("`assert` expects 1 or 2 arguments, found {}", args.len()),
                        span,
                    ));
                }
                if let Some(first) = args.first() {
                    let t = self.check_expr(first);
                    if !matches!(t, Type::Bool | Type::Unknown) {
                        self.diagnostics.push(Diagnostic::error(
                            "E0501",
                            format!("`assert` condition must be `bool`, found `{}`", t),
                            first.span(),
                        ));
                    }
                }
                if let Some(second) = args.get(1) {
                    self.check_expr(second);
                }
                Type::Unit
            }
            "len" => {
                if args.len() != 1 {
                    self.diagnostics.push(Diagnostic::error(
                        "E0502",
                        format!("`len` expects 1 argument, found {}", args.len()),
                        span,
                    ));
                    return Type::Int;
                }
                let t = self.check_expr(&args[0]);
                if !matches!(t, Type::Array(_) | Type::String | Type::Unknown) {
                    self.diagnostics.push(Diagnostic::error(
                        "E0502",
                        format!("`len` expects an array or string, found `{}`", t),
                        args[0].span(),
                    ));
                }
                Type::Int
            }
            _ => {
                let sig = self.functions.get(callee).cloned();
                match sig {
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error("E0503", format!("undefined function `{}`", callee), span)
                                .with_help("check the function name for typos, or declare it with `fn`"),
                        );
                        for a in args {
                            self.check_expr(a);
                        }
                        Type::Unknown
                    }
                    Some(sig) => {
                        if args.len() != sig.params.len() {
                            self.diagnostics.push(Diagnostic::error(
                                "E0504",
                                format!(
                                    "function `{}` expects {} argument(s), found {}",
                                    callee,
                                    sig.params.len(),
                                    args.len()
                                ),
                                span,
                            ));
                        }
                        for (i, a) in args.iter().enumerate() {
                            let arg_ty = self.check_expr(a);
                            if let Some((_, expected)) = sig.params.get(i) {
                                if !types_compatible(expected, &arg_ty) {
                                    self.diagnostics.push(Diagnostic::error(
                                        "E0505",
                                        format!(
                                            "argument {} to `{}` has type `{}`, expected `{}`",
                                            i + 1,
                                            callee,
                                            arg_ty,
                                            expected
                                        ),
                                        a.span(),
                                    ));
                                }
                            }
                        }
                        sig.return_type
                    }
                }
            }
        }
    }

    fn check_struct_lit(&mut self, name: &str, fields: &[(String, Expr)], span: Span) -> Type {
        let info = self.structs.get(name).cloned();
        let Some(info) = info else {
            self.diagnostics.push(
                Diagnostic::error("E0202", format!("undefined type `{}`", name), span)
                    .with_help("declare it with `struct` before constructing it"),
            );
            for (_, v) in fields {
                self.check_expr(v);
            }
            return Type::Unknown;
        };

        let mut provided: HashMap<String, Span> = HashMap::new();
        for (fname, fexpr) in fields {
            let value_ty = self.check_expr(fexpr);
            if let Some(prev) = provided.get(fname) {
                self.diagnostics.push(
                    Diagnostic::error("E0600", format!("field `{}` specified more than once", fname), fexpr.span())
                        .with_related(*prev, "first specified here"),
                );
                continue;
            }
            provided.insert(fname.clone(), fexpr.span());
            match info.fields.iter().find(|(n, _)| n == fname) {
                Some((_, expected)) => {
                    if !types_compatible(expected, &value_ty) {
                        self.diagnostics.push(Diagnostic::error(
                            "E0601",
                            format!(
                                "field `{}` of struct `{}` expects `{}`, found `{}`",
                                fname, name, expected, value_ty
                            ),
                            fexpr.span(),
                        ));
                    }
                }
                None => {
                    self.diagnostics.push(Diagnostic::error(
                        "E0602",
                        format!("struct `{}` has no field `{}`", name, fname),
                        fexpr.span(),
                    ));
                }
            }
        }
        for (fname, _) in &info.fields {
            if !provided.contains_key(fname) {
                self.diagnostics.push(Diagnostic::error(
                    "E0603",
                    format!("missing field `{}` in initializer of struct `{}`", fname, name),
                    span,
                ));
            }
        }
        Type::Struct(name.to_string())
    }
}

fn types_compatible(expected: &Type, actual: &Type) -> bool {
    expected == actual || matches!(expected, Type::Unknown) || matches!(actual, Type::Unknown)
}

fn bin_op_symbol(op: &BinaryOp) -> &'static str {
    use BinaryOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Eq => "==",
        NotEq => "!=",
        Lt => "<",
        Gt => ">",
        LtEq => "<=",
        GtEq => ">=",
        And => "&&",
        Or => "||",
    }
}

/// Conservative "definitely returns on every path" analysis, used to detect
/// non-`unit` functions that can fall off the end without a `return`.
fn block_returns(block: &Block) -> bool {
    block.stmts.iter().any(stmt_returns)
}

fn stmt_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::If { then_branch, else_branch: Some(eb), .. } => {
            let then_r = block_returns(then_branch);
            let else_r = match eb.as_ref() {
                ElseBranch::Block(b) => block_returns(b),
                ElseBranch::If(s) => stmt_returns(s),
            };
            then_r && else_r
        }
        Stmt::Block(b) => block_returns(b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str) -> DiagnosticBag {
        let (program, parse_diags) = nexus_parser::parse(src);
        assert!(!parse_diags.has_errors(), "parse errors: {:#?}", parse_diags.into_vec());
        check_program(&program)
    }

    #[test]
    fn accepts_well_typed_program() {
        let diags = check(
            r#"
            struct Point { x: int, y: int }
            fn dist2(p: Point) -> int {
                return p.x * p.x + p.y * p.y;
            }
            fn main() -> unit {
                let p = Point { x: 3, y: 4 };
                let d = dist2(p);
                print(d);
            }
            "#,
        );
        assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    }

    #[test]
    fn detects_type_mismatch_in_let() {
        let diags = check("fn main() -> unit { let x: int = \"hi\"; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn detects_undefined_variable() {
        let diags = check("fn main() -> unit { print(y); }");
        assert!(diags.has_errors());
    }

    #[test]
    fn detects_missing_return() {
        let diags = check("fn f() -> int { let x = 1; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn if_else_both_returning_satisfies_missing_return_check() {
        let diags = check(
            r#"
            fn f(flag: bool) -> int {
                if flag {
                    return 1;
                } else {
                    return 2;
                }
            }
            "#,
        );
        assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    }

    #[test]
    fn detects_assignment_to_immutable_variable() {
        let diags = check("fn main() -> unit { let x = 1; x = 2; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn allows_assignment_to_mutable_variable() {
        let diags = check("fn main() -> unit { let mut x = 1; x = 2; }");
        assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    }

    #[test]
    fn detects_wrong_argument_count_and_type() {
        let diags = check(
            r#"
            fn add(a: int, b: int) -> int { return a + b; }
            fn main() -> unit {
                let x = add(1);
                let y = add(1, "two");
            }
            "#,
        );
        assert!(diags.has_errors());
        assert!(diags.error_count() >= 2);
    }

    #[test]
    fn detects_break_outside_loop() {
        let diags = check("fn main() -> unit { break; }");
        assert!(diags.has_errors());
    }

    #[test]
    fn allows_break_inside_loop() {
        let diags = check("fn main() -> unit { while true { break; } }");
        assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    }

    #[test]
    fn detects_missing_and_unknown_struct_fields() {
        let diags = check(
            r#"
            struct Point { x: int, y: int }
            fn main() -> unit {
                let p = Point { x: 1, z: 2 };
            }
            "#,
        );
        assert!(diags.has_errors());
    }

    #[test]
    fn arrays_and_indexing_type_check() {
        let diags = check(
            r#"
            fn main() -> unit {
                let a = [1, 2, 3];
                let x = a[0] + 1;
                print(x);
            }
            "#,
        );
        assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    }
}
