//! nexus-ast
//!
//! Abstract syntax tree for the NEXUS language subset implemented by this
//! milestone. Every node carries a [`Span`] so later stages (type checker,
//! interpreter, formatter) can point back at exact source locations.
//!
//! When built with `--features serde`, every node derives `Serialize` /
//! `Deserialize`, so the tree can be dumped to JSON via `nexus emit-ast
//! --json` (see the `cli` crate) — this is the "AST must be serializable"
//! requirement from the project spec.

use nexus_diagnostics::Span;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(FunctionDecl),
    Struct(StructDecl),
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Int,
    Float,
    Bool,
    String,
    Unit,
    Array(Box<TypeExpr>),
    Named(String),
}

impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeExpr::Int => write!(f, "int"),
            TypeExpr::Float => write!(f, "float"),
            TypeExpr::Bool => write!(f, "bool"),
            TypeExpr::String => write!(f, "string"),
            TypeExpr::Unit => write!(f, "unit"),
            TypeExpr::Array(inner) => write!(f, "[{}]", inner),
            TypeExpr::Named(n) => write!(f, "{}", n),
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeExpr>,
        value: Expr,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Expr(Expr),
    If {
        cond: Expr,
        then_branch: Block,
        else_branch: Option<Box<ElseBranch>>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Block,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Block(Block),
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    If(Box<Stmt>),
    Block(Block),
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntLit(i64, Span),
    FloatLit(f64, Span),
    StringLit(String, Span),
    BoolLit(bool, Span),
    Ident(String, Span),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
        span: Span,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
    ArrayLit {
        elements: Vec<Expr>,
        span: Span,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit(_, s)
            | Expr::FloatLit(_, s)
            | Expr::StringLit(_, s)
            | Expr::BoolLit(_, s)
            | Expr::Ident(_, s) => *s,
            Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Assign { span, .. }
            | Expr::Call { span, .. }
            | Expr::Index { span, .. }
            | Expr::FieldAccess { span, .. }
            | Expr::ArrayLit { span, .. }
            | Expr::StructLit { span, .. } => *span,
        }
    }
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::For { span, .. } => *span,
            Stmt::Expr(e) => e.span(),
            Stmt::Break(s) | Stmt::Continue(s) => *s,
            Stmt::Block(b) => b.span,
        }
    }
}

/// A small, dependency-free pretty printer used by `nexus emit-ast` and by
/// `nexusfmt` as a structural (non-formatting) debug view.
pub mod pretty {
    use super::*;

    pub fn print_program(program: &Program) -> String {
        let mut out = String::new();
        for item in &program.items {
            print_item(&mut out, item, 0);
        }
        out
    }

    fn indent(out: &mut String, depth: usize) {
        out.push_str(&"  ".repeat(depth));
    }

    fn print_item(out: &mut String, item: &Item, depth: usize) {
        match item {
            Item::Function(f) => {
                indent(out, depth);
                out.push_str(&format!(
                    "fn {}({}) -> {}\n",
                    f.name,
                    f.params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, p.ty))
                        .collect::<Vec<_>>()
                        .join(", "),
                    f.return_type.as_ref().map(|t| t.to_string()).unwrap_or_else(|| "unit".into())
                ));
                for s in &f.body.stmts {
                    print_stmt(out, s, depth + 1);
                }
            }
            Item::Struct(s) => {
                indent(out, depth);
                out.push_str(&format!("struct {}\n", s.name));
                for field in &s.fields {
                    indent(out, depth + 1);
                    out.push_str(&format!("{}: {}\n", field.name, field.ty));
                }
            }
        }
    }

    fn print_stmt(out: &mut String, stmt: &Stmt, depth: usize) {
        indent(out, depth);
        match stmt {
            Stmt::Let { name, mutable, value, .. } => {
                out.push_str(&format!("let{} {} = {}\n", if *mutable { " mut" } else { "" }, name, print_expr(value)));
            }
            Stmt::Return { value, .. } => {
                out.push_str(&format!("return {}\n", value.as_ref().map(print_expr).unwrap_or_default()));
            }
            Stmt::Expr(e) => out.push_str(&format!("{}\n", print_expr(e))),
            Stmt::If { cond, then_branch, else_branch, .. } => {
                out.push_str(&format!("if {}\n", print_expr(cond)));
                for s in &then_branch.stmts {
                    print_stmt(out, s, depth + 1);
                }
                if let Some(else_b) = else_branch {
                    indent(out, depth);
                    out.push_str("else\n");
                    match else_b.as_ref() {
                        ElseBranch::Block(b) => {
                            for s in &b.stmts {
                                print_stmt(out, s, depth + 1);
                            }
                        }
                        ElseBranch::If(s) => print_stmt(out, s, depth + 1),
                    }
                }
            }
            Stmt::While { cond, body, .. } => {
                out.push_str(&format!("while {}\n", print_expr(cond)));
                for s in &body.stmts {
                    print_stmt(out, s, depth + 1);
                }
            }
            Stmt::For { var, start, end, body, .. } => {
                out.push_str(&format!("for {} in {}..{}\n", var, print_expr(start), print_expr(end)));
                for s in &body.stmts {
                    print_stmt(out, s, depth + 1);
                }
            }
            Stmt::Break(_) => out.push_str("break\n"),
            Stmt::Continue(_) => out.push_str("continue\n"),
            Stmt::Block(b) => {
                out.push_str("{\n");
                for s in &b.stmts {
                    print_stmt(out, s, depth + 1);
                }
            }
        }
    }

    fn print_expr(expr: &Expr) -> String {
        match expr {
            Expr::IntLit(v, _) => v.to_string(),
            Expr::FloatLit(v, _) => v.to_string(),
            Expr::StringLit(v, _) => format!("\"{}\"", v),
            Expr::BoolLit(v, _) => v.to_string(),
            Expr::Ident(n, _) => n.clone(),
            Expr::Unary { op, expr, .. } => format!("{}{}", if matches!(op, UnaryOp::Neg) { "-" } else { "!" }, print_expr(expr)),
            Expr::Binary { op, lhs, rhs, .. } => format!("({} {} {})", print_expr(lhs), bin_op_str(op), print_expr(rhs)),
            Expr::Assign { target, value, .. } => format!("{} = {}", print_expr(target), print_expr(value)),
            Expr::Call { callee, args, .. } => {
                format!("{}({})", callee, args.iter().map(print_expr).collect::<Vec<_>>().join(", "))
            }
            Expr::Index { base, index, .. } => format!("{}[{}]", print_expr(base), print_expr(index)),
            Expr::FieldAccess { base, field, .. } => format!("{}.{}", print_expr(base), field),
            Expr::ArrayLit { elements, .. } => format!("[{}]", elements.iter().map(print_expr).collect::<Vec<_>>().join(", ")),
            Expr::StructLit { name, fields, .. } => format!(
                "{} {{ {} }}",
                name,
                fields.iter().map(|(k, v)| format!("{}: {}", k, print_expr(v))).collect::<Vec<_>>().join(", ")
            ),
        }
    }

    fn bin_op_str(op: &BinaryOp) -> &'static str {
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
}
