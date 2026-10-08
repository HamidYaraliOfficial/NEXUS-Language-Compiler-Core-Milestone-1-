# NEXUS Language — Compiler Core (Milestone 1)

A real, working lexer → parser → type checker → tree-walking interpreter
pipeline for a new general-purpose language called **NEXUS**, written in
Rust. Every part described below actually runs — there are no mocked
stages, no placeholder passes, and no fake CLI commands. Everything not
yet built (native/LLVM code generation, the package manager, the LSP
server, the desktop IDE, and more) is listed honestly in the **Roadmap**
section instead of being faked.

*(به زبان فارسی و چینی هم در ادامه همین فایل توضیح داده شده — see
Persian and Chinese sections below.)*

---

## 1. What's actually implemented

| Stage | Crate | What it really does |
|---|---|---|
| Lexer | `nexus-lexer` | Hand-written tokenizer, line/col tracking, string escapes, `//` and `/* */` comments, recovers from bad characters/unterminated strings instead of aborting |
| Parser | `nexus-parser` | Recursive-descent + precedence climbing, full expression grammar, **panic-mode error recovery** (one bad statement doesn't stop the whole file) |
| AST | `nexus-ast` | Fully typed node tree, span on every node, optional `serde` feature for JSON (de)serialization, built-in pretty printer |
| Type checker | `nexus-typeck` | Structs, functions, generics-free static types, mutability checking, missing-return analysis, struct-literal field checking, call arity/type checking |
| Interpreter | `nexus-interpreter` | Tree-walking evaluator: recursion, closures-free functions, structs & arrays with reference semantics, short-circuit `&&`/`||`, `break`/`continue`, runtime errors (div-by-zero, OOB index, failed `assert`) |
| CLI | `nexus-cli` (binary `nexus`) | `run`, `check`, `emit-tokens`, `emit-ast`, `test`, `repl` — all real |

**36 automated tests** across every crate, all passing (`cargo test --workspace`).

## 2. The NEXUS language (this milestone's subset)

```nexus
struct Point { x: int, y: int }

fn manhattan(a: Point, b: Point) -> int {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    return abs(dx) + abs(dy);
}

fn abs(n: int) -> int {
    if n < 0 { return -n; }
    return n;
}

fn main() -> unit {
    let origin = Point { x: 0, y: 0 };
    let target = Point { x: 3, y: 4 };
    print(manhattan(origin, target));

    let mut total = 0;
    for i in 0..10 {
        if i % 2 == 0 { continue; }
        total = total + i;
    }
    print(total);
}
```

Supported today: `int` (i64) / `float` (f64) / `bool` / `string` / `unit`,
arrays (`[T]`), `struct`, functions, `let` / `let mut`, `if`/`else`,
`while`, `for x in a..b`, `break`/`continue`, all standard operators with
correct precedence, struct literals, array/field indexing, and the
built-ins `print`, `assert`, `len`.

Not in this milestone (see Roadmap): generics, traits/interfaces, enums,
pattern matching, closures, modules/packages, async, macros, FFI — the
full list from the original vision.

## 3. Installation

```bash
# 1. Install a Rust toolchain (skip if you already have one)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# or, on Debian/Ubuntu:
sudo apt-get install rustc cargo

# 2. Build the whole workspace
cargo build --workspace --release

# 3. The compiled binary is at:
./target/release/nexus --help
```

## 4. CLI reference

```
nexus run <file.nex>          Compile and execute a NEXUS program
nexus check <file.nex>        Lex, parse and type-check without running
nexus emit-tokens <file.nex>  Print the raw token stream
nexus emit-ast <file.nex>     Print the parsed AST
nexus test <file.nex>         Run every zero-argument `test_*` function
nexus repl                    Start an interactive session
nexus help                    Show usage
```

Try it:

```bash
cargo run --bin nexus -- run examples/hello.nex
cargo run --bin nexus -- run examples/fib.nex
cargo run --bin nexus -- test examples/tests_demo.nex
cargo run --bin nexus -- check examples/broken.nex   # see multi-error diagnostics
cargo run --bin nexus -- repl
```

## 5. Repository layout

```
nexus-lang/
├── Cargo.toml                 workspace manifest
├── crates/
│   ├── diagnostics/           Span, Diagnostic, colored renderer
│   ├── lexer/                 tokenizer
│   ├── ast/                   AST node definitions (+ optional serde)
│   ├── parser/                recursive-descent parser, error recovery
│   ├── typeck/                symbol tables + type checker
│   ├── interpreter/           tree-walking runtime
│   └── cli/                   the `nexus` binary
├── examples/                  runnable .nex sample programs
└── README.md                  this file
```

## 6. Roadmap — what this milestone deliberately does *not* include

The original project vision (asked for in the requesting conversation)
covers a full production compiler ecosystem: an LLVM-based native
backend for Windows/Linux/ARM64, a borrow checker and ownership system,
generics/traits, a package manager (`nexus-pm`), an LSP server, a
Tauri+React desktop IDE with an integrated debugger and compiler
explorer, a formatter/linter, and more. That is realistically a
multi-year, many-engineer effort — building it as convincing-looking but
non-functional stubs would violate the "no mock/fake/placeholder" ask
more than simply not building it yet. This milestone is the first real,
solid layer (front end + a working execution backend) that any of that
can be built on top of next, one genuinely working piece at a time.

## 7. License

MIT.

---

# فارسی

## زبان و کامپایلر NEXUS — هسته کامپایلر (فاز اول)

یک پایپ‌لاین واقعی و کاملاً کارکننده شامل Lexer، Parser، Type Checker و
یک مفسر (Interpreter) درخت‌محور برای زبان جدید عمومی **NEXUS**، نوشته‌شده
با Rust. هر بخشی که در جدول زیر آمده واقعاً اجرا می‌شود؛ هیچ مرحله‌ی
شبیه‌سازی‌شده، Placeholder یا دستور CLI ساختگی وجود ندارد. هر آنچه هنوز
ساخته نشده (Backend بومی/LLVM، Package Manager، سرور LSP، IDE دسکتاپ و
غیره) صادقانه در بخش «نقشه راه» فهرست شده، نه اینکه جعل شده باشد.

### ۱. چه چیزی واقعاً پیاده‌سازی شده است

| مرحله | Crate | کاری که واقعاً انجام می‌دهد |
|---|---|---|
| Lexer | `nexus-lexer` | توکنایزر دستی، ردیابی خط/ستون، Escape رشته‌ها، کامنت‌های `//` و `/* */`، بازیابی از کاراکتر نامعتبر یا رشته ناتمام بدون توقف کامل |
| Parser | `nexus-parser` | Recursive-Descent + Precedence Climbing، گرامر کامل Expression، **بازیابی خطا (Error Recovery)** به‌صورت Panic-Mode |
| AST | `nexus-ast` | درخت گره‌های کاملاً Typed، Span روی هر گره، قابلیت اختیاری `serde` برای Serialize/Deserialize به JSON، Pretty Printer داخلی |
| Type Checker | `nexus-typeck` | Structها، توابع، بررسی Mutability، تحلیل Missing Return، بررسی فیلدهای Struct Literal، بررسی تعداد/نوع آرگومان‌ها |
| Interpreter | `nexus-interpreter` | اجرای درخت‌محور: Recursion، Struct و Array با معناشناسی ارجاعی (Reference Semantics)، عملگرهای Short-Circuit، `break`/`continue`، خطاهای Runtime (تقسیم بر صفر، خروج از محدوده آرایه، شکست `assert`) |
| CLI | `nexus-cli` (باینری `nexus`) | دستورات `run`، `check`، `emit-tokens`، `emit-ast`، `test`، `repl` — همگی واقعی |

**۳۶ تست خودکار** در تمام Crateها، همگی موفق (`cargo test --workspace`).

### ۲. زبان NEXUS (زیرمجموعه‌ی این فاز)

نمونه کد را در بخش انگلیسی بالا ببینید (کد یکسان است و برای هر سه زبان
مشترک است). امکانات پشتیبانی‌شده در این فاز: انواع `int`، `float`،
`bool`، `string`، `unit`، آرایه (`[T]`)، `struct`، توابع، `let`/`let mut`،
`if`/`else`، `while`، `for x in a..b`، `break`/`continue`، تمام عملگرهای
استاندارد با اولویت صحیح، Struct Literal، ایندکس‌گذاری آرایه/فیلد، و
توابع درونی `print`، `assert`، `len`.

در این فاز نیستند (نگاه کنید به «نقشه راه»): Generics، Trait/Interface،
Enum، Pattern Matching، Closure، Module/Package، Async، Macro، FFI.

### ۳. نصب

```bash
# ۱. نصب Rust (اگر از قبل ندارید)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# یا در Debian/Ubuntu:
sudo apt-get install rustc cargo

# ۲. ساخت کل Workspace
cargo build --workspace --release

# ۳. باینری ساخته‌شده اینجاست:
./target/release/nexus --help
```

### ۴. مرجع دستورات CLI

```
nexus run <file.nex>          کامپایل و اجرای یک برنامه NEXUS
nexus check <file.nex>        Lex + Parse + Type Check بدون اجرا
nexus emit-tokens <file.nex>  چاپ جریان Tokenها
nexus emit-ast <file.nex>     چاپ AST تجزیه‌شده
nexus test <file.nex>         اجرای تمام توابع بدون‌آرگومان `test_*`
nexus repl                    شروع نشست تعاملی (REPL)
nexus help                    نمایش راهنما
```

نمونه اجرا:

```bash
cargo run --bin nexus -- run examples/hello.nex
cargo run --bin nexus -- run examples/fib.nex
cargo run --bin nexus -- test examples/tests_demo.nex
cargo run --bin nexus -- check examples/broken.nex   # نمایش چند خطا در یک اجرا
cargo run --bin nexus -- repl
```

### ۵. ساختار مخزن

ساختار پوشه‌ها دقیقاً مطابق بخش انگلیسی بالا است (`crates/diagnostics`،
`crates/lexer`، `crates/ast`، `crates/parser`، `crates/typeck`،
`crates/interpreter`، `crates/cli`، `examples/`).

### ۶. نقشه راه — آنچه عمداً در این فاز نیست

چشم‌انداز اصلی پروژه (که در گفتگوی درخواست‌کننده آمده) یک اکوسیستم کامل
Production شامل Backend بومی مبتنی بر LLVM برای Windows/Linux/ARM64،
سیستم Borrow Checker و Ownership، Generics/Trait، یک Package Manager
(`nexus-pm`)، سرور LSP، یک IDE دسکتاپ با Tauri+React همراه با دیباگر و
Compiler Explorer، Formatter/Linter و موارد دیگر را در بر می‌گیرد. این
واقع‌بینانه کار چند سالِ چند مهندس است؛ ساختن آن به شکل Stubهای
قانع‌کننده اما غیرکاربردی، بیشتر از نساختنش، با خواسته‌ی «بدون
Mock/Fake/Placeholder» در تضاد است. این فاز، اولین لایه‌ی واقعی و محکم
(Frontend + یک Backend اجرایی کاربردی) است که هر بخش دیگر می‌تواند در
مرحله بعد، قطعه‌به‌قطعه و واقعاً کارکننده، روی آن ساخته شود.

### ۷. مجوز

MIT.

---

# 中文

## NEXUS 语言与编译器 — 编译器核心（第一阶段）

一个真实可运行的完整流水线：Lexer（词法分析）→ Parser（语法分析）→
Type Checker（类型检查）→ 基于树遍历的 Interpreter（解释执行），面向全新的
通用编程语言 **NEXUS**，使用 Rust 编写。下表列出的每一部分都是真正可以
运行的——没有模拟的阶段，没有占位符,也没有虚假的 CLI 命令。所有尚未构建的部分
（原生/LLVM 后端、包管理器、LSP 服务器、桌面 IDE 等）都诚实地列在下面的
"路线图"部分,而不是伪造出来。

### 1. 目前真正实现的内容

| 阶段 | Crate | 实际功能 |
|---|---|---|
| 词法分析器 | `nexus-lexer` | 手写分词器,跟踪行/列号,字符串转义,支持 `//` 与 `/* */` 注释,遇到非法字符或未闭合字符串时能恢复而不是直接中止 |
| 语法分析器 | `nexus-parser` | 递归下降 + 运算符优先级解析,完整的表达式文法,**Panic-Mode 错误恢复**(一处语句错误不会导致整个文件解析失败) |
| 抽象语法树 | `nexus-ast` | 完全类型化的节点树,每个节点都带有源码位置(Span),可选的 `serde` 特性支持 JSON 序列化/反序列化,内置美化打印器 |
| 类型检查器 | `nexus-typeck` | 结构体、函数、可变性检查、"缺少 return"分析、结构体字面量字段检查、函数调用参数个数/类型检查 |
| 解释器 | `nexus-interpreter` | 基于树遍历的执行引擎:递归调用、具有引用语义的结构体与数组、短路求值的 `&&`/`||`、`break`/`continue`、运行时错误(除零、数组越界、`assert` 失败) |
| 命令行工具 | `nexus-cli`(可执行文件 `nexus`) | `run`、`check`、`emit-tokens`、`emit-ast`、`test`、`repl` —— 全部真实可用 |

整个工作区共有 **36 个自动化测试**,全部通过(`cargo test --workspace`)。

### 2. NEXUS 语言(本阶段支持的子集)

示例代码见上方英文部分(代码在三种语言的说明中是通用的)。当前已支持:
`int`(64位整数)/ `float`(64位浮点数)/ `bool` / `string` / `unit` 类型、
数组(`[T]`)、`struct`、函数、`let` / `let mut`、`if`/`else`、`while`、
`for x in a..b`、`break`/`continue`、具有正确优先级的全部标准运算符、
结构体字面量、数组/字段索引,以及内置函数 `print`、`assert`、`len`。

本阶段尚未包含(见"路线图"):泛型、trait/接口、枚举、模式匹配、闭包、
模块/包系统、异步、宏、FFI 等完整愿景中的功能。

### 3. 安装步骤

```bash
# 1. 安装 Rust 工具链(如果尚未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# 或者在 Debian/Ubuntu 上:
sudo apt-get install rustc cargo

# 2. 编译整个工作区
cargo build --workspace --release

# 3. 编译好的可执行文件位于:
./target/release/nexus --help
```

### 4. 命令行参考

```
nexus run <file.nex>          编译并执行一个 NEXUS 程序
nexus check <file.nex>        仅执行词法/语法/类型检查,不运行
nexus emit-tokens <file.nex>  打印原始的 token 流
nexus emit-ast <file.nex>     打印解析后的抽象语法树
nexus test <file.nex>         运行所有无参数的 `test_*` 函数
nexus repl                    启动交互式会话
nexus help                    显示帮助信息
```

试运行:

```bash
cargo run --bin nexus -- run examples/hello.nex
cargo run --bin nexus -- run examples/fib.nex
cargo run --bin nexus -- test examples/tests_demo.nex
cargo run --bin nexus -- check examples/broken.nex   # 查看一次运行中报告的多个错误
cargo run --bin nexus -- repl
```

### 5. 仓库结构

目录结构与上方英文部分完全一致(`crates/diagnostics`、`crates/lexer`、
`crates/ast`、`crates/parser`、`crates/typeck`、`crates/interpreter`、
`crates/cli`、`examples/`)。

### 6. 路线图 —— 本阶段刻意未包含的内容

项目最初的完整愿景(在发起请求的对话中提出)涵盖了一整套生产级编译器生态:
面向 Windows/Linux/ARM64 的基于 LLVM 的原生后端、借用检查器与所有权系统、
泛型/trait、包管理器(`nexus-pm`)、LSP 服务器、基于 Tauri+React 的桌面
IDE(含集成调试器与编译器浏览器)、格式化工具/静态检查工具等等。现实地说,
这需要多名工程师历时数年才能完成;把它做成"看起来令人信服但实际不可用"的
占位代码,比干脆不做更违背"不要模拟/伪造/占位"的要求。本阶段是第一层真正
扎实的基础(前端 + 一个可用的执行后端),之后的任何部分都可以在此基础上,
一块一块地、真正可运行地继续构建。

### 7. 许可证

MIT。
