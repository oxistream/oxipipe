//! Player-based signature/nsig solver
//!
//! Instead of cutting single functions out of the player, this keeps all top-level
//! declarations of the player and drops statements with side effects. The resulting code
//! is executed in QuickJS. The deobfuscation is done by calling the player's own URL builder
//! function (the one that adds `alr=yes` to stream URLs) and reading back the `s` and `n`
//! parameters. This way neither the function names nor their dependencies have to be
//! extracted, which makes it a lot more resilient against obfuscation changes.

use std::{collections::HashSet, fmt::Write, ops::Range};

use ress::tokens::{Punct, Token};
use rquickjs::{Context, Runtime};

use crate::error::internal::DeobfError;

pub const SOLVE_FUNC_NAME: &str = "deobf_solve";

/// Dummy nsig parameter used when only the signature should be deobfuscated
const DUMMY_N: &str = "rustypipe_dummy_n";
const TEST_SIG: &str = "000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F202122232425262728292A2B2C2D2E2F303132333435363738393A3B3C3D3E3F404142434445464748494A4B4C4D4E4F505152535455565758595A5B5C5D5E5F606162636465666768696A";
const TEST_N: &str = "rYKKd90cGmViBQaX";
const MAX_ARG_CANDIDATES: usize = 32;

const PREAMBLE: &str = r#"if(typeof globalThis.XMLHttpRequest==="undefined"){globalThis.XMLHttpRequest={prototype:{}};}
if(typeof globalThis.location==="undefined"){globalThis.location={hash:"",host:"www.youtube.com",hostname:"www.youtube.com",href:"https://www.youtube.com/watch?v=dQw4w9WgXcQ",origin:"https://www.youtube.com",password:"",pathname:"/watch",port:"",protocol:"https:",search:"?v=dQw4w9WgXcQ",username:""};}
if(typeof globalThis.document==="undefined"){globalThis.document=Object.create(null);}
if(typeof globalThis.navigator==="undefined"){globalThis.navigator=Object.create(null);}
if(typeof globalThis.self==="undefined"){globalThis.self=globalThis;}
if(typeof globalThis.window==="undefined"){globalThis.window=globalThis;}
globalThis.window.location=globalThis.location;
var __rp_solvers=[];
function __rp_run(build,sig,n){var url=build("https://www.youtube.com/watch?v=dQw4w9WgXcQ","s",sig==null?undefined:encodeURIComponent(sig));url.set("n",n);var proto=Object.getPrototypeOf(url);var keys=Object.keys(proto).concat(Object.getOwnPropertyNames(proto));for(var i=0;i<keys.length;i++){var k=keys[i];if(["constructor","set","get","clone"].indexOf(k)===-1&&typeof url[k]==="function"){url[k]();break;}}var s=url.get("s");return{sig:s?decodeURIComponent(s):null,n:url.get("n")};}
"#;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind<'a> {
    Word(&'a str),
    Punct(Punct),
    Str(&'a str),
    Num(Option<i64>),
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Tok<'a> {
    kind: Kind<'a>,
    start: usize,
    end: usize,
}

impl Tok<'_> {
    fn is_punct(&self, p: Punct) -> bool {
        self.kind == Kind::Punct(p)
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.kind, Kind::Word(x) if x == w)
    }

    fn num(&self) -> Option<i64> {
        match self.kind {
            Kind::Num(n) => n,
            _ => None,
        }
    }
}

const RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "return",
    "super",
    "switch",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "true",
    "false",
    "null",
    "async",
    "await",
];

fn is_ident(t: &Tok) -> bool {
    matches!(t.kind, Kind::Word(w) if !RESERVED.contains(&w))
}

fn parse_num(s: &str) -> Option<i64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn tokenize(js: &str) -> Result<Vec<Tok<'_>>, DeobfError> {
    let mut toks = Vec::with_capacity(js.len() / 4);
    for item in ress::Scanner::new(js) {
        let it = item?;
        let (start, end) = (it.span.start, it.span.end);
        let kind = match it.token {
            Token::EoF => break,
            Token::Comment(_) => continue,
            Token::Ident(_) | Token::Keyword(_) | Token::Boolean(_) | Token::Null => {
                Kind::Word(&js[start..end])
            }
            Token::Punct(p) => Kind::Punct(p),
            Token::String(_) => Kind::Str(&js[start + 1..end - 1]),
            Token::Number(_) => Kind::Num(parse_num(&js[start..end])),
            _ => Kind::Other,
        };
        toks.push(Tok { kind, start, end });
    }
    Ok(toks)
}

