//! Bounded, capability-free expression subset used by scenario scripts and tables.
use serde_json::Value;
use std::collections::BTreeMap;

pub type Variables = BTreeMap<String, Value>;
type Result<T> = std::result::Result<T, String>;
const LIMIT: usize = 8192;

#[derive(Clone, Debug, PartialEq)]
enum Lex {
    Word(String),
    Literal(Value),
    Op(String),
}

fn lex(source: &str) -> Result<Vec<Lex>> {
    if source.len() > 262144 {
        return Err("expression size limit".into());
    }
    let mut chars = source.chars().peekable();
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut last = ' ';
            let mut closed = false;
            for c in chars.by_ref() {
                if last == '*' && c == '/' {
                    closed = true;
                    break;
                }
                last = c;
            }
            if !closed {
                return Err("unfinished script comment".into());
            }
            continue;
        }
        if c == '\'' || c == '"' {
            let mut text = String::new();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == c {
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    text.push(match chars.next().ok_or("unfinished string escape")? {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        ch => ch,
                    });
                } else {
                    text.push(ch);
                }
            }
            if !closed {
                return Err("unfinished script string".into());
            }
            out.push(Lex::Literal(Value::String(text)));
        } else if c.is_ascii_digit() {
            let mut number = c.to_string();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_hexdigit() || *c == 'x' || *c == '.')
            {
                number.push(chars.next().unwrap());
            }
            let value = if let Some(n) = number.strip_prefix("0x") {
                Value::from(i64::from_str_radix(n, 16).map_err(|_| "invalid integer")?)
            } else {
                serde_json::from_str(&number).map_err(|_| "invalid number")?
            };
            out.push(Lex::Literal(value));
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let mut word = c.to_string();
            while chars
                .peek()
                .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            {
                word.push(chars.next().unwrap());
            }
            out.push(Lex::Word(word));
        } else {
            let mut op = c.to_string();
            if chars.peek().is_some_and(|n| {
                matches!(
                    (c, *n),
                    ('=', '=')
                        | ('!', '=')
                        | ('<', '=')
                        | ('>', '=')
                        | ('<', '<')
                        | ('>', '>')
                        | ('&', '&')
                        | ('|', '|')
                        | ('+', '+')
                        | ('-', '-')
                        | ('+', '=')
                        | ('-', '=')
                        | ('*', '=')
                        | ('/', '=')
                )
            }) {
                op.push(chars.next().unwrap());
                if (op == "==" || op == "!=") && chars.peek() == Some(&'=') {
                    op.push(chars.next().unwrap());
                }
            }
            if !"()[]{}.,;?:+-*/%\\!~&|^<>=".contains(c) {
                return Err("unsupported script character".into());
            }
            out.push(Lex::Op(op));
        }
        if out.len() > LIMIT {
            return Err("expression token limit".into());
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Expr {
    Literal(Value),
    Variable(String),
    Array(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
}

struct Parser {
    tokens: Vec<Lex>,
    pos: usize,
    depth: usize,
}
impl Parser {
    fn new(source: &str) -> Result<Self> {
        Ok(Self {
            tokens: lex(source)?,
            pos: 0,
            depth: 0,
        })
    }
    fn at(&self, op: &str) -> bool {
        self.tokens.get(self.pos) == Some(&Lex::Op(op.into()))
    }
    fn take(&mut self, op: &str) -> bool {
        if self.at(op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, op: &str) -> Result<()> {
        if self.take(op) {
            Ok(())
        } else {
            Err(format!("expected {op} at expression token {}", self.pos))
        }
    }
    fn expression(&mut self, min: u8) -> Result<Expr> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("expression nesting limit".into());
        }
        let next = self
            .tokens
            .get(self.pos)
            .cloned()
            .ok_or("missing expression")?;
        self.pos += 1;
        let mut left = match next {
            Lex::Literal(v) => Expr::Literal(v),
            Lex::Word(w) if matches!(w.as_str(), "true" | "false" | "void" | "null") => {
                Expr::Literal(match w.as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    _ => Value::Null,
                })
            }
            Lex::Word(w) if w == "typeof" || w == "int" => {
                Expr::Unary(w, Box::new(self.expression(14)?))
            }
            Lex::Word(w) => Expr::Variable(w),
            Lex::Op(op) if matches!(op.as_str(), "!" | "~" | "+" | "-" | "&") => {
                Expr::Unary(op, Box::new(self.expression(14)?))
            }
            Lex::Op(op) if op == "(" => {
                let e = self.expression(0)?;
                self.expect(")")?;
                e
            }
            Lex::Op(op) if op == "[" => {
                let mut elements = Vec::new();
                while !self.take("]") {
                    elements.push(self.expression(2)?);
                    if !self.take(",") {
                        self.expect("]")?;
                        break;
                    }
                }
                Expr::Array(elements)
            }
            _ => {
                return Err(format!(
                    "unsupported expression syntax at token {}",
                    self.pos
                ))
            }
        };
        loop {
            if min <= 15 && self.take(".") {
                let Some(Lex::Word(w)) = self.tokens.get(self.pos).cloned() else {
                    return Err("expected member name".into());
                };
                self.pos += 1;
                left = Expr::Member(Box::new(left), w);
                continue;
            }
            if min <= 15 && self.take("[") {
                let i = self.expression(0)?;
                self.expect("]")?;
                left = Expr::Index(Box::new(left), Box::new(i));
                continue;
            }
            if min <= 15 && self.take("(") {
                let mut args = Vec::new();
                while !self.take(")") {
                    args.push(self.expression(2)?);
                    if !self.take(",") {
                        self.expect(")")?;
                        break;
                    }
                }
                left = Expr::Call(Box::new(left), args);
                continue;
            }
            if min <= 15 && (self.at("++") || self.at("--")) {
                let op = if self.take("++") {
                    "post++"
                } else {
                    self.expect("--")?;
                    "post--"
                };
                left = Expr::Unary(op.into(), Box::new(left));
                continue;
            }
            if min <= 3 && self.take("?") {
                let yes = self.expression(2)?;
                self.expect(":")?;
                let no = self.expression(3)?;
                left = Expr::Conditional(Box::new(left), Box::new(yes), Box::new(no));
                continue;
            }
            let Some(Lex::Op(op)) = self.tokens.get(self.pos).cloned() else {
                break;
            };
            let precedence = match op.as_str() {
                "," => 1,
                "=" | "+=" | "-=" | "*=" | "/=" => 2,
                "||" => 4,
                "&&" => 5,
                "|" => 6,
                "^" => 7,
                "&" => 8,
                "==" | "!=" | "===" | "!==" => 9,
                "<" | ">" | "<=" | ">=" => 10,
                "<<" | ">>" => 11,
                "+" | "-" => 12,
                "*" | "/" | "%" | "\\" => 13,
                _ => break,
            };
            if precedence < min {
                break;
            }
            self.pos += 1;
            let right = self.expression(if precedence == 2 {
                precedence
            } else {
                precedence + 1
            })?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }
}

pub fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
pub fn string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}
fn integer(v: &Value) -> Result<i64> {
    match v {
        Value::Null => Ok(0),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => n.as_i64().ok_or("expected bounded integer".into()),
        Value::String(s) => s.parse().map_err(|_| "expected integer".into()),
        _ => Err("expected integer".into()),
    }
}
fn path(expr: &Expr, vars: &mut Variables, budget: &mut usize) -> Result<String> {
    match expr {
        Expr::Variable(w) => Ok(w.clone()),
        Expr::Member(e, w) => Ok(format!("{}.{}", path(e, vars, budget)?, w)),
        Expr::Index(e, i) => Ok(format!(
            "{}[{}]",
            path(e, vars, budget)?,
            string(&evaluate(i, vars, budget)?)
        )),
        _ => Err("invalid assignment or function target".into()),
    }
}
fn valid_path(key: &str) -> bool {
    ["f.", "tf.", "sf.", "lf.", "mp."]
        .iter()
        .any(|p| key.starts_with(p))
        || key.starts_with("local.")
}
fn check_expr(expr: &Expr, read_only: bool) -> Result<()> {
    let mut stack = vec![(expr, 0)];
    while let Some((e, depth)) = stack.pop() {
        if depth > 64 {
            return Err("expression tree nesting limit".into());
        }
        match e {
            Expr::Unary(op, e) => {
                if read_only && matches!(op.as_str(), "post++" | "post--") {
                    return Err("side effect in read-only expression".into());
                }
                stack.push((e, depth + 1));
            }
            Expr::Binary(op, l, r) => {
                if read_only && matches!(op.as_str(), "=" | "+=" | "-=" | "*=" | "/=") {
                    return Err("assignment in read-only expression".into());
                }
                stack.push((l, depth + 1));
                stack.push((r, depth + 1));
            }
            Expr::Index(e, i) => {
                stack.push((e, depth + 1));
                stack.push((i, depth + 1));
            }
            Expr::Member(e, _) => stack.push((e, depth + 1)),
            Expr::Conditional(c, y, n) => {
                stack.push((c, depth + 1));
                stack.push((y, depth + 1));
                stack.push((n, depth + 1));
            }
            Expr::Call(f, args) => {
                stack.push((f, depth + 1));
                for e in args {
                    stack.push((e, depth + 1));
                }
            }
            Expr::Array(es) => {
                for e in es {
                    stack.push((e, depth + 1));
                }
            }
            _ => (),
        }
    }
    Ok(())
}
fn assign(expr: &Expr, value: Value, vars: &mut Variables, budget: &mut usize) -> Result<()> {
    if let Expr::Index(parent, index) = expr {
        let index = integer(&evaluate(index, vars, budget)?)?;
        if !(0..4096).contains(&index) {
            return Err("array assignment index limit".into());
        }
        let mut array = match evaluate(parent, vars, budget)? {
            Value::Null => Vec::new(),
            Value::Array(a) => a,
            _ => return Err("array assignment on non-array".into()),
        };
        array.resize(array.len().max(index as usize + 1), Value::Null);
        array[index as usize] = value;
        return assign(parent, Value::Array(array), vars, budget);
    }
    let key = path(expr, vars, budget)?;
    if !valid_path(&key) || key.len() > 256 {
        return Err("invalid writable variable".into());
    }
    if vars.len() >= 4096 && !vars.contains_key(&key) {
        return Err("variable count limit".into());
    }
    vars.insert(key, value);
    Ok(())
}
fn evaluate(expr: &Expr, vars: &mut Variables, budget: &mut usize) -> Result<Value> {
    *budget = budget.checked_sub(1).ok_or("expression execution limit")?;
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Array(es) => Ok(Value::Array(
            es.iter()
                .map(|e| evaluate(e, vars, budget))
                .collect::<Result<_>>()?,
        )),
        Expr::Variable(_) | Expr::Member(_, _) | Expr::Index(_, _) => {
            let key = path(expr, vars, budget)?;
            if key == "kag.clickCount" {
                return Ok(vars.get(&key).cloned().unwrap_or(Value::from(0)));
            }
            if !valid_path(&key) {
                return Err(format!(
                    "unsupported variable namespace: {}",
                    key.split('.').next().unwrap_or("")
                ));
            }
            if let Some(v) = vars.get(&key) {
                return Ok(v.clone());
            }
            if let Expr::Index(e, i) = expr {
                let array = evaluate(e, vars, budget)?;
                let index = integer(&evaluate(i, vars, budget)?)?;
                if index < 0 {
                    return Err("negative array index".into());
                }
                return Ok(array.get(index as usize).cloned().unwrap_or(Value::Null));
            }
            if let Expr::Member(e, w) = expr {
                if (w == "count" || w == "length")
                    && !matches!(e.as_ref(),Expr::Variable(namespace) if matches!(namespace.as_str(),"f"|"tf"|"sf"|"lf"|"mp"|"local"))
                {
                    let value = evaluate(e, vars, budget)?;
                    return Ok(Value::from(match value {
                        Value::Array(v) => v.len(),
                        Value::String(v) => v.chars().count(),
                        _ => 0,
                    }));
                }
            }
            Ok(Value::Null)
        }
        Expr::Unary(op, e) => {
            if op == "post++" || op == "post--" {
                let old = evaluate(e, vars, budget)?;
                let n = integer(&old)?
                    .checked_add(if op == "post++" { 1 } else { -1 })
                    .ok_or("integer overflow")?;
                assign(e, Value::from(n), vars, budget)?;
                return Ok(old);
            }
            let v = evaluate(e, vars, budget)?;
            match op.as_str() {
                "!" => Ok(Value::Bool(!truth(&v))),
                "&" => Ok(v),
                "+" | "int" => Ok(Value::from(integer(&v)?)),
                "-" => Ok(Value::from(
                    integer(&v)?.checked_neg().ok_or("integer overflow")?,
                )),
                "~" => Ok(Value::from(!integer(&v)?)),
                "typeof" => Ok(Value::String(
                    match v {
                        Value::Null => "undefined",
                        Value::String(_) => "String",
                        Value::Number(_) | Value::Bool(_) => "Integer",
                        _ => "Object",
                    }
                    .into(),
                )),
                _ => Err("unsupported unary operator".into()),
            }
        }
        Expr::Conditional(c, y, n) => {
            if truth(&evaluate(c, vars, budget)?) {
                evaluate(y, vars, budget)
            } else {
                evaluate(n, vars, budget)
            }
        }
        Expr::Binary(op, l, r) => {
            if matches!(op.as_str(), "=" | "+=" | "-=" | "*=" | "/=") {
                let value = if op == "=" {
                    evaluate(r, vars, budget)?
                } else {
                    evaluate(
                        &Expr::Binary(op[..1].into(), l.clone(), r.clone()),
                        vars,
                        budget,
                    )?
                };
                assign(l, value.clone(), vars, budget)?;
                return Ok(value);
            }
            let a = evaluate(l, vars, budget)?;
            if op == "&&" && !truth(&a) {
                return Ok(Value::Bool(false));
            }
            if op == "||" && truth(&a) {
                return Ok(Value::Bool(true));
            }
            let b = evaluate(r, vars, budget)?;
            match op.as_str() {
                "," => Ok(b),
                "&&" | "||" => Ok(Value::Bool(truth(&b))),
                "===" | "!==" => Ok(Value::Bool((a == b) == (op == "==="))),
                "==" | "!=" => {
                    let eq =
                        a == b || (integer(&a).is_ok() && integer(&a).ok() == integer(&b).ok());
                    Ok(Value::Bool(eq == (op == "==")))
                }
                "+" if a.is_string() || b.is_string() => {
                    let s = string(&a) + &string(&b);
                    if s.len() > 65536 {
                        Err("string limit".into())
                    } else {
                        Ok(Value::String(s))
                    }
                }
                _ => {
                    let x = integer(&a)?;
                    let y = integer(&b)?;
                    let n = match op.as_str() {
                        "<" => return Ok(Value::Bool(x < y)),
                        ">" => return Ok(Value::Bool(x > y)),
                        "<=" => return Ok(Value::Bool(x <= y)),
                        ">=" => return Ok(Value::Bool(x >= y)),
                        "+" => x.checked_add(y),
                        "-" => x.checked_sub(y),
                        "*" => x.checked_mul(y),
                        "/" | "\\" => x.checked_div(y),
                        "%" => x.checked_rem(y),
                        "<<" if (0..63).contains(&y) => x.checked_mul(1i64 << y),
                        ">>" if (0..64).contains(&y) => Some(x >> y),
                        "&" => Some(x & y),
                        "|" => Some(x | y),
                        "^" => Some(x ^ y),
                        _ => None,
                    }
                    .ok_or("arithmetic overflow, invalid shift or division by zero")?;
                    Ok(Value::from(n))
                }
            }
        }
        Expr::Call(f, args) => {
            let name = path(f, vars, budget)?;
            // These original calls affect only the replaced auto/skip UI or removed Steam integration.
            if !args.is_empty()
                || !matches!(
                    name.as_str(),
                    "kag.cancelAutoMode" | "kag.cancelSkip" | "SteamCheckAllRouteClear"
                )
            {
                return Err(format!("unsupported side effect/function: {name}"));
            }
            Ok(Value::Null)
        }
    }
}

