//! ZExpr v1 — a safe, bounded, deterministic, hashable expression language for
//! watchers / KPIs / breach `when:` / loop `stop:` / cardinality (M11).
//!
//! No shell, IO, clock, or randomness — only pure functions over numbers,
//! booleans, strings, and arrays, evaluated under a fuel + depth bound. A
//! canonical AST hash is stored so a run replays the SAME computation; a
//! hand-rolled lexer/parser keeps `zyal-core` dependency-light.
//!
//! Grammar (precedence low→high): `||`, `&&`, `== !=`, `< <= > >=`, `+ -`,
//! `* / %`, unary `- !`, primary (`number`, `"string"`, `true/false/null`,
//! `ident`, `fn(args…)`, `[array]`, `( expr )`).

use std::collections::BTreeSet;

use serde::Serialize;

const MAX_FUEL: u32 = 100_000;
const MAX_DEPTH: usize = 64;

/// A runtime value. (Not a contract type — purely the result of evaluation.)
#[derive(Debug, Clone, PartialEq)]
pub enum ZValue {
    Num(f64),
    Bool(bool),
    Str(String),
    Null,
    Arr(Vec<ZValue>),
}

impl ZValue {
    fn as_num(&self) -> Result<f64, String> {
        match self {
            ZValue::Num(n) => Ok(*n),
            ZValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            other => Err(format!("expected a number, got {other:?}")),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            ZValue::Bool(b) => *b,
            ZValue::Num(n) => *n != 0.0,
            ZValue::Null => false,
            ZValue::Str(s) => !s.is_empty(),
            ZValue::Arr(a) => !a.is_empty(),
        }
    }
}

/// Resolves named inputs (metrics / watcher refs / window buffers) to values.
pub trait ZBindings {
    fn resolve(&self, name: &str) -> Option<ZValue>;
}

/// An empty binding set (only literals/functions resolve).
pub struct NoBindings;
impl ZBindings for NoBindings {
    fn resolve(&self, _name: &str) -> Option<ZValue> {
        None
    }
}

// ---- AST -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Expr {
    Num(f64),
    Bool(bool),
    Str(String),
    Null,
    Ident(String),
    Array(Vec<Expr>),
    Unary {
        un: String,
        rhs: Box<Expr>,
    },
    Binary {
        bin: String,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

/// A compiled expression: the AST, a stable hash, and the identifiers it reads.
#[derive(Debug, Clone)]
pub struct CompiledZExpr {
    pub ast: Expr,
    pub ast_hash: String,
    pub refs: Vec<String>,
}

/// Compile source → a checked, hashable expression. Rejects forbidden functions,
/// unbalanced input, and over-deep nesting.
pub fn compile(src: &str) -> Result<CompiledZExpr, String> {
    let tokens = lex(src)?;
    let mut p = Parser { tokens, pos: 0 };
    let ast = p.parse_expr(0, 0)?;
    if p.pos != p.tokens.len() {
        return Err(format!("unexpected trailing input near token {}", p.pos));
    }
    check_calls(&ast)?;
    let mut refs = BTreeSet::new();
    collect_refs(&ast, &mut refs);
    let canonical = serde_json::to_string(&ast).map_err(|e| e.to_string())?;
    let ast_hash = format!("zexpr:{:016x}", fnv1a64(&canonical));
    Ok(CompiledZExpr {
        ast,
        ast_hash,
        refs: refs.into_iter().collect(),
    })
}

/// Evaluate a compiled expression against a binding set, fuel-bounded. Named
/// `eval_zexpr` (not `eval`) to make explicit this is the SANDBOXED ZExpr
/// interpreter — no IO, clock, randomness, or dynamic code — not a code `eval`.
pub fn eval_zexpr(expr: &CompiledZExpr, bindings: &dyn ZBindings) -> Result<ZValue, String> {
    let mut fuel = MAX_FUEL;
    eval_node(&expr.ast, bindings, &mut fuel, 0)
}

// ---- lexer -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    s.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("unterminated string literal".into());
                }
                i += 1; // closing quote
                out.push(Tok::Str(s));
            }
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let lit: String = chars[start..i].iter().collect();
                let n = lit
                    .parse::<f64>()
                    .map_err(|_| format!("bad number literal `{lit}`"))?;
                out.push(Tok::Num(n));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            // operators (two-char first)
            _ => {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                if matches!(two.as_str(), "==" | "!=" | "<=" | ">=" | "&&" | "||") {
                    out.push(Tok::Op(two));
                    i += 2;
                } else if matches!(c, '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!') {
                    out.push(Tok::Op(c.to_string()));
                    i += 1;
                } else {
                    return Err(format!("unexpected character `{c}`"));
                }
            }
        }
    }
    Ok(out)
}