fn depth_delta(t: &Tok) -> isize {
    match t.kind {
        Kind::Punct(Punct::OpenBrace | Punct::OpenParen | Punct::OpenBracket) => 1,
        Kind::Punct(Punct::CloseBrace | Punct::CloseParen | Punct::CloseBracket) => -1,
        _ => 0,
    }
}

/// Get the index of the bracket closing the one at `open`
fn matching_close(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in toks.iter().enumerate().skip(open) {
        depth += depth_delta(t);
        if depth == 0 {
            return Some(i);
        }
    }
    None
}

/// Find the body of the player IIFE: `(function(g){ BODY })(_yt_player)`
fn find_iife_body(toks: &[Tok]) -> Option<Range<usize>> {
    let start = toks.windows(3).position(|w| {
        w[0].is_punct(Punct::OpenParen)
            && w[1].is_word("function")
            && w[2].is_punct(Punct::OpenParen)
    })? + 2;
    let open = matching_close(toks, start)? + 1;
    if !toks.get(open)?.is_punct(Punct::OpenBrace) {
        return None;
    }
    let close = matching_close(toks, open)?;
    Some(open + 1..close)
}

/// Split a token range into top-level statements
fn split_statements(toks: &[Tok], range: Range<usize>) -> Vec<Range<usize>> {
    const BLOCK_STMTS: &[&str] = &[
        "function", "class", "if", "for", "while", "try", "switch", "do", "with",
    ];
    const CONTINUE_KW: &[&str] = &["else", "catch", "finally", "while"];

    let mut stmts = Vec::new();
    let mut depth = 0;
    let mut start = range.start;
    for i in range.clone() {
        let t = &toks[i];
        depth += depth_delta(t);
        if depth != 0 {
            continue;
        }
        let end_stmt = if t.is_punct(Punct::SemiColon) {
            true
        } else if t.is_punct(Punct::CloseBrace) {
            let first = &toks[start];
            let is_label = is_ident(first) && toks[start + 1].is_punct(Punct::Colon);
            let is_block = first.is_punct(Punct::OpenBrace)
                || is_label
                || matches!(first.kind, Kind::Word(w) if BLOCK_STMTS.contains(&w));
            let next_continues = toks
                .get(i + 1)
                .map(|n| matches!(n.kind, Kind::Word(w) if CONTINUE_KW.contains(&w)))
                .unwrap_or_default();
            is_block && !next_continues
        } else {
            false
        };
        if end_stmt {
            stmts.push(start..i + 1);
            start = i + 1;
        }
    }
    if start < range.end {
        stmts.push(start..range.end);
    }
    stmts
}

/// Length of a member expression chain (`a.b[c].d`) starting at `i`
fn member_chain_len(toks: &[Tok], i: usize, end: usize) -> Option<usize> {
    if !is_ident(toks.get(i)?) {
        return None;
    }
    let mut j = i + 1;
    while j < end {
        if toks[j].is_punct(Punct::Period)
            && toks
                .get(j + 1)
                .map_or(false, |t| matches!(t.kind, Kind::Word(_)))
        {
            j += 2;
        } else if toks[j].is_punct(Punct::OpenBracket) {
            j = matching_close(toks, j)? + 1;
        } else {
            break;
        }
    }
    Some(j - i)
}

#[derive(Debug, PartialEq, Eq)]
enum StmtKind {
    /// Function declaration, class or let/const declaration that must not be wrapped
    Bare,
    /// var declaration, assignment or helper call; wrapped in try/catch
    Wrapped,
    Drop,
}

struct FnDef<'a> {
    /// Source of the expression referring to the function
    name: &'a str,
    /// Token range of the function body (excluding braces)
    body: Range<usize>,
}