pub fn eval(source: &str, vars: &Variables) -> Result<Value> {
    let mut p = Parser::new(source)?;
    let e = p.expression(0)?;
    if p.pos != p.tokens.len() {
        return Err(format!("trailing expression tokens at {}", p.pos));
    }
    check_expr(&e, true)?;
    let mut budget = LIMIT;
    evaluate(&e, &mut vars.clone(), &mut budget)
}

pub fn execute(source: &str, vars: &mut Variables) -> Result<()> {
    let mut p = Parser::new(source)?;
    let mut next = vars.clone();
    let mut budget = LIMIT;
    statements(&mut p, &mut next, &mut budget, true, false, 0)?;
    *vars = next;
    Ok(())
}
fn statements(
    p: &mut Parser,
    vars: &mut Variables,
    budget: &mut usize,
    active: bool,
    block: bool,
    depth: usize,
) -> Result<()> {
    if depth > 64 {
        return Err("script block nesting limit".into());
    }
    while p.pos < p.tokens.len() && !p.at("}") {
        if p.take(";") {
            continue;
        }
        let is_if = matches!(p.tokens.get(p.pos),Some(Lex::Word(w)) if w=="if");
        if is_if {
            p.pos += 1;
            p.expect("(")?;
            let e = p.expression(0)?;
            p.expect(")")?;
            check_expr(&e, true)?;
            let condition = active && truth(&evaluate(&e, vars, budget)?);
            p.expect("{")?;
            statements(p, vars, budget, condition, true, depth + 1)?;
            if matches!(p.tokens.get(p.pos),Some(Lex::Word(w)) if w=="else") {
                p.pos += 1;
                p.expect("{")?;
                statements(p, vars, budget, active && !condition, true, depth + 1)?;
            }
            continue;
        }
        let e = p.expression(0)?;
        check_expr(&e, false)?;
        if active {
            evaluate(&e, vars, budget)?;
        }
        if !p.take(";") && p.pos < p.tokens.len() && !p.at("}") {
            return Err("script statement separator missing".into());
        }
    }
    if block {
        p.expect("}")?;
    } else if p.pos != p.tokens.len() {
        return Err("unexpected script closing brace".into());
    }
    Ok(())
}