// ---- parser (precedence climbing) ------------------------------------------

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

fn binding_power(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        "<" | "<=" | ">" | ">=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn parse_expr(&mut self, min_bp: u8, depth: usize) -> Result<Expr, String> {
        if depth > MAX_DEPTH {
            return Err("expression nested too deep".into());
        }
        let mut lhs = self.parse_unary(depth)?;
        while let Some(Tok::Op(op)) = self.peek() {
            let Some(bp) = binding_power(op) else { break };
            if bp < min_bp {
                break;
            }
            let op = op.clone();
            self.pos += 1;
            let rhs = self.parse_expr(bp + 1, depth + 1)?;
            lhs = Expr::Binary {
                bin: op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self, depth: usize) -> Result<Expr, String> {
        if let Some(Tok::Op(op)) = self.peek() {
            if op == "-" || op == "!" {
                let op = op.clone();
                self.pos += 1;
                let rhs = self.parse_unary(depth + 1)?;
                return Ok(Expr::Unary {
                    un: op,
                    rhs: Box::new(rhs),
                });
            }
        }
        self.parse_primary(depth)
    }

    fn parse_primary(&mut self, depth: usize) -> Result<Expr, String> {
        let tok = self.peek().cloned().ok_or("unexpected end of input")?;
        match tok {
            Tok::Num(n) => {
                self.pos += 1;
                Ok(Expr::Num(n))
            }
            Tok::Str(s) => {
                self.pos += 1;
                Ok(Expr::Str(s))
            }
            Tok::Ident(name) => {
                self.pos += 1;
                match name.as_str() {
                    "true" => Ok(Expr::Bool(true)),
                    "false" => Ok(Expr::Bool(false)),
                    "null" => Ok(Expr::Null),
                    _ => {
                        if matches!(self.peek(), Some(Tok::LParen)) {
                            self.pos += 1; // consume (
                            let args = self.parse_args(depth)?;
                            Ok(Expr::Call { name, args })
                        } else {
                            Ok(Expr::Ident(name))
                        }
                    }
                }
            }
            Tok::LParen => {
                self.pos += 1;
                let e = self.parse_expr(0, depth + 1)?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Tok::LBracket => {
                self.pos += 1;
                let mut items = Vec::new();
                if !matches!(self.peek(), Some(Tok::RBracket)) {
                    loop {
                        items.push(self.parse_expr(0, depth + 1)?);
                        match self.peek() {
                            Some(Tok::Comma) => {
                                self.pos += 1;
                            }
                            _ => break,
                        }
                    }
                }
                self.expect(Tok::RBracket)?;
                Ok(Expr::Array(items))
            }
            other => Err(format!("unexpected token {other:?}")),
        }
    }

    fn parse_args(&mut self, depth: usize) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if matches!(self.peek(), Some(Tok::RParen)) {
            self.pos += 1;
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr(0, depth + 1)?);
            match self.peek() {
                Some(Tok::Comma) => {
                    self.pos += 1;
                }
                Some(Tok::RParen) => {
                    self.pos += 1;
                    break;
                }
                _ => return Err("expected `,` or `)` in argument list".into()),
            }
        }
        Ok(args)
    }

    fn expect(&mut self, t: Tok) -> Result<(), String> {
        if self.peek() == Some(&t) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {t:?}, found {:?}", self.peek()))
        }
    }
}

// ---- static checks ---------------------------------------------------------

/// The ALLOWED function set. Anything else (shell/read_file/now/rand/…) is a hard
/// compile error — the safety boundary.
fn is_allowed_fn(name: &str) -> bool {
    matches!(
        name,
        // scalar
        "abs" | "sqrt" | "log" | "log10" | "exp" | "min" | "max" | "clamp" | "round"
            | "floor" | "ceil" | "sign" | "safe_div" | "pct_change" | "sigmoid"
            | "coalesce" | "is_null" | "if"
            // window (array arg)
            | "mean" | "sum" | "count" | "median" | "p50" | "p90" | "p95" | "p99"
            | "stdev" | "min_of" | "max_of" | "last" | "first"
            // text
            | "len" | "contains"
    )
}