/// Find all function definitions on the top level of a statement
fn find_fn_defs<'a>(js: &'a str, toks: &[Tok<'a>], stmt: Range<usize>) -> Vec<FnDef<'a>> {
    let mut res = Vec::new();
    let first = &toks[stmt.start];

    let body_of = |fn_kw: usize| -> Option<Range<usize>> {
        let open = (fn_kw..stmt.end).find(|&i| toks[i].is_punct(Punct::OpenBrace))?;
        let close = matching_close(toks, open)?;
        Some(open + 1..close)
    };

    if first.is_word("function") {
        if let Some(name) = toks.get(stmt.start + 1).filter(|t| is_ident(t)) {
            if let Some(body) = body_of(stmt.start) {
                res.push(FnDef {
                    name: &js[name.start..name.end],
                    body,
                });
            }
        }
        return res;
    }

    // Look for `target = function` at depth 0, where target begins at the statement start
    // or after `var`/`,`
    let mut depth = 0;
    let mut target_start = Some(stmt.start + usize::from(first.is_word("var")));
    let mut i = stmt.start;
    while i < stmt.end {
        let t = &toks[i];
        if depth == 0 {
            if t.is_punct(Punct::Comma) {
                target_start = Some(i + 1);
            } else if let Some(ts) = target_start.filter(|ts| *ts == i) {
                target_start = None;
                if let Some(len) = member_chain_len(toks, ts, stmt.end) {
                    let eq = ts + len;
                    if toks.get(eq).map_or(false, |t| t.is_punct(Punct::Equal))
                        && toks.get(eq + 1).map_or(false, |t| t.is_word("function"))
                    {
                        if let Some(body) = body_of(eq + 1) {
                            res.push(FnDef {
                                name: &js[toks[ts].start..toks[eq - 1].end],
                                body: body.clone(),
                            });
                            i = body.end;
                            continue;
                        }
                    }
                }
            }
        }
        depth += depth_delta(t);
        i += 1;
    }
    res
}

fn classify(js: &str, toks: &[Tok], stmt: Range<usize>, helpers: &HashSet<&str>) -> StmtKind {
    let first = &toks[stmt.start];
    match first.kind {
        Kind::Word("var") => return StmtKind::Wrapped,
        Kind::Word("function" | "class" | "let" | "const") => return StmtKind::Bare,
        Kind::Str(_) => return StmtKind::Bare,
        _ => {}
    }
    if let Some(len) = member_chain_len(toks, stmt.start, stmt.end) {
        let next = stmt.start + len;
        if toks.get(next).map_or(false, |t| t.is_punct(Punct::Equal)) {
            return StmtKind::Wrapped;
        }
        if toks
            .get(next)
            .map_or(false, |t| t.is_punct(Punct::OpenParen))
            && helpers.contains(&js[first.start..toks[next - 1].end])
        {
            return StmtKind::Wrapped;
        }
    }
    StmtKind::Drop
}

/// Functions that set up prototypes (e.g. inheritance helpers).
/// Top-level calls to these functions have to be kept.
fn find_prototype_helpers<'a>(fns: &[FnDef<'a>], toks: &[Tok]) -> HashSet<&'a str> {
    fns.iter()
        .filter(|f| {
            toks[f.body.clone()].windows(3).any(|w| {
                w[0].is_punct(Punct::Period)
                    && w[1].is_word("prototype")
                    && w[2].is_punct(Punct::Equal)
            })
        })
        .map(|f| f.name)
        .collect()
}

/// Extract the string pool (`var ca="...".split("{")`) and the indices of `set`, `alr`, `yes`
fn find_string_pool<'a>(
    js: &'a str,
    toks: &[Tok<'a>],
    stmts: &[Range<usize>],
    rt: &Runtime,
) -> Option<(&'a str, [i64; 3])> {
    for stmt in stmts.iter().take(50) {
        let ts = &toks[stmt.clone()];
        for (i, w) in ts.windows(7).enumerate() {
            let is_split = is_ident(&w[0])
                && w[1].is_punct(Punct::Equal)
                && matches!(w[2].kind, Kind::Str(_))
                && w[3].is_punct(Punct::Period)
                && w[4].is_word("split")
                && w[5].is_punct(Punct::OpenParen)
                && matches!(w[6].kind, Kind::Str(_));
            if !is_split || (i > 0 && ts[i - 1].is_punct(Punct::Period)) {
                continue;
            }
            let Kind::Word(name) = w[0].kind else {
                continue;
            };
            let expr = &js[w[2].start..w[6].end + 1];
            let ctx = Context::full(rt).ok()?;
            let pool: Option<Vec<String>> = ctx.with(|ctx| ctx.eval(expr).ok());
            let Some(pool) = pool else {
                continue;
            };
            let idx = |s: &str| pool.iter().position(|x| x == s).map(|x| x as i64);
            if let (Some(set), Some(alr), Some(yes)) = (idx("set"), idx("alr"), idx("yes")) {
                return Some((name, [set, alr, yes]));
            }
        }
    }
    None
}