/// Extract one named literal table without executing surrounding initialization code.
pub fn table(source: &str, name: &str) -> Result<Value> {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(after) = trimmed.strip_prefix(name) {
            if let Some(after) = after.trim_start().strip_prefix('=') {
                let start = offset + line.len() - after.len();
                let tail = &source[start..];
                let begin = tail.find('[').ok_or("table array missing")?;
                if !tail[..begin].trim().is_empty() {
                    return Err("table must be a literal array".into());
                }
                let mut depth = 0usize;
                let mut quote = None;
                let mut escape = false;
                let mut end = None;
                for (i, c) in tail.char_indices().skip_while(|(i, _)| *i < begin) {
                    if escape {
                        escape = false;
                        continue;
                    }
                    if quote.is_some() && c == '\\' {
                        escape = true;
                        continue;
                    }
                    if quote == Some(c) {
                        quote = None;
                        continue;
                    }
                    if quote.is_none() && matches!(c, '\'' | '"') {
                        quote = Some(c);
                        continue;
                    }
                    if quote.is_some() {
                        continue;
                    }
                    if c == '[' {
                        depth += 1;
                    }
                    if depth > 64 {
                        return Err("table nesting limit".into());
                    }
                    if c == ']' {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(i + 1);
                            break;
                        }
                    }
                }
                let mut p = Parser::new(&tail[..end.ok_or("unterminated table array")?])?;
                let e = p.expression(2)?;
                check_expr(&e, true)?;
                let mut budget = LIMIT;
                return evaluate(&e, &mut Variables::new(), &mut budget);
            }
        }
        offset += line.len();
    }
    Err(format!("table missing: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transactional_eval_short_circuit_and_side_effects() {
        let mut v = Variables::new();
        execute("f.a = 2; if (f.a == 2) { f.a += 3; } else { System.exit(); } f.b = f.a > 3 ? 'yes' : 'no';",&mut v).unwrap();
        assert_eq!(v["f.a"], 5);
        assert_eq!(v["f.b"], "yes");
        assert_eq!(eval("false && System.exit()", &v).unwrap(), false);
        let before = v.clone();
        assert!(execute("f.a = 10, System.exit()", &mut v).is_err());
        assert_eq!(before, v);
        assert!(execute("f.a = 1 / 0", &mut v).is_err());
    }
    #[test]
    fn tables_quoted_delimiters_and_indexing() {
        let v = table(
            ";init\nlf.Test = [['a;]', -1], ['b', 1 << 2]];\n[endscript]",
            "lf.Test",
        )
        .unwrap();
        assert_eq!(v, serde_json::json!([["a;]", -1], ["b", 4]]));
        let vars = BTreeMap::from([("lf.rows".into(), v)]);
        assert_eq!(eval("lf.rows[1][1]", &vars).unwrap(), 4);
    }
}
