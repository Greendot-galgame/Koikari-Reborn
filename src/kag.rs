use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Text(String),
    Tag {
        name: String,
        args: BTreeMap<String, String>,
    },
    Label(String),
    Script(String),
}
#[derive(Clone, Debug)]
pub struct Token {
    pub line: usize,
    pub kind: Kind,
}

pub fn attributes(source: &str) -> Result<(String, BTreeMap<String, String>), String> {
    let mut chars = source.trim().chars().peekable();
    let name: String = chars.by_ref().take_while(|c| !c.is_whitespace()).collect();
    if name.is_empty() {
        return Err("empty command".into());
    }
    let mut args = BTreeMap::new();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut key = String::new();
        while chars
            .peek()
            .is_some_and(|c| !c.is_whitespace() && *c != '=')
        {
            key.push(chars.next().unwrap());
        }
        if key.is_empty() {
            return Err("invalid attribute name".into());
        }
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() != Some(&'=') {
            args.insert(key, "true".into());
            continue;
        }
        chars.next();
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if matches!(chars.peek(), Some('"' | '\'')) {
            let quote = chars.next().unwrap();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == quote {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    let next = chars.next().ok_or("unfinished escape")?;
                    if next != quote && next != '\\' {
                        value.push('\\');
                    }
                    value.push(next);
                } else {
                    value.push(c);
                }
            }
            if !closed {
                return Err("unterminated attribute quote".into());
            }
        } else {
            while chars.peek().is_some_and(|c| !c.is_whitespace()) {
                value.push(chars.next().unwrap());
            }
        }
        args.insert(key, value);
    }
    Ok((name, args))
}

pub fn parse(source: &str) -> Result<Vec<Token>, String> {
    if source.len() > 8 * 1024 * 1024 {
        return Err("script size limit".into());
    }
    let mut tokens = Vec::new();
    let mut script: Option<(usize, String)> = None;
    for (row, line) in source.lines().enumerate() {
        let line = line.trim();
        let row = row + 1;
        if let Some((start, body)) = &mut script {
            if line.eq_ignore_ascii_case("[endscript]") || line.eq_ignore_ascii_case("@endscript") {
                tokens.push(Token {
                    line: *start,
                    kind: Kind::Script(std::mem::take(body)),
                });
                script = None;
            } else {
                body.push_str(line);
                body.push('\n');
            }
            continue;
        }
        if line.eq_ignore_ascii_case("[iscript]") || line.eq_ignore_ascii_case("@iscript") {
            script = Some((row, String::new()));
            continue;
        }
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(label) = line.strip_prefix('*') {
            tokens.push(Token {
                line: row,
                kind: Kind::Label(label.split('|').next().unwrap_or_default().into()),
            });
            continue;
        }
        if let Some(command) = line.strip_prefix('@') {
            let (name, args) = attributes(command)?;
            tokens.push(Token {
                line: row,
                kind: Kind::Tag { name, args },
            });
            continue;
        }
        let mut chars = line.chars().peekable();
        let mut text = String::new();
        while let Some(c) = chars.next() {
            if c != '[' {
                text.push(c);
                continue;
            }
            if chars.peek() == Some(&'[') {
                chars.next();
                text.push('[');
                continue;
            }
            if !text.is_empty() {
                tokens.push(Token {
                    line: row,
                    kind: Kind::Text(std::mem::take(&mut text)),
                });
            }
            let mut tag = String::new();
            let mut quote = None;
            let mut escaped = false;
            let mut closed = false;
            for c in chars.by_ref() {
                if escaped {
                    tag.push(c);
                    escaped = false;
                    continue;
                }
                if c == '\\' && quote.is_some() {
                    tag.push(c);
                    escaped = true;
                    continue;
                }
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() && matches!(c, '"' | '\'') {
                    quote = Some(c);
                }
                if c == ']' && quote.is_none() {
                    closed = true;
                    break;
                }
                tag.push(c);
            }
            if !closed {
                return Err(format!("line {row}: unterminated tag"));
            }
            let (name, args) = attributes(&tag)?;
            tokens.push(Token {
                line: row,
                kind: Kind::Tag { name, args },
            });
        }
        if !text.is_empty() {
            tokens.push(Token {
                line: row,
                kind: Kind::Text(text),
            });
        }
    }
    if let Some((line, _)) = script {
        return Err(format!("line {line}: unterminated script block"));
    }
    if tokens.len() > 500000 {
        return Err("script token limit".into());
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_quoted_brackets() {
        let t = parse(
            ";comment\n*Start|heading\n[人物 vo=voice]文字[[标签[c]\n@eval exp=\"f.a[0] = 'x'\"",
        )
        .unwrap();
        assert_eq!(t.len(), 5);
        assert!(matches!(&t[3].kind,Kind::Tag{name,..} if name=="c"));
        assert!(matches!(&t[4].kind,Kind::Tag{args,..} if args["exp"]=="f.a[0] = 'x'"));
    }
    #[test]
    fn malformed_quote_is_error() {
        assert!(parse("[a key=\"broken]").is_err());
        assert!(parse("[broken").is_err());
    }
    #[test]
    fn flags_and_spaces() {
        let (_, a) = attributes("stc ch = 6 dr1 pl f=113").unwrap();
        assert_eq!(a["ch"], "6");
        assert_eq!(a["pl"], "true");
    }
    #[test]
    fn script_blocks_are_opaque_code_and_unclosed_blocks_fail() {
        let tokens = parse("@iscript\nf.a = '[not-a-tag]';\n@endscript\nVisible[c]").unwrap();
        assert!(matches!(&tokens[0].kind,Kind::Script(s) if s.contains("[not-a-tag]")));
        assert!(matches!(&tokens[1].kind,Kind::Text(s) if s=="Visible"));
        assert!(parse("[iscript]\nf.a=1;").is_err());
    }
    #[test]
    fn quoted_attributes_preserve_non_escape_backslashes() {
        let (_, args) = attributes(r#"eval exp="f.a = 10 \ 2""#).unwrap();
        assert_eq!(args["exp"], r#"f.a = 10 \ 2"#);
    }
}