/// Parse a pool index expression (`N`, `x^N` or `N^x`) at `i`. Returns (value, is_xor, len)
fn pool_index(toks: &[Tok], i: usize) -> Option<(i64, bool, usize)> {
    let a = toks.get(i)?;
    let b = toks.get(i + 1)?;
    if let Some(n) = a.num() {
        if b.is_punct(Punct::CloseBracket) {
            return Some((n, false, 1));
        }
        if b.is_punct(Punct::Caret)
            && is_ident(toks.get(i + 2)?)
            && toks.get(i + 3)?.is_punct(Punct::CloseBracket)
        {
            return Some((n, true, 3));
        }
    } else if is_ident(a) && b.is_punct(Punct::Caret) {
        let n = toks.get(i + 2)?.num()?;
        if toks.get(i + 3)?.is_punct(Punct::CloseBracket) {
            return Some((n, true, 3));
        }
    }
    None
}

/// Parse `POOL[idx]` at `i`. Returns (value, is_xor, len)
fn pool_access(toks: &[Tok], i: usize, pool: &str) -> Option<(i64, bool, usize)> {
    if !toks.get(i)?.is_word(pool) || !toks.get(i + 1)?.is_punct(Punct::OpenBracket) {
        return None;
    }
    let (n, xor, len) = pool_index(toks, i + 2)?;
    Some((n, xor, len + 3))
}

#[derive(Debug, PartialEq, Eq)]
enum UrlFnMatch {
    Direct,
    /// Key for the obfuscated dispatcher function
    Keyed(i64),
}

/// Check if the function body contains `x.set("alr","yes")` or its string-pool equivalent
fn match_url_fn(
    toks: &[Tok],
    body: Range<usize>,
    pool: Option<(&str, [i64; 3])>,
) -> Option<UrlFnMatch> {
    let ts = &toks[body.clone()];
    if ts.windows(3).any(|w| {
        matches!(w[0].kind, Kind::Str("alr"))
            && w[1].is_punct(Punct::Comma)
            && matches!(w[2].kind, Kind::Str("yes"))
    }) {
        return Some(UrlFnMatch::Direct);
    }

    let (pool, [i_set, i_alr, i_yes]) = pool?;
    for i in body.clone() {
        // [POOL[k1]](POOL[k2],POOL[k3])
        if !toks[i].is_punct(Punct::OpenBracket) {
            continue;
        }
        let Some((k1, x1, l1)) = pool_access(toks, i + 1, pool) else {
            continue;
        };
        let mut j = i + 1 + l1;
        if !toks
            .get(j)
            .map_or(false, |t| t.is_punct(Punct::CloseBracket))
            || !toks
                .get(j + 1)
                .map_or(false, |t| t.is_punct(Punct::OpenParen))
        {
            continue;
        }
        j += 2;
        let Some((k2, x2, l2)) = pool_access(toks, j, pool) else {
            continue;
        };
        j += l2;
        if !toks.get(j).map_or(false, |t| t.is_punct(Punct::Comma)) {
            continue;
        }
        let Some((k3, x3, _)) = pool_access(toks, j + 1, pool) else {
            continue;
        };
        if (k1 ^ k2) == (i_set ^ i_alr) && (k2 ^ k3) == (i_alr ^ i_yes) {
            if !x1 && !x2 && !x3 {
                return Some(UrlFnMatch::Direct);
            }
            return Some(UrlFnMatch::Keyed(k1 ^ i_set));
        }
    }
    None
}