fn check_calls(e: &Expr) -> Result<(), String> {
    match e {
        Expr::Call { name, args } => {
            if !is_allowed_fn(name) {
                return Err(format!(
                    "E_ZEXPR_FORBIDDEN_FN: `{name}` is not an allowed function"
                ));
            }
            for a in args {
                check_calls(a)?;
            }
            Ok(())
        }
        Expr::Array(items) => items.iter().try_for_each(check_calls),
        Expr::Unary { rhs, .. } => check_calls(rhs),
        Expr::Binary { lhs, rhs, .. } => {
            check_calls(lhs)?;
            check_calls(rhs)
        }
        _ => Ok(()),
    }
}

fn collect_refs(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::Ident(name) => {
            out.insert(name.clone());
        }
        Expr::Array(items) => items.iter().for_each(|i| collect_refs(i, out)),
        Expr::Unary { rhs, .. } => collect_refs(rhs, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect_refs(lhs, out);
            collect_refs(rhs, out);
        }
        Expr::Call { args, .. } => args.iter().for_each(|a| collect_refs(a, out)),
        _ => {}
    }
}

// ---- evaluator -------------------------------------------------------------

fn eval_node(e: &Expr, b: &dyn ZBindings, fuel: &mut u32, depth: usize) -> Result<ZValue, String> {
    if *fuel == 0 {
        return Err("E_ZEXPR_OUT_OF_FUEL".into());
    }
    *fuel -= 1;
    if depth > MAX_DEPTH {
        return Err("evaluation nested too deep".into());
    }
    match e {
        Expr::Num(n) => Ok(ZValue::Num(*n)),
        Expr::Bool(x) => Ok(ZValue::Bool(*x)),
        Expr::Str(s) => Ok(ZValue::Str(s.clone())),
        Expr::Null => Ok(ZValue::Null),
        Expr::Ident(name) => b
            .resolve(name)
            .ok_or_else(|| format!("E_ZEXPR_UNBOUND_IDENT: `{name}`")),
        Expr::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(eval_node(it, b, fuel, depth + 1)?);
            }
            Ok(ZValue::Arr(out))
        }
        Expr::Unary { un, rhs } => {
            let v = eval_node(rhs, b, fuel, depth + 1)?;
            match un.as_str() {
                "-" => Ok(ZValue::Num(-v.as_num()?)),
                "!" => Ok(ZValue::Bool(!v.truthy())),
                _ => Err(format!("unknown unary op `{un}`")),
            }
        }
        Expr::Binary { bin, lhs, rhs } => {
            // short-circuit logical ops
            if bin == "&&" {
                let l = eval_node(lhs, b, fuel, depth + 1)?;
                return Ok(ZValue::Bool(
                    l.truthy() && eval_node(rhs, b, fuel, depth + 1)?.truthy(),
                ));
            }
            if bin == "||" {
                let l = eval_node(lhs, b, fuel, depth + 1)?;
                return Ok(ZValue::Bool(
                    l.truthy() || eval_node(rhs, b, fuel, depth + 1)?.truthy(),
                ));
            }
            let l = eval_node(lhs, b, fuel, depth + 1)?;
            let r = eval_node(rhs, b, fuel, depth + 1)?;
            match bin.as_str() {
                "==" => Ok(ZValue::Bool(l == r)),
                "!=" => Ok(ZValue::Bool(l != r)),
                "<" | "<=" | ">" | ">=" => {
                    let (a, c) = (l.as_num()?, r.as_num()?);
                    Ok(ZValue::Bool(match bin.as_str() {
                        "<" => a < c,
                        "<=" => a <= c,
                        ">" => a > c,
                        _ => a >= c,
                    }))
                }
                "+" => Ok(ZValue::Num(l.as_num()? + r.as_num()?)),
                "-" => Ok(ZValue::Num(l.as_num()? - r.as_num()?)),
                "*" => Ok(ZValue::Num(l.as_num()? * r.as_num()?)),
                "/" => Ok(ZValue::Num(l.as_num()? / r.as_num()?)),
                "%" => Ok(ZValue::Num(l.as_num()? % r.as_num()?)),
                _ => Err(format!("unknown binary op `{bin}`")),
            }
        }
        Expr::Call { name, args } => eval_call(name, args, b, fuel, depth),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    b: &dyn ZBindings,
    fuel: &mut u32,
    depth: usize,
) -> Result<ZValue, String> {
    // `if` is lazy in its branches.
    if name == "if" {
        if args.len() != 3 {
            return Err("if(cond, then, else) takes 3 args".into());
        }
        let cond = eval_node(&args[0], b, fuel, depth + 1)?;
        return if cond.truthy() {
            eval_node(&args[1], b, fuel, depth + 1)
        } else {
            eval_node(&args[2], b, fuel, depth + 1)
        };
    }
    // eager-evaluate args
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(eval_node(a, b, fuel, depth + 1)?);
    }
    let num = |i: usize| {
        vals.get(i)
            .ok_or_else(|| "missing arg".to_string())
            .and_then(ZValue::as_num)
    };
    let window = |i: usize| -> Result<Vec<f64>, String> {
        match vals.get(i) {
            Some(ZValue::Arr(a)) => a.iter().map(ZValue::as_num).collect(),
            _ => Err(format!("`{name}` expects an array argument")),
        }
    };
    let out = match name {
        "abs" => num(0)?.abs(),
        "sqrt" => num(0)?.sqrt(),
        "log" => num(0)?.ln(),
        "log10" => num(0)?.log10(),
        "exp" => num(0)?.exp(),
        "round" => num(0)?.round(),
        "floor" => num(0)?.floor(),
        "ceil" => num(0)?.ceil(),
        "sign" => num(0)?.signum(),
        "sigmoid" => 1.0 / (1.0 + (-num(0)?).exp()),
        "min" => num(0)?.min(num(1)?),
        "max" => num(0)?.max(num(1)?),
        "clamp" => num(0)?.max(num(1)?).min(num(2)?),
        "pct_change" => {
            let (a, c) = (num(0)?, num(1)?);
            if a == 0.0 {
                0.0
            } else {
                (c - a) / a
            }
        }
        "safe_div" => {
            let (a, c, d) = (num(0)?, num(1)?, num(2)?);
            if c == 0.0 {
                d
            } else {
                a / c
            }
        }
        "is_null" => {
            return Ok(ZValue::Bool(matches!(
                vals.first(),
                Some(ZValue::Null) | None
            )))
        }
        "coalesce" => {
            return Ok(vals
                .into_iter()
                .find(|v| !matches!(v, ZValue::Null))
                .unwrap_or(ZValue::Null))
        }
        "len" => match vals.first() {
            Some(ZValue::Str(s)) => s.chars().count() as f64,
            Some(ZValue::Arr(a)) => a.len() as f64,
            _ => return Err("len expects a string or array".into()),
        },
        "contains" => {
            return Ok(ZValue::Bool(match (vals.first(), vals.get(1)) {
                (Some(ZValue::Str(s)), Some(ZValue::Str(sub))) => s.contains(sub.as_str()),
                _ => false,
            }))
        }
        // window functions
        "mean" => {
            let w = window(0)?;
            if w.is_empty() {
                0.0
            } else {
                w.iter().sum::<f64>() / w.len() as f64
            }
        }
        "sum" => window(0)?.iter().sum(),
        "count" => window(0)?.len() as f64,
        "min_of" => window(0)?.into_iter().fold(f64::INFINITY, f64::min),
        "max_of" => window(0)?.into_iter().fold(f64::NEG_INFINITY, f64::max),
        "first" => *window(0)?.first().unwrap_or(&0.0),
        "last" => *window(0)?.last().unwrap_or(&0.0),
        "median" | "p50" => percentile(&window(0)?, 0.50),
        "p90" => percentile(&window(0)?, 0.90),
        "p95" => percentile(&window(0)?, 0.95),
        "p99" => percentile(&window(0)?, 0.99),
        "stdev" => {
            let w = window(0)?;
            if w.len() < 2 {
                0.0
            } else {
                let m = w.iter().sum::<f64>() / w.len() as f64;
                (w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
            }
        }
        _ => return Err(format!("unknown function `{name}`")),
    };
    Ok(ZValue::Num(out))
}

