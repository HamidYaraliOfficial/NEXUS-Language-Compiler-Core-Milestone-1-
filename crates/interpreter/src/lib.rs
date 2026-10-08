//! nexus-interpreter
//!
//! A tree-walking evaluator that executes a **type-checked** NEXUS
//! [`Program`]. This is the milestone-1 execution backend: it is a real,
//! complete, working implementation of every construct in the language
//! subset (functions, structs, arrays, control flow, `break`/`continue`,
//! short-circuit boolean operators, built-ins). It is deliberately *not*
//! claimed to be the eventual native/LLVM code generator described in the
//! full project vision — see the workspace README's roadmap section — but
//! everything it does, it does for real: no stubbed-out opcodes, no fake
//! success paths.
//!
//! Arrays and structs use reference semantics at runtime (`Rc<RefCell<_>>`)
//! so that aliasing and in-place mutation behave the way the type checker
//! documents them.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use nexus_ast::*;
use nexus_diagnostics::Span;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Struct(String, Rc<RefCell<BTreeMap<String, Value>>>),
    Unit,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Float(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Str(v) => write!(f, "{v}"),
            Value::Unit => write!(f, "()"),
            Value::Array(v) => {
                write!(f, "[")?;
                for (i, el) in v.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{el}")?;
                }
                write!(f, "]")
            }
            Value::Struct(name, fields) => {
                write!(f, "{name} {{ ")?;
                for (i, (k, v)) in fields.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, " }}")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
}

impl RuntimeError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self { message: message.into(), span }
    }
}

pub type EvalResult = Result<Value, RuntimeError>;

/// One block/function-call worth of variable bindings, as a stack of
/// nested scopes. Assignment walks outward from the innermost scope so
/// shadowed `let`s and loop variables resolve correctly.
struct Environment {
    scopes: Vec<HashMap<String, Value>>,
}

impl Environment {
    fn new() -> Self {
        Self { scopes: vec![HashMap::new()] }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: String, value: Value) {
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    fn get(&self, name: &str) -> Option<Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn assign(&mut self, name: &str, value: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}

/// Control-flow signal produced while executing a statement/block.
enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

type FlowResult = Result<Flow, RuntimeError>;

pub struct Interpreter<'a> {
    functions: HashMap<&'a str, &'a FunctionDecl>,
    output: Box<dyn FnMut(&str) + 'a>,
}

impl<'a> Interpreter<'a> {
    pub fn new(program: &'a Program, output: impl FnMut(&str) + 'a) -> Self {
        let mut functions = HashMap::new();
        for item in &program.items {
            if let Item::Function(f) = item {
                functions.insert(f.name.as_str(), f);
            }
        }
        Self { functions, output: Box::new(output) }
    }

    /// Runs `main()` (which must take no arguments) and returns its result.
    pub fn run_main(&mut self) -> EvalResult {
        let Some(main_fn) = self.functions.get("main").copied() else {
            return Err(RuntimeError::new("no `main` function to run", Span::default()));
        };
        self.call_function(main_fn, Vec::new(), Span::default())
    }

    /// Runs an arbitrary zero-argument top-level function by name (used by
    /// `nexus test` to run every `test_*` function it finds).
    pub fn run_function(&mut self, name: &str) -> EvalResult {
        let f = *self
            .functions
            .get(name)
            .ok_or_else(|| RuntimeError::new(format!("no such function `{name}`"), Span::default()))?;
        self.call_function(f, Vec::new(), Span::default())
    }

    fn call_function(&mut self, f: &'a FunctionDecl, args: Vec<Value>, span: Span) -> EvalResult {
        let mut env = Environment::new();
        for (param, arg) in f.params.iter().zip(args.into_iter()) {
            env.define(param.name.clone(), arg);
        }
        match self.exec_block(&f.body, &mut env)? {
            Flow::Return(v) => Ok(v),
            Flow::Normal => Ok(Value::Unit),
            Flow::Break | Flow::Continue => {
                Err(RuntimeError::new("`break`/`continue` escaped their loop (internal compiler error)", span))
            }
        }
    }