/// Find the numeric leading arguments the dispatcher function is called with
fn find_dispatch_args(toks: &[Tok], name: &str, key: i64) -> Vec<Vec<i64>> {
    let short_name = name.rsplit('.').next().unwrap_or(name);
    let mut matching = Vec::new();
    let mut other = Vec::new();

    for i in 0..toks.len().saturating_sub(4) {
        if !toks[i].is_word(short_name) || (i > 0 && toks[i - 1].is_word("function")) {
            continue;
        }
        // Allow Name(...), Name.call(this, ...)
        let mut j = i + 1;
        if toks[j].is_punct(Punct::Period) && toks.get(j + 1).map_or(false, |t| t.is_word("call")) {
            j += 2;
            if !toks.get(j).map_or(false, |t| t.is_punct(Punct::OpenParen)) {
                continue;
            }
            j += 1;
            while j < toks.len()
                && !toks[j].is_punct(Punct::Comma)
                && !toks[j].is_punct(Punct::CloseParen)
            {
                j += 1;
            }
        } else if !toks[j].is_punct(Punct::OpenParen) {
            continue;
        }
        let (Some(a0), Some(a1)) = (
            toks.get(j + 1).and_then(Tok::num),
            toks.get(j + 3).and_then(Tok::num),
        ) else {
            continue;
        };
        if !toks[j + 2].is_punct(Punct::Comma) {
            continue;
        }
        let args = vec![a0, a1];
        if (a0 ^ a1) == key || a0 + a1 == key || a0 - a1 == key {
            if !matching.contains(&args) {
                matching.push(args);
            }
        } else if a0 == key {
            let args = vec![a0];
            if !matching.contains(&args) {
                matching.push(args);
            }
        } else if !other.contains(&args) {
            other.push(args);
        }
    }
    matching.extend(other);
    matching.truncate(MAX_ARG_CANDIDATES);
    matching
}