fn percentile(sorted_input: &[f64], q: f64) -> f64 {
    if sorted_input.is_empty() {
        return 0.0;
    }
    let mut w = sorted_input.to_vec();
    w.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((w.len() as f64 - 1.0) * q).round() as usize;
    w[idx.min(w.len() - 1)]
}

/// FNV-1a (64-bit) — dependency-free stable hash for the canonical AST.
fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct MapBindings(BTreeMap<String, ZValue>);
    impl ZBindings for MapBindings {
        fn resolve(&self, name: &str) -> Option<ZValue> {
            self.0.get(name).cloned()
        }
    }

    fn ev(src: &str) -> ZValue {
        eval_zexpr(&compile(src).unwrap(), &NoBindings).unwrap()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("1 + 2 * 3"), ZValue::Num(7.0));
        assert_eq!(ev("(1 + 2) * 3"), ZValue::Num(9.0));
        assert_eq!(ev("-2 + 5"), ZValue::Num(3.0));
        assert_eq!(ev("10 % 3"), ZValue::Num(1.0));
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(ev("2 < 3 && 3 <= 3"), ZValue::Bool(true));
        assert_eq!(ev("1 > 2 || 2 == 2"), ZValue::Bool(true));
        assert_eq!(ev("!(1 == 1)"), ZValue::Bool(false));
    }

    #[test]
    fn scalar_and_window_functions() {
        assert_eq!(ev("safe_div(1, 0, -1)"), ZValue::Num(-1.0));
        assert_eq!(ev("sigmoid(0)"), ZValue::Num(0.5));
        assert_eq!(ev("clamp(15, 0, 10)"), ZValue::Num(10.0));
        assert_eq!(ev("mean([2, 4, 6])"), ZValue::Num(4.0));
        assert_eq!(ev("count([1, 2, 3, 4])"), ZValue::Num(4.0));
        assert_eq!(ev("max_of([3, 9, 1])"), ZValue::Num(9.0));
    }

    #[test]
    fn if_is_lazy() {
        // the false branch (which would be unbound) must not be evaluated
        assert_eq!(ev("if(true, 1, 0)"), ZValue::Num(1.0));
        let c = compile("if(false, missing_ident, 42)").unwrap();
        assert_eq!(eval_zexpr(&c, &NoBindings).unwrap(), ZValue::Num(42.0));
    }

    #[test]
    fn idents_resolve_via_bindings() {
        let mut m = BTreeMap::new();
        m.insert("score".into(), ZValue::Num(0.8));
        m.insert("prev".into(), ZValue::Num(0.6));
        let b = MapBindings(m);
        let c = compile("score - prev > 0.1").unwrap();
        assert_eq!(eval_zexpr(&c, &b).unwrap(), ZValue::Bool(true));
        assert_eq!(c.refs, vec!["prev".to_string(), "score".to_string()]);
    }

    #[test]
    fn forbidden_functions_are_rejected_at_compile() {
        for bad in [
            "now()",
            "rand()",
            "shell(\"rm -rf /\")",
            "read_file(\"/etc/passwd\")",
        ] {
            let err = compile(bad).unwrap_err();
            assert!(err.contains("E_ZEXPR_FORBIDDEN_FN"), "{bad} -> {err}");
        }
    }

    #[test]
    fn unbound_ident_errors_at_eval() {
        let c = compile("nonexistent + 1").unwrap();
        assert!(eval_zexpr(&c, &NoBindings)
            .unwrap_err()
            .contains("E_ZEXPR_UNBOUND_IDENT"));
    }

    #[test]
    fn ast_hash_is_deterministic_and_distinguishing() {
        let a = compile("score - prev").unwrap();
        let b = compile("score - prev").unwrap();
        assert_eq!(a.ast_hash, b.ast_hash);
        assert!(a.ast_hash.starts_with("zexpr:"));
        let c = compile("score + prev").unwrap();
        assert_ne!(a.ast_hash, c.ast_hash);
    }

    #[test]
    fn fuel_bounds_pathological_depth() {
        // a deeply nested parse is rejected before evaluation
        let deep = "(".repeat(200) + "1" + &")".repeat(200);
        assert!(compile(&deep).is_err());
    }
}
