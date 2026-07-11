//! Recursive-descent JSON parser producing a [`Value`].

use super::value::Value;

/// Parse a JSON string into a [`Value`]. This is the inverse of
/// [`Value::to_string`] for the subset of JSON that this crate emits, and is
/// strict enough for tests and config-free round-tripping (numbers are parsed
/// as `f64`; `NaN`/`Infinity` are rejected).
pub fn parse(s: &str) -> Result<Value, String> {
    let mut p = Parser::new(s);
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing data at byte {}", p.pos));
    }
    Ok(v)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn next_byte(&mut self) -> Option<u8> {
        let b = self.peek();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        match self.next_byte() {
            Some(got) if got == b => Ok(()),
            Some(got) => Err(format!(
                "expected '{}' but got '{}' at byte {}",
                b as char, got as char, self.pos
            )),
            None => Err(format!("expected '{}' at eof", b as char)),
        }
    }

    fn expect_lit(&mut self, lit: &[u8]) -> Result<(), String> {
        for &b in lit {
            self.expect(b)?;
        }
        Ok(())
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self
            .peek()
            .ok_or_else(|| "unexpected end of input".to_string())?
        {
            b'n' => {
                self.expect_lit(b"null")?;
                Ok(Value::Null)
            }
            b't' => {
                self.expect_lit(b"true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.expect_lit(b"false")?;
                Ok(Value::Bool(false))
            }
            b'"' => Ok(Value::String(self.parse_string()?)),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => Ok(Value::Number(self.parse_number()?)),
            other => Err(format!(
                "unexpected '{}' at byte {}",
                other as char, self.pos
            )),
        }
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(arr));
        }
        loop {
            arr.push(self.parse_value()?);
            self.skip_ws();
            match self.next_byte() {
                Some(b',') => self.skip_ws(),
                Some(b']') => return Ok(Value::Array(arr)),
                Some(got) => {
                    return Err(format!(
                        "expected ',' or ']' but got '{}' at byte {}",
                        got as char, self.pos
                    ))
                }
                None => return Err("unterminated array".into()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut obj = Value::object();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(obj);
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value()?;
            obj.insert(key, value);
            self.skip_ws();
            match self.next_byte() {
                Some(b',') => self.skip_ws(),
                Some(b'}') => return Ok(obj),
                Some(got) => {
                    return Err(format!(
                        "expected ',' or '}}' but got '{}' at byte {}",
                        got as char, self.pos
                    ))
                }
                None => return Err("unterminated object".into()),
            }
        }
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'+' | b'-' | b'e' | b'E')
        ) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "invalid utf-8 in number".to_string())?;
        let n: f64 = text
            .parse()
            .map_err(|_| format!("invalid number {text:?} at byte {start}"))?;
        if !n.is_finite() {
            return Err(format!("non-finite number {text:?} at byte {start}"));
        }
        Ok(n)
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let c = self
                .read_char()
                .ok_or_else(|| "unterminated string".to_string())?;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.read_char().ok_or_else(|| "bad escape".to_string())?;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hi = self.read_hex4()?;
                            let code = if (0xD800..=0xDBFF).contains(&hi) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let lo = self.read_hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&lo) {
                                    return Err(format!(
                                        "invalid surrogate pair at byte {}",
                                        self.pos
                                    ));
                                }
                                0x1_0000 + (((hi - 0xD800) << 10) | (lo - 0xDC00))
                            } else {
                                hi
                            };
                            let ch = char::from_u32(code).ok_or_else(|| {
                                format!("invalid unicode scalar {code:x} at byte {}", self.pos)
                            })?;
                            out.push(ch);
                        }
                        _ => return Err(format!("invalid escape '\\{e}' at byte {}", self.pos)),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(format!("unescaped control character at byte {}", self.pos));
                }
                c => out.push(c),
            }
        }
    }

    fn read_hex4(&mut self) -> Result<u32, String> {
        let mut acc = 0u32;
        for _ in 0..4 {
            let b = self
                .next_byte()
                .ok_or_else(|| "truncated \\u escape".to_string())?;
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => {
                    return Err(format!(
                        "invalid hex digit '{}' at byte {}",
                        b as char, self.pos
                    ))
                }
            };
            acc = (acc << 4) | d;
        }
        Ok(acc)
    }

    fn read_char(&mut self) -> Option<char> {
        let tail = std::str::from_utf8(&self.bytes[self.pos..]).ok()?;
        let c = tail.chars().next()?;
        self.pos += c.len_utf8();
        Some(c)
    }
}