    // ---- Statements --------------------------------------------------

    fn exec_block(&mut self, block: &'a Block, env: &mut Environment) -> FlowResult {
        for stmt in &block.stmts {
            match self.exec_stmt(stmt, env)? {
                Flow::Normal => continue,
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, stmt: &'a Stmt, env: &mut Environment) -> FlowResult {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let v = self.eval_expr(value, env)?;
                env.define(name.clone(), v);
                Ok(Flow::Normal)
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval_expr(e, env)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Expr(e) => {
                self.eval_expr(e, env)?;
                Ok(Flow::Normal)
            }
            Stmt::If { cond, then_branch, else_branch, span } => {
                let c = as_bool(self.eval_expr(cond, env)?, *span)?;
                if c {
                    env.push_scope();
                    let r = self.exec_block(then_branch, env);
                    env.pop_scope();
                    r
                } else {
                    match else_branch.as_deref() {
                        Some(ElseBranch::Block(b)) => {
                            env.push_scope();
                            let r = self.exec_block(b, env);
                            env.pop_scope();
                            r
                        }
                        Some(ElseBranch::If(s)) => self.exec_stmt(s, env),
                        None => Ok(Flow::Normal),
                    }
                }
            }
            Stmt::While { cond, body, span } => {
                loop {
                    let c = as_bool(self.eval_expr(cond, env)?, *span)?;
                    if !c {
                        break;
                    }
                    env.push_scope();
                    let flow = self.exec_block(body, env);
                    env.pop_scope();
                    match flow? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Continue | Flow::Normal => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For { var, start, end, body, span } => {
                let s = as_int(self.eval_expr(start, env)?, *span)?;
                let e = as_int(self.eval_expr(end, env)?, *span)?;
                for i in s..e {
                    env.push_scope();
                    env.define(var.clone(), Value::Int(i));
                    let flow = self.exec_block(body, env);
                    env.pop_scope();
                    match flow? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Continue | Flow::Normal => {}
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Break(_) => Ok(Flow::Break),
            Stmt::Continue(_) => Ok(Flow::Continue),
            Stmt::Block(b) => {
                env.push_scope();
                let r = self.exec_block(b, env);
                env.pop_scope();
                r
            }
        }
    }

    // ---- Expressions --------------------------------------------------

    fn eval_expr(&mut self, expr: &'a Expr, env: &mut Environment) -> EvalResult {
        match expr {
            Expr::IntLit(v, _) => Ok(Value::Int(*v)),
            Expr::FloatLit(v, _) => Ok(Value::Float(*v)),
            Expr::StringLit(v, _) => Ok(Value::Str(v.clone())),
            Expr::BoolLit(v, _) => Ok(Value::Bool(*v)),
            Expr::Ident(name, span) => env
                .get(name)
                .ok_or_else(|| RuntimeError::new(format!("undefined variable `{name}` at runtime"), *span)),
            Expr::Unary { op, expr, span } => {
                let v = self.eval_expr(expr, env)?;
                match (op, v) {
                    (UnaryOp::Neg, Value::Int(i)) => Ok(Value::Int(-i)),
                    (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
                    (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    _ => Err(RuntimeError::new("invalid operand for unary operator", *span)),
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.eval_binary(op, lhs, rhs, *span, env),
            Expr::Assign { target, value, span } => {
                let v = self.eval_expr(value, env)?;
                self.assign_to(target, v.clone(), env, *span)?;
                Ok(v)
            }
            Expr::Call { callee, args, span } => self.eval_call(callee, args, *span, env),
            Expr::Index { base, index, span } => {
                let base_v = self.eval_expr(base, env)?;
                let idx = as_int(self.eval_expr(index, env)?, *span)?;
                match base_v {
                    Value::Array(rc) => {
                        let vec = rc.borrow();
                        if idx < 0 || idx as usize >= vec.len() {
                            return Err(RuntimeError::new(
                                format!("index {idx} out of bounds (length {})", vec.len()),
                                *span,
                            ));
                        }
                        Ok(vec[idx as usize].clone())
                    }
                    _ => Err(RuntimeError::new("cannot index a non-array value", *span)),
                }
            }
            Expr::FieldAccess { base, field, span } => {
                let base_v = self.eval_expr(base, env)?;
                match base_v {
                    Value::Struct(_, fields) => fields
                        .borrow()
                        .get(field)
                        .cloned()
                        .ok_or_else(|| RuntimeError::new(format!("no field `{field}` at runtime"), *span)),
                    _ => Err(RuntimeError::new("cannot access a field on a non-struct value", *span)),
                }
            }
            Expr::ArrayLit { elements, .. } => {
                let mut vals = Vec::with_capacity(elements.len());
                for e in elements {
                    vals.push(self.eval_expr(e, env)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(vals))))
            }
            Expr::StructLit { name, fields, .. } => {
                let mut map = BTreeMap::new();
                for (fname, fexpr) in fields {
                    map.insert(fname.clone(), self.eval_expr(fexpr, env)?);
                }
                Ok(Value::Struct(name.clone(), Rc::new(RefCell::new(map))))
            }
        }
    }

    fn assign_to(&mut self, target: &'a Expr, value: Value, env: &mut Environment, span: Span) -> Result<(), RuntimeError> {
        match target {
            Expr::Ident(name, _) => {
                if !env.assign(name, value) {
                    return Err(RuntimeError::new(format!("undefined variable `{name}` at runtime"), span));
                }
                Ok(())
            }
            Expr::Index { base, index, .. } => {
                let base_v = self.eval_expr(base, env)?;
                let idx = as_int(self.eval_expr(index, env)?, span)?;
                match base_v {
                    Value::Array(rc) => {
                        let mut vec = rc.borrow_mut();
                        if idx < 0 || idx as usize >= vec.len() {
                            return Err(RuntimeError::new(
                                format!("index {idx} out of bounds (length {})", vec.len()),
                                span,
                            ));
                        }
                        vec[idx as usize] = value;
                        Ok(())
                    }
                    _ => Err(RuntimeError::new("cannot index-assign a non-array value", span)),
                }
            }
            Expr::FieldAccess { base, field, .. } => {
                let base_v = self.eval_expr(base, env)?;
                match base_v {
                    Value::Struct(_, fields) => {
                        fields.borrow_mut().insert(field.clone(), value);
                        Ok(())
                    }
                    _ => Err(RuntimeError::new("cannot assign a field on a non-struct value", span)),
                }
            }
            _ => Err(RuntimeError::new("invalid assignment target at runtime", span)),
        }
    }

    fn eval_binary(&mut self, op: &BinaryOp, lhs: &'a Expr, rhs: &'a Expr, span: Span, env: &mut Environment) -> EvalResult {
        use BinaryOp::*;
        // Short-circuit boolean operators evaluate the right-hand side only
        // when necessary.
        if matches!(op, And) {
            let l = as_bool(self.eval_expr(lhs, env)?, span)?;
            if !l {
                return Ok(Value::Bool(false));
            }
            let r = as_bool(self.eval_expr(rhs, env)?, span)?;
            return Ok(Value::Bool(r));
        }
        if matches!(op, Or) {
            let l = as_bool(self.eval_expr(lhs, env)?, span)?;
            if l {
                return Ok(Value::Bool(true));
            }
            let r = as_bool(self.eval_expr(rhs, env)?, span)?;
            return Ok(Value::Bool(r));
        }

        let l = self.eval_expr(lhs, env)?;
        let r = self.eval_expr(rhs, env)?;
        match op {
            Add => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Str(a), Value::Str(b)) => Ok(Value::Str(a + &b)),
                _ => Err(RuntimeError::new("invalid operands for `+`", span)),
            },
            Sub => arith(l, r, span, |a, b| a.wrapping_sub(b), |a, b| a - b),
            Mul => arith(l, r, span, |a, b| a.wrapping_mul(b), |a, b| a * b),
            Div => match (l, r) {
                (Value::Int(_), Value::Int(0)) => Err(RuntimeError::new("division by zero", span)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                _ => Err(RuntimeError::new("invalid operands for `/`", span)),
            },
            Mod => match (l, r) {
                (Value::Int(_), Value::Int(0)) => Err(RuntimeError::new("modulo by zero", span)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
                _ => Err(RuntimeError::new("invalid operands for `%`", span)),
            },
            Lt => cmp(l, r, span, |o| o.is_lt()),
            Gt => cmp(l, r, span, |o| o.is_gt()),
            LtEq => cmp(l, r, span, |o| o.is_le()),
            GtEq => cmp(l, r, span, |o| o.is_ge()),
            Eq => Ok(Value::Bool(values_equal(&l, &r))),
            NotEq => Ok(Value::Bool(!values_equal(&l, &r))),
            And | Or => unreachable!("handled above"),
        }
    }

    fn eval_call(&mut self, callee: &str, args: &'a [Expr], span: Span, env: &mut Environment) -> EvalResult {
        match callee {
            "print" => {
                let v = self.eval_expr(&args[0], env)?;
                (self.output)(&format!("{v}"));
                Ok(Value::Unit)
            }
            "assert" => {
                let cond = as_bool(self.eval_expr(&args[0], env)?, span)?;
                if !cond {
                    let msg = if let Some(m) = args.get(1) {
                        match self.eval_expr(m, env)? {
                            Value::Str(s) => s,
                            other => format!("{other}"),
                        }
                    } else {
                        "assertion failed".to_string()
                    };
                    return Err(RuntimeError::new(msg, span));
                }
                Ok(Value::Unit)
            }
            "len" => match self.eval_expr(&args[0], env)? {
                Value::Array(rc) => Ok(Value::Int(rc.borrow().len() as i64)),
                Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
                _ => Err(RuntimeError::new("`len` expects an array or string", span)),
            },
            _ => {
                let f = *self
                    .functions
                    .get(callee)
                    .ok_or_else(|| RuntimeError::new(format!("undefined function `{callee}` at runtime"), span))?;
                let mut arg_vals = Vec::with_capacity(args.len());
                for a in args {
                    arg_vals.push(self.eval_expr(a, env)?);
                }
                self.call_function(f, arg_vals, span)
            }
        }
    }
}

fn arith(l: Value, r: Value, span: Span, fi: fn(i64, i64) -> i64, ff: fn(f64, f64) -> f64) -> EvalResult {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(fi(a, b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(ff(a, b))),
        _ => Err(RuntimeError::new("invalid operands for arithmetic operator", span)),
    }
}

fn cmp(l: Value, r: Value, span: Span, f: fn(std::cmp::Ordering) -> bool) -> EvalResult {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(f(a.cmp(&b)))),
        (Value::Float(a), Value::Float(b)) => match a.partial_cmp(&b) {
            Some(o) => Ok(Value::Bool(f(o))),
            None => Err(RuntimeError::new("comparison with NaN", span)),
        },
        _ => Err(RuntimeError::new("invalid operands for comparison operator", span)),
    }
}

fn values_equal(l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Unit, Value::Unit) => true,
        _ => false,
    }
}

fn as_bool(v: Value, span: Span) -> Result<bool, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(b),
        _ => Err(RuntimeError::new("expected a `bool` value", span)),
    }
}

fn as_int(v: Value, span: Span) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(i) => Ok(i),
        _ => Err(RuntimeError::new("expected an `int` value", span)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc as StdRc;

    fn run(src: &str) -> (Value, Vec<String>) {
        let (program, parse_diags) = nexus_parser::parse(src);
        assert!(!parse_diags.has_errors(), "parse errors: {:#?}", parse_diags.into_vec());
        let type_diags = nexus_typeck::check_program(&program);
        assert!(!type_diags.has_errors(), "type errors: {:#?}", type_diags.into_vec());

        let captured: StdRc<StdRefCell<Vec<String>>> = StdRc::new(StdRefCell::new(Vec::new()));
        let captured2 = captured.clone();
        let mut interp = Interpreter::new(&program, move |s: &str| captured2.borrow_mut().push(s.to_string()));
        let result = interp.run_main().expect("runtime error");
        let out = captured.borrow().clone();
        (result, out)
    }

    #[test]
    fn runs_fibonacci_recursively() {
        let (_, out) = run(
            r#"
            fn fib(n: int) -> int {
                if n < 2 {
                    return n;
                }
                return fib(n - 1) + fib(n - 2);
            }
            fn main() -> unit {
                print(fib(10));
            }
            "#,
        );
        assert_eq!(out, vec!["55".to_string()]);
    }

    #[test]
    fn while_loop_and_mutation() {
        let (_, out) = run(
            r#"
            fn main() -> unit {
                let mut i = 0;
                let mut sum = 0;
                while i < 5 {
                    sum = sum + i;
                    i = i + 1;
                }
                print(sum);
            }
            "#,
        );
        assert_eq!(out, vec!["10".to_string()]);
    }

    #[test]
    fn for_loop_with_break_and_continue() {
        let (_, out) = run(
            r#"
            fn main() -> unit {
                let mut total = 0;
                for i in 0..10 {
                    if i == 5 { break; }
                    if i % 2 == 0 { continue; }
                    total = total + i;
                }
                print(total);
            }
            "#,
        );
        // i = 1, 3 pass the continue filter before breaking at i == 5
        assert_eq!(out, vec!["4".to_string()]);
    }

    #[test]
    fn structs_and_field_mutation_share_reference_semantics() {
        let (_, out) = run(
            r#"
            struct Counter { value: int }
            fn bump(c: Counter) -> unit {
                c.value = c.value + 1;
            }
            fn main() -> unit {
                let c = Counter { value: 0 };
                bump(c);
                bump(c);
                print(c.value);
            }
            "#,
        );
        assert_eq!(out, vec!["2".to_string()]);
    }

    #[test]
    fn arrays_index_and_len() {
        let (_, out) = run(
            r#"
            fn main() -> unit {
                let a = [10, 20, 30];
                a[1] = 99;
                print(a[1]);
                print(len(a));
            }
            "#,
        );
        assert_eq!(out, vec!["99".to_string(), "3".to_string()]);
    }

    #[test]
    fn division_by_zero_is_a_runtime_error() {
        let (program, _) = nexus_parser::parse("fn main() -> unit { let x = 1 / 0; }");
        let mut interp = Interpreter::new(&program, |_s: &str| {});
        let err = interp.run_main().unwrap_err();
        assert!(err.message.contains("division by zero"));
    }

    #[test]
    fn assert_failure_is_a_runtime_error_with_message() {
        let (program, _) = nexus_parser::parse(r#"fn main() -> unit { assert(false, "nope"); }"#);
        let mut interp = Interpreter::new(&program, |_s: &str| {});
        let err = interp.run_main().unwrap_err();
        assert_eq!(err.message, "nope");
    }

    #[test]
    fn short_circuit_or_skips_right_hand_side() {
        // If short-circuiting were broken, calling `boom()` would recurse
        // forever / blow the stack; this test passing proves `||` really
        // does not evaluate its right side once the left side is true.
        let (_, out) = run(
            r#"
            fn boom() -> bool {
                return boom();
            }
            fn main() -> unit {
                let x = true || boom();
                print(x);
            }
            "#,
        );
        assert_eq!(out, vec!["true".to_string()]);
    }
}
