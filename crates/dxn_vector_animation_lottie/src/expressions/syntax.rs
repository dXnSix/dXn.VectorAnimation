//! The JavaScript subset expressions are written in: tokens, then a tree of
//! statements and expressions. Sources, nesting and trees are bounded, so
//! reading, running and dropping a tree never runs deep.

use std::rc::Rc;

/// The longest expression read, in bytes.
const MAX_SOURCE: usize = 256 * 1024;
/// How deep statements, parentheses and operands nest while reading.
const MAX_NEST: usize = 128;
/// How deep one expression's tree goes (chains of operators included).
const MAX_TREE: u16 = 160;

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    InstanceOf,
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnOp {
    Neg,
    Plus,
    Not,
    BitNot,
    TypeOf,
    Void,
}

/// A short-circuiting operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Logic {
    And,
    Or,
    Nullish,
}

/// An expression, with the depth of its tree.
#[derive(Debug)]
pub(super) struct Expr {
    pub(super) kind: Kind,
    depth: u16,
}

#[derive(Debug)]
pub(super) enum Kind {
    Num(f64),
    Str(Rc<str>),
    Bool(bool),
    Null,
    Ident(Rc<str>),
    Array(Vec<Expr>),
    Func(Rc<Func>),
    Unary(UnOp, Box<Expr>),
    /// `++x`, `x--`, …
    Update {
        increment: bool,
        prefix: bool,
        target: Box<Expr>,
    },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Logical(Logic, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `target = value`, or `target op= value`.
    Assign(Option<BinOp>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, Rc<str>),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    /// `a, b`: each in turn, the last one's value.
    Seq(Vec<Expr>),
}

/// A function declared or written as an expression.
#[derive(Debug)]
pub(super) struct Func {
    pub(super) name: Option<Rc<str>>,
    pub(super) params: Vec<Rc<str>>,
    pub(super) body: Vec<Stmt>,
}

#[derive(Debug)]
pub(super) enum Stmt {
    Var(Vec<(Rc<str>, Option<Expr>)>),
    Expr(Expr),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For {
        init: Option<Box<Stmt>>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Block(Vec<Stmt>),
    Return(Option<Expr>),
    Break,
    Continue,
    Func(Rc<Func>),
    Try {
        body: Vec<Stmt>,
        param: Option<Rc<str>>,
        catch: Option<Vec<Stmt>>,
        finally: Option<Vec<Stmt>>,
    },
    Throw(Expr),
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Empty,
}

/// Reads the expression `source`; the error names what was not understood.
pub(super) fn parse(source: &str) -> Result<Vec<Stmt>, String> {
    if source.len() > MAX_SOURCE {
        return Err("an expression over 256 KB".into());
    }
    let mut parser = Parser {
        tokens: tokenize(source)?,
        pos: 0,
        nest: 0,
    };
    let mut body = Vec::new();
    while parser.peek() != &Tok::End {
        body.push(parser.statement()?);
    }
    Ok(body)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(Rc<str>),
    Name(Rc<str>),
    Punct(&'static str),
    End,
}

#[derive(Debug)]
struct Token {
    tok: Tok,
    /// A line break before it (where a statement may end without `;`).
    newline: bool,
}

/// Punctuators, longest first.
const PUNCTS: [&str; 52] = [
    ">>>=", "===", "!==", ">>>", "**=", "<<=", ">>=", "...", "==", "!=", "<=", ">=", "&&", "||",
    "??", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**", "<<", ">>", "=>", "{",
    "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~",
    "?", ":", "=",
];

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut newline = false;
    let is_break = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    while let Some(&c) = chars.get(i) {
        let next = chars.get(i + 1).copied();
        if is_break(c) {
            newline = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() || c == '\u{feff}' {
            i += 1;
            continue;
        }
        if c == '/' && next == Some('/') {
            while chars.get(i).is_some_and(|c| !is_break(*c)) {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            loop {
                match (chars.get(i), chars.get(i + 1)) {
                    (Some('*'), Some('/')) => {
                        i += 2;
                        break;
                    }
                    (Some(c), _) => {
                        newline |= is_break(*c);
                        i += 1;
                    }
                    (None, _) => return Err("an unclosed comment".into()),
                }
            }
            continue;
        }
        let tok = if c.is_ascii_digit() || (c == '.' && next.is_some_and(|n| n.is_ascii_digit())) {
            number(&chars, &mut i)?
        } else if c == '"' || c == '\'' {
            string(&chars, &mut i)?
        } else if c == '`' {
            return Err("template strings".into());
        } else if c == '.' {
            i += 1;
            Tok::Punct(".")
        } else if is_name_start(c) {
            let start = i;
            while chars.get(i).is_some_and(|c| is_name_part(*c)) {
                i += 1;
            }
            Tok::Name(chars[start..i].iter().collect::<String>().into())
        } else {
            let Some(p) = PUNCTS.iter().find(|p| {
                p.chars()
                    .enumerate()
                    .all(|(k, pc)| chars.get(i + k) == Some(&pc))
            }) else {
                return Err(format!("the character `{c}`"));
            };
            i += p.len();
            Tok::Punct(p)
        };
        tokens.push(Token { tok, newline });
        newline = false;
    }
    tokens.push(Token {
        tok: Tok::End,
        newline: true,
    });
    Ok(tokens)
}

fn is_name_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

fn is_name_part(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c.is_numeric()
}

fn number(chars: &[char], i: &mut usize) -> Result<Tok, String> {
    let start = *i;
    let at = |k: usize| chars.get(k).copied();
    if at(*i) == Some('0') && matches!(at(*i + 1), Some('x' | 'X')) {
        *i += 2;
        let mut n = 0.0_f64;
        let mut digits = 0;
        while let Some(d) = at(*i).and_then(|c| c.to_digit(16)) {
            n = n * 16.0 + f64::from(d);
            digits += 1;
            *i += 1;
        }
        if digits == 0 {
            return Err("a hexadecimal number without digits".into());
        }
        return Ok(Tok::Num(n));
    }
    while at(*i).is_some_and(|c| c.is_ascii_digit()) {
        *i += 1;
    }
    if at(*i) == Some('.') {
        *i += 1;
        while at(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
    }
    if matches!(at(*i), Some('e' | 'E')) {
        let mut k = *i + 1;
        if matches!(at(k), Some('+' | '-')) {
            k += 1;
        }
        if at(k).is_some_and(|c| c.is_ascii_digit()) {
            *i = k;
            while at(*i).is_some_and(|c| c.is_ascii_digit()) {
                *i += 1;
            }
        }
    }
    if at(*i).is_some_and(is_name_start) {
        return Err("a number followed by a name".into());
    }
    let text: String = chars[start..*i].iter().collect();
    text.parse()
        .map(Tok::Num)
        .map_err(|_| format!("the number `{text}`"))
}

fn string(chars: &[char], i: &mut usize) -> Result<Tok, String> {
    let quote = chars[*i];
    *i += 1;
    let mut out = String::new();
    loop {
        let Some(&c) = chars.get(*i) else {
            return Err("an unclosed string".into());
        };
        *i += 1;
        match c {
            c if c == quote => break,
            '\n' | '\r' => return Err("an unclosed string".into()),
            '\\' => {
                let Some(&e) = chars.get(*i) else {
                    return Err("an unclosed string".into());
                };
                *i += 1;
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'v' => out.push('\u{b}'),
                    '0' => out.push('\0'),
                    '\r' | '\n' => {}
                    'x' | 'u' => {
                        let len = if e == 'x' { 2 } else { 4 };
                        let hex: String = chars.iter().skip(*i).take(len).collect();
                        let code = u32::from_str_radix(&hex, 16)
                            .ok()
                            .filter(|_| hex.len() == len)
                            .ok_or("a bad escape in a string")?;
                        *i += len;
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    Ok(Tok::Str(out.into()))
}

/// Words that cannot name a variable.
const RESERVED: [&str; 31] = [
    "var",
    "let",
    "const",
    "if",
    "else",
    "for",
    "while",
    "do",
    "return",
    "break",
    "continue",
    "switch",
    "case",
    "default",
    "try",
    "catch",
    "finally",
    "throw",
    "class",
    "new",
    "delete",
    "in",
    "instanceof",
    "typeof",
    "void",
    "with",
    "yield",
    "import",
    "export",
    "extends",
    "super",
];

fn node(kind: Kind) -> Result<Expr, String> {
    let child = |e: &Expr| e.depth;
    let children = match &kind {
        Kind::Num(_)
        | Kind::Str(_)
        | Kind::Bool(_)
        | Kind::Null
        | Kind::Ident(_)
        | Kind::Func(_) => 0,
        Kind::Array(list) | Kind::Seq(list) => list.iter().map(child).max().unwrap_or(0),
        Kind::Unary(_, a) | Kind::Update { target: a, .. } | Kind::Member(a, _) => a.depth,
        Kind::Binary(_, a, b)
        | Kind::Logical(_, a, b)
        | Kind::Assign(_, a, b)
        | Kind::Index(a, b) => a.depth.max(b.depth),
        Kind::Cond(a, b, c) => a.depth.max(b.depth).max(c.depth),
        Kind::Call(f, args) => args.iter().map(child).fold(f.depth, u16::max),
    };
    let depth = children + 1;
    if depth > MAX_TREE {
        return Err("an expression nested too deeply".into());
    }
    Ok(Expr { kind, depth })
}

fn boxed(e: Expr) -> Box<Expr> {
    Box::new(e)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    nest: usize,
}

type Parsed<T> = Result<T, String>;

impl Parser {
    fn peek(&self) -> &Tok {
        self.tokens.get(self.pos).map_or(&Tok::End, |t| &t.tok)
    }

    fn newline_before(&self) -> bool {
        self.tokens.get(self.pos).is_none_or(|t| t.newline)
    }

    fn next(&mut self) -> Tok {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if &**n == w)
    }

    fn eat(&mut self, p: &str) -> bool {
        let found = self.is(p);
        if found {
            self.pos += 1;
        }
        found
    }

    fn expect(&mut self, p: &str) -> Parsed<()> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(format!("`{p}` expected, {} found", describe(self.peek())))
        }
    }

    fn name(&mut self) -> Parsed<Rc<str>> {
        match self.next() {
            Tok::Name(n) => Ok(n),
            tok => Err(format!("a name expected, {} found", describe(&tok))),
        }
    }

    fn variable_name(&mut self) -> Parsed<Rc<str>> {
        if self.is("[") || self.is("{") {
            return Err("destructuring".into());
        }
        let n = self.name()?;
        if RESERVED.contains(&&*n) {
            return Err(format!("`{n}` as a name"));
        }
        Ok(n)
    }

    fn enter(&mut self) -> Parsed<()> {
        self.nest += 1;
        if self.nest > MAX_NEST {
            return Err("an expression nested too deeply".into());
        }
        Ok(())
    }

    /// The end of a statement: `;`, or a line break, `}` or the end
    /// before the next token.
    fn semicolon(&mut self) -> Parsed<()> {
        if self.eat(";") || self.is("}") || self.newline_before() {
            Ok(())
        } else {
            Err(format!("`;` expected, {} found", describe(self.peek())))
        }
    }

    fn statement(&mut self) -> Parsed<Stmt> {
        self.enter()?;
        let s = self.statement_inner();
        self.nest -= 1;
        s
    }

    fn statement_inner(&mut self) -> Parsed<Stmt> {
        if self.eat("{") {
            return Ok(Stmt::Block(self.block_rest()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        let word = match self.peek() {
            Tok::Name(n) => n.clone(),
            _ => return self.expression_statement(),
        };
        match &*word {
            "var" | "let" | "const" => {
                self.next();
                let s = self.declarations()?;
                self.semicolon()?;
                Ok(s)
            }
            "if" => {
                self.next();
                self.expect("(")?;
                let test = self.expression()?;
                self.expect(")")?;
                let yes = Box::new(self.statement()?);
                let no = if self.is_word("else") {
                    self.next();
                    Some(Box::new(self.statement()?))
                } else {
                    None
                };
                Ok(Stmt::If(test, yes, no))
            }
            "for" => self.for_statement(),
            "while" => {
                self.next();
                self.expect("(")?;
                let test = self.expression()?;
                self.expect(")")?;
                Ok(Stmt::While(test, Box::new(self.statement()?)))
            }
            "do" => {
                self.next();
                let body = Box::new(self.statement()?);
                if !self.is_word("while") {
                    return Err(format!("`while` expected, {} found", describe(self.peek())));
                }
                self.next();
                self.expect("(")?;
                let test = self.expression()?;
                self.expect(")")?;
                self.eat(";");
                Ok(Stmt::DoWhile(body, test))
            }
            "function" => {
                self.next();
                let f = self.function_rest(true)?;
                Ok(Stmt::Func(f))
            }
            "return" => {
                self.next();
                let value = if self.is(";") || self.is("}") || self.newline_before() {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.semicolon()?;
                Ok(Stmt::Return(value))
            }
            "break" | "continue" => {
                self.next();
                if matches!(self.peek(), Tok::Name(_)) && !self.newline_before() {
                    return Err("labels".into());
                }
                self.semicolon()?;
                Ok(if &*word == "break" {
                    Stmt::Break
                } else {
                    Stmt::Continue
                })
            }
            "throw" => {
                self.next();
                let value = self.expression()?;
                self.semicolon()?;
                Ok(Stmt::Throw(value))
            }
            "try" => self.try_statement(),
            "switch" => self.switch_statement(),
            "class" | "import" | "export" | "with" | "yield" | "async" | "await" | "debugger" => {
                Err(format!("`{word}`"))
            }
            _ => self.expression_statement(),
        }
    }

    fn expression_statement(&mut self) -> Parsed<Stmt> {
        let e = self.expression()?;
        self.semicolon()?;
        Ok(Stmt::Expr(e))
    }

    fn block_rest(&mut self) -> Parsed<Vec<Stmt>> {
        let mut body = Vec::new();
        while !self.eat("}") {
            if self.peek() == &Tok::End {
                return Err("`}` expected, the end found".into());
            }
            body.push(self.statement()?);
        }
        Ok(body)
    }

    fn block(&mut self) -> Parsed<Vec<Stmt>> {
        self.expect("{")?;
        self.block_rest()
    }

    fn declarations(&mut self) -> Parsed<Stmt> {
        let mut list = Vec::new();
        loop {
            let name = self.variable_name()?;
            let init = if self.eat("=") {
                Some(self.assign()?)
            } else {
                None
            };
            list.push((name, init));
            if !self.eat(",") {
                break;
            }
        }
        Ok(Stmt::Var(list))
    }

    fn for_statement(&mut self) -> Parsed<Stmt> {
        self.next();
        self.expect("(")?;
        let init = if self.is(";") {
            None
        } else if self.is_word("var") || self.is_word("let") || self.is_word("const") {
            self.next();
            Some(Box::new(self.declarations()?))
        } else {
            Some(Box::new(Stmt::Expr(self.expression()?)))
        };
        if self.is_word("in") || self.is_word("of") {
            return Err("`for … in` and `for … of`".into());
        }
        self.expect(";")?;
        let test = if self.is(";") {
            None
        } else {
            Some(self.expression()?)
        };
        self.expect(";")?;
        let update = if self.is(")") {
            None
        } else {
            Some(self.expression()?)
        };
        self.expect(")")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt::For {
            init,
            test,
            update,
            body,
        })
    }

    fn try_statement(&mut self) -> Parsed<Stmt> {
        self.next();
        let body = self.block()?;
        let (mut param, mut catch, mut finally) = (None, None, None);
        if self.is_word("catch") {
            self.next();
            if self.eat("(") {
                param = Some(self.variable_name()?);
                self.expect(")")?;
            }
            catch = Some(self.block()?);
        }
        if self.is_word("finally") {
            self.next();
            finally = Some(self.block()?);
        }
        if catch.is_none() && finally.is_none() {
            return Err("`try` without `catch` or `finally`".into());
        }
        Ok(Stmt::Try {
            body,
            param,
            catch,
            finally,
        })
    }

    fn switch_statement(&mut self) -> Parsed<Stmt> {
        self.next();
        self.expect("(")?;
        let value = self.expression()?;
        self.expect(")")?;
        self.expect("{")?;
        let mut cases: Vec<(Option<Expr>, Vec<Stmt>)> = Vec::new();
        loop {
            if self.eat("}") {
                break;
            }
            let test = if self.is_word("case") {
                self.next();
                Some(self.expression()?)
            } else if self.is_word("default") {
                self.next();
                None
            } else {
                return Err(format!("`case` expected, {} found", describe(self.peek())));
            };
            self.expect(":")?;
            let mut body = Vec::new();
            while !(self.is_word("case") || self.is_word("default") || self.is("}")) {
                if self.peek() == &Tok::End {
                    return Err("`}` expected, the end found".into());
                }
                body.push(self.statement()?);
            }
            cases.push((test, body));
        }
        Ok(Stmt::Switch(value, cases))
    }

    /// A function after `function`: its name (required for declarations),
    /// parameters and body.
    fn function_rest(&mut self, declaration: bool) -> Parsed<Rc<Func>> {
        let name = if matches!(self.peek(), Tok::Name(_)) {
            Some(self.variable_name()?)
        } else if declaration {
            return Err("a function without a name".into());
        } else {
            None
        };
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.eat(")") {
            if self.is("...") {
                return Err("rest parameters".into());
            }
            params.push(self.variable_name()?);
            if self.is("=") {
                return Err("default parameters".into());
            }
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        let body = self.block()?;
        Ok(Rc::new(Func { name, params, body }))
    }

    fn expression(&mut self) -> Parsed<Expr> {
        let first = self.assign()?;
        if !self.is(",") {
            return Ok(first);
        }
        let mut list = vec![first];
        while self.eat(",") {
            list.push(self.assign()?);
        }
        node(Kind::Seq(list))
    }

    fn assign(&mut self) -> Parsed<Expr> {
        self.enter()?;
        let e = self.assign_inner();
        self.nest -= 1;
        e
    }

    fn assign_inner(&mut self) -> Parsed<Expr> {
        let target = self.conditional()?;
        let op = match self.peek() {
            Tok::Punct("=>") => return Err("arrow functions (`=>`)".into()),
            Tok::Punct("=") => None,
            Tok::Punct("+=") => Some(BinOp::Add),
            Tok::Punct("-=") => Some(BinOp::Sub),
            Tok::Punct("*=") => Some(BinOp::Mul),
            Tok::Punct("/=") => Some(BinOp::Div),
            Tok::Punct("%=") => Some(BinOp::Rem),
            Tok::Punct("**=") => Some(BinOp::Pow),
            Tok::Punct("&=") => Some(BinOp::BitAnd),
            Tok::Punct("|=") => Some(BinOp::BitOr),
            Tok::Punct("^=") => Some(BinOp::BitXor),
            Tok::Punct("<<=") => Some(BinOp::Shl),
            Tok::Punct(">>=") => Some(BinOp::Shr),
            Tok::Punct(">>>=") => Some(BinOp::UShr),
            _ => return Ok(target),
        };
        self.next();
        assignable(&target)?;
        let value = self.assign()?;
        node(Kind::Assign(op, boxed(target), boxed(value)))
    }

    fn conditional(&mut self) -> Parsed<Expr> {
        let test = self.binary(0)?;
        if !self.eat("?") {
            return Ok(test);
        }
        let yes = self.assign()?;
        self.expect(":")?;
        let no = self.assign()?;
        node(Kind::Cond(boxed(test), boxed(yes), boxed(no)))
    }

    fn binary(&mut self, min: u8) -> Parsed<Expr> {
        let mut left = self.unary()?;
        while let Some((precedence, op)) = binary_operator(self.peek()) {
            if precedence < min {
                break;
            }
            self.next();
            // `**` groups to the right, the others to the left.
            let right = if op == Op::Bin(BinOp::Pow) {
                self.binary(precedence)?
            } else {
                self.binary(precedence + 1)?
            };
            left = node(match op {
                Op::Bin(op) => Kind::Binary(op, boxed(left), boxed(right)),
                Op::Logic(op) => Kind::Logical(op, boxed(left), boxed(right)),
            })?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Parsed<Expr> {
        self.enter()?;
        let e = self.unary_inner();
        self.nest -= 1;
        e
    }

    fn unary_inner(&mut self) -> Parsed<Expr> {
        let op = match self.peek() {
            Tok::Punct("!") => UnOp::Not,
            Tok::Punct("-") => UnOp::Neg,
            Tok::Punct("+") => UnOp::Plus,
            Tok::Punct("~") => UnOp::BitNot,
            Tok::Name(n) if &**n == "typeof" => UnOp::TypeOf,
            Tok::Name(n) if &**n == "void" => UnOp::Void,
            Tok::Name(n) if &**n == "delete" || &**n == "new" || &**n == "await" => {
                return Err(format!("`{n}`"));
            }
            Tok::Punct(p @ ("++" | "--")) => {
                let increment = *p == "++";
                self.next();
                let target = self.unary()?;
                assignable(&target)?;
                return node(Kind::Update {
                    increment,
                    prefix: true,
                    target: boxed(target),
                });
            }
            _ => return self.postfix(),
        };
        self.next();
        let operand = self.unary()?;
        node(Kind::Unary(op, boxed(operand)))
    }

    fn postfix(&mut self) -> Parsed<Expr> {
        let e = self.call_member()?;
        if self.newline_before() || !(self.is("++") || self.is("--")) {
            return Ok(e);
        }
        let increment = self.is("++");
        self.next();
        assignable(&e)?;
        node(Kind::Update {
            increment,
            prefix: false,
            target: boxed(e),
        })
    }

    fn call_member(&mut self) -> Parsed<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat(".") {
                let name = self.name()?;
                e = node(Kind::Member(boxed(e), name))?;
            } else if self.eat("[") {
                let index = self.expression()?;
                self.expect("]")?;
                e = node(Kind::Index(boxed(e), boxed(index)))?;
            } else if self.eat("(") {
                let mut args = Vec::new();
                while !self.eat(")") {
                    if self.is("...") {
                        return Err("spread arguments (`...`)".into());
                    }
                    args.push(self.assign()?);
                    if !self.eat(",") {
                        self.expect(")")?;
                        break;
                    }
                }
                e = node(Kind::Call(boxed(e), args))?;
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Parsed<Expr> {
        match self.next() {
            Tok::Num(n) => node(Kind::Num(n)),
            Tok::Str(s) => node(Kind::Str(s)),
            Tok::Name(n) => match &*n {
                "true" => node(Kind::Bool(true)),
                "false" => node(Kind::Bool(false)),
                "null" => node(Kind::Null),
                "this" => node(Kind::Ident("thisProperty".into())),
                "function" => {
                    let f = self.function_rest(false)?;
                    node(Kind::Func(f))
                }
                word if RESERVED.contains(&word) => Err(format!("`{word}` in an expression")),
                _ => node(Kind::Ident(n)),
            },
            Tok::Punct("(") => {
                let e = self.expression()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Punct("[") => {
                let mut items = Vec::new();
                while !self.eat("]") {
                    if self.is(",") {
                        return Err("arrays with holes".into());
                    }
                    if self.is("...") {
                        return Err("spread elements (`...`)".into());
                    }
                    items.push(self.assign()?);
                    if !self.eat(",") {
                        self.expect("]")?;
                        break;
                    }
                }
                node(Kind::Array(items))
            }
            Tok::Punct("{") => Err("object literals".into()),
            tok => Err(format!("{} where a value belongs", describe(&tok))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Bin(BinOp),
    Logic(Logic),
}

/// A binary operator's precedence (higher binds tighter) and kind.
fn binary_operator(tok: &Tok) -> Option<(u8, Op)> {
    use BinOp::*;
    let p = match tok {
        Tok::Punct(p) => *p,
        Tok::Name(n) if &**n == "instanceof" => return Some((8, Op::Bin(InstanceOf))),
        Tok::Name(n) if &**n == "in" => return None,
        _ => return None,
    };
    Some(match p {
        "??" => (1, Op::Logic(Logic::Nullish)),
        "||" => (2, Op::Logic(Logic::Or)),
        "&&" => (3, Op::Logic(Logic::And)),
        "|" => (4, Op::Bin(BitOr)),
        "^" => (5, Op::Bin(BitXor)),
        "&" => (6, Op::Bin(BitAnd)),
        "==" => (7, Op::Bin(Eq)),
        "!=" => (7, Op::Bin(Ne)),
        "===" => (7, Op::Bin(StrictEq)),
        "!==" => (7, Op::Bin(StrictNe)),
        "<" => (8, Op::Bin(Lt)),
        "<=" => (8, Op::Bin(Le)),
        ">" => (8, Op::Bin(Gt)),
        ">=" => (8, Op::Bin(Ge)),
        "<<" => (9, Op::Bin(Shl)),
        ">>" => (9, Op::Bin(Shr)),
        ">>>" => (9, Op::Bin(UShr)),
        "+" => (10, Op::Bin(Add)),
        "-" => (10, Op::Bin(Sub)),
        "*" => (11, Op::Bin(Mul)),
        "/" => (11, Op::Bin(Div)),
        "%" => (11, Op::Bin(Rem)),
        "**" => (12, Op::Bin(Pow)),
        _ => return None,
    })
}

fn assignable(e: &Expr) -> Parsed<()> {
    match e.kind {
        Kind::Ident(_) | Kind::Member(..) | Kind::Index(..) => Ok(()),
        _ => Err("an assignment to something other than a variable".into()),
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Num(n) => format!("the number {n}"),
        Tok::Str(_) => "a string".into(),
        Tok::Name(n) => format!("`{}`", short(n)),
        Tok::Punct(p) => format!("`{p}`"),
        Tok::End => "the end".into(),
    }
}

/// `s`, cut to a length that fits a warning.
pub(super) fn short(s: &str) -> String {
    const MAX: usize = 40;
    if s.chars().count() <= MAX {
        return s.to_owned();
    }
    let mut cut: String = s.chars().take(MAX).collect();
    cut.push('…');
    cut
}
