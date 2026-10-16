//! JSON (RFC 8259), as the daemon's frames carry it: objects keep their
//! keys sorted, numbers are `f64`.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// An object of one member: `{"key": value}`.
    pub fn one(key: &str, value: Json) -> Json {
        Json::Object(BTreeMap::from([(key.to_string(), value)]))
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(o) => o.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
        };
        let v = p.value()?;
        p.space();
        if p.i != p.s.len() {
            return Err(format!("JSON: something after the value, at {}", p.i));
        }
        Ok(v)
    }
}

fn quote(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Number(n) if n.is_finite() => write!(f, "{n}"),
            Json::Number(_) => f.write_str("null"),
            Json::String(s) => quote(f, s),
            Json::Array(a) => {
                f.write_str("[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Json::Object(o) => {
                f.write_str("{")?;
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    quote(f, k)?;
                    write!(f, ":{v}")?;
                }
                f.write_str("}")
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("JSON: {what} expected at {}", self.i))
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            return true;
        }
        false
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut o = BTreeMap::new();
                self.space();
                if self.eat("}") {
                    return Ok(Json::Object(o));
                }
                loop {
                    self.space();
                    let k = self.string()?;
                    self.space();
                    if !self.eat(":") {
                        return self.fail("`:`");
                    }
                    o.insert(k, self.value()?);
                    self.space();
                    if self.eat("}") {
                        return Ok(Json::Object(o));
                    }
                    if !self.eat(",") {
                        return self.fail("`,` or `}`");
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.space();
                if self.eat("]") {
                    return Ok(Json::Array(a));
                }
                loop {
                    a.push(self.value()?);
                    self.space();
                    if self.eat("]") {
                        return Ok(Json::Array(a));
                    }
                    if !self.eat(",") {
                        return self.fail("`,` or `]`");
                    }
                }
            }
            Some(b'"') => self.string().map(Json::String),
            Some(b't') if self.eat("true") => Ok(Json::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Json::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i]).expect("ASCII");
                text.parse()
                    .map(Json::Number)
                    .or_else(|_| self.fail("a number"))
            }
            _ => self.fail("a value"),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .s
            .get(self.i..self.i + 4)
            .and_then(|d| std::str::from_utf8(d).ok());
        let n = digits.and_then(|d| u32::from_str_radix(d, 16).ok());
        let Some(n) = n else {
            return self.fail("four hexadecimal digits");
        };
        self.i += 4;
        Ok(n)
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat("\"") {
            return self.fail("a string");
        }
        let mut out = String::new();
        loop {
            let Some(&b) = self.s.get(self.i) else {
                return self.fail("the string's end");
            };
            self.i += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return self.fail("an escape");
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut c = self.hex4()?;
                            if (0xd800..0xdc00).contains(&c) && self.eat("\\u") {
                                let low = self.hex4()?;
                                c = 0x10000
                                    + ((c - 0xd800) << 10)
                                    + (low.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                        }
                        _ => return self.fail("an escape"),
                    }
                }
                _ => {
                    // A UTF-8 sequence, copied whole.
                    let start = self.i - 1;
                    let len = match b {
                        0..0x80 => 1,
                        0xc0..0xe0 => 2,
                        0xe0..0xf0 => 3,
                        _ => 4,
                    };
                    self.i = start + len;
                    let c = self
                        .s
                        .get(start..self.i)
                        .and_then(|c| std::str::from_utf8(c).ok());
                    match c {
                        Some(c) => out.push_str(c),
                        None => return self.fail("UTF-8"),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn values_print_and_parse_back() {
        // Non-ASCII written as escapes, the source staying ASCII.
        let b = "x\\\"y\\\\z\\n\\u0001 \u{e9} \u{1d11e}";
        let text = format!("{{\"a\":[1,2.5,-3e2,true,false,null],\"b\":\"{b}\",\"c\":{{}}}}");
        let v = Json::parse(&text).unwrap();
        assert_eq!(Json::parse(&v.to_string()).unwrap(), v);
        let expected = "x\"y\\z\n\u{1} \u{e9} \u{1d11e}";
        assert_eq!(v.get("b").and_then(Json::as_str), Some(expected));
        let pair = Json::parse(&format!("\"\\u{}\\u{}\"", "d834", "dd1e")).unwrap();
        assert_eq!(pair, Json::String("\u{1d11e}".into()));
    }

    #[test]
    fn errors_say_where() {
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "\"open", "tru", "1 2"] {
            assert!(Json::parse(bad).is_err(), "{bad}");
        }
    }
}