/// Build a JS solver from the player code
///
/// The returned code defines the global function `deobf_solve(sig, n)`
pub fn build_solver(player_js: &str) -> Result<String, DeobfError> {
    let toks = tokenize(player_js)?;
    let body = find_iife_body(&toks).ok_or(DeobfError::Extraction("player IIFE"))?;
    let stmts = split_statements(&toks, body.clone());

    let mut fns = Vec::new();
    for stmt in &stmts {
        fns.extend(find_fn_defs(player_js, &toks, stmt.clone()));
    }
    let helpers = find_prototype_helpers(&fns, &toks);
    let rt = Runtime::new()?;
    let pool = find_string_pool(player_js, &toks, &stmts, &rt);

    // Candidate calls of URL builder functions
    let mut candidates: Vec<String> = Vec::new();
    for f in &fns {
        match match_url_fn(&toks, f.body.clone(), pool) {
            Some(UrlFnMatch::Direct) => candidates.push(f.name.to_owned()),
            Some(UrlFnMatch::Keyed(key)) => {
                for args in find_dispatch_args(&toks, f.name, key) {
                    let mut c = format!("{}(", f.name);
                    for a in args {
                        _ = write!(c, "{a},");
                    }
                    candidates.push(c);
                }
            }
            None => {}
        }
    }
    if candidates.is_empty() {
        return Err(DeobfError::Extraction("url builder fn"));
    }
    tracing::debug!("deobf solver candidates: {candidates:?}");

    let mut code = String::with_capacity(player_js.len());
    code.push_str(PREAMBLE);
    code.push_str(&player_js[..toks[body.start].start]);
    for stmt in &stmts {
        let src = &player_js[toks[stmt.start].start..toks[stmt.end - 1].end];
        match classify(player_js, &toks, stmt.clone(), &helpers) {
            StmtKind::Bare => code.push_str(src),
            StmtKind::Wrapped => {
                code.push_str("try{");
                code.push_str(src);
                code.push_str("\n}catch(e){}");
            }
            StmtKind::Drop => continue,
        }
        code.push('\n');
    }
    let glue_pos = code.len();
    let suffix = &player_js[toks[body.end - 1].end..];

    // Test all candidates and keep the working ones
    let glue = |cands: &[&String]| {
        let mut g = String::new();
        for c in cands {
            let call = if c.ends_with('(') {
                format!("{c}u,p,v)")
            } else {
                format!("({c})(u,p,v)")
            };
            _ = writeln!(
                g,
                "try{{__rp_solvers.push(function(s,n){{return __rp_run(function(u,p,v){{return {call}}},s,n)}})}}catch(e){{}}"
            );
        }
        g
    };
    let all_cands = candidates.iter().collect::<Vec<_>>();
    let test_code = format!("{}{}{}", &code[..glue_pos], glue(&all_cands), suffix);

    let ctx = Context::full(&rt)?;
    let results: Vec<Option<(String, String)>> = ctx.with(|ctx| {
        let mut opts = rquickjs::context::EvalOptions::default();
        opts.strict = false;
        ctx.eval_with_options::<(), _>(test_code.as_bytes(), opts)?;
        let solvers: rquickjs::Array = ctx.globals().get("__rp_solvers")?;
        let mut results = Vec::new();
        for s in solvers.iter::<rquickjs::Function>() {
            let s = s?;
            let res: rquickjs::Result<rquickjs::Object> = s.call((TEST_SIG, TEST_N));
            if let Err(e) = &res {
                let msg = match e {
                    rquickjs::Error::Exception => format!("{:?}", ctx.catch()),
                    e => e.to_string(),
                };
                tracing::debug!("deobf candidate failed: {msg}");
            }
            let res = res.ok().and_then(|o| {
                let sig: Option<String> = o.get("sig").ok()?;
                let n: Option<String> = o.get("n").ok()?;
                Some((sig?, n?))
            });
            results.push(res);
        }
        Ok::<_, rquickjs::Error>(results)
    })?;
    drop(ctx);

    let working = candidates
        .iter()
        .zip(results)
        .filter_map(|(c, r)| {
            let (sig, n) = r?;
            tracing::debug!("deobf candidate `{c}`: sig={sig} n={n}");
            if sig.is_empty()
                || sig == TEST_SIG
                || n.is_empty()
                || n == TEST_N
                || n.starts_with("enhanced_except_")
                || n.ends_with(TEST_N)
            {
                None
            } else {
                Some((c, sig, n))
            }
        })
        .collect::<Vec<_>>();
    let (first, sig, n) = working
        .first()
        .ok_or(DeobfError::Extraction("working url builder fn"))?;
    if working.iter().any(|(_, s, x)| s != sig || x != n) {
        tracing::warn!("deobf solver candidates returned different results, using `{first}`");
    }
    tracing::debug!("deobf solver: using `{first}`");

    let mut res = code;
    res.truncate(glue_pos);
    res.push_str(&glue(&[*first]));
    res.push_str(suffix);
    _ = write!(
        res,
        "\nvar {SOLVE_FUNC_NAME}=function(s,n){{return __rp_solvers[0](s,n==null?\"{DUMMY_N}\":n)}};"
    );
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER_JS: &str = r#"var _yt_player={};(function(g){var window=this;/*
 comment */
var ca="set{alr{yes{n{split{reverse{join".split("{");
var U=function(a){this.p={};for(var m=/[?&]([^=&]+)=([^&]*)/g,r;r=m.exec(a);)this.p[r[1]]=r[2]};
g.inherit=function(a,b){a.prototype=Object.create(b.prototype)};
U.prototype.set=function(k,v){this.p[k]=v};U.prototype.get=function(k){return this.p[k]};
U.prototype.toString=function(){var n=this.get(ca[3]);n&&this.set(ca[3],R(n));return""};
function R(a){return a[ca[4]]("")[ca[5]]()[ca[6]]("")}
sideEffect();if(document.body){throw 1}lbl:{break lbl}
var B=function(a,b,c){a=new U(a);a.set("alr","yes");c&&(c=R(decodeURIComponent(c)),a.set(b,encodeURIComponent(c)));return a};
var W=window.notDefined.foo;
})(_yt_player);
"#;

    #[test]
    fn t_split_statements() {
        let js = "var a=1;function b(){}if(a){b()}else{a=2}lbl:{break lbl}c=3,d=4;";
        let toks = tokenize(js).unwrap();
        let stmts = split_statements(&toks, 0..toks.len())
            .into_iter()
            .map(|r| &js[toks[r.start].start..toks[r.end - 1].end])
            .collect::<Vec<_>>();
        assert_eq!(
            stmts,
            [
                "var a=1;",
                "function b(){}",
                "if(a){b()}else{a=2}",
                "lbl:{break lbl}",
                "c=3,d=4;"
            ]
        );
    }

    #[test]
    fn t_build_solver() {
        let code = build_solver(PLAYER_JS).unwrap();
        assert!(!code.contains("sideEffect"));

        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let (sig, n) = ctx.with(|ctx| {
            ctx.eval::<(), _>(code.as_bytes()).unwrap();
            let sig: String = ctx
                .eval(format!("{SOLVE_FUNC_NAME}('abc%def',null).sig"))
                .unwrap();
            let n: String = ctx
                .eval(format!("{SOLVE_FUNC_NAME}(null,'xyz').n"))
                .unwrap();
            (sig, n)
        });
        assert_eq!(sig, "fed%cba");
        assert_eq!(n, "zyx");
    }

    #[test]
    fn t_build_solver_no_url_fn() {
        let err = build_solver("(function(g){var a=1;})(_yt_player);").unwrap_err();
        assert!(
            matches!(err, DeobfError::Extraction("url builder fn")),
            "{err}"
        );
    }
}
