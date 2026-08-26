//! Dependency-free, strict JSON parser and serializer.
//!
//! WorldSim has **zero third-party dependencies** by design; this module is
//! the single JSON implementation used across configuration parsing, the HTTP
//! API, snapshots, and streaming frames.
//!
//! - Parsing follows RFC 8259 strictly (no comments, no trailing commas,
//!   no NaN/Infinity literals). A nesting depth limit guards against stack
//!   exhaustion on hostile input.
//! - Serialization emits compact valid JSON. Non-finite floats serialize as
//!   `null` (JSON has no representation for them); this is documented API
//!   behavior, never silently applied to simulation data (state snapshots
//!   travel as Base64-encoded IEEE-754 bytes and are bit-exact).

use crate::base64;

/// Maximum nesting depth accepted by the parser.
const MAX_DEPTH: usize = 512;

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    // ---------- constructors ----------

    pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn arr(items: Vec<Json>) -> Json {
        Json::Arr(items)
    }

    pub fn num(v: f64) -> Json {
        Json::Num(v)
    }

    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    /// Encode a byte slice as a Base64 JSON string.
    pub fn b64(bytes: &[u8]) -> Json {
        Json::Str(base64::encode(bytes))
    }

    /// Decode a Base64 JSON string into bytes.
    pub fn as_b64(&self) -> Result<Vec<u8>, String> {
        match self {
            Json::Str(s) => base64::decode(s),
            other => Err(format!("expected base64 string, got {other:?}")),
        }
    }

    // ---------- accessors ----------

    /// Object member lookup.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= u64::MAX as f64 => {
                Some(*n as u64)
            }
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) if n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Parse a `[x, y, z]` array of three finite numbers.
    pub fn as_vec3(&self) -> Option<crate::math::Vec3> {
        let items = self.as_arr()?;
        if items.len() != 3 {
            return None;
        }
        Some(crate::math::Vec3::new(
            items[0].as_f64()?,
            items[1].as_f64()?,
            items[2].as_f64()?,
        ))
    }

    // ---------- parsing ----------

    /// Parse a complete JSON document. Trailing non-whitespace is an error.
    pub fn parse(input: &str) -> Result<Json, String> {
        let mut p = Parser {
            bytes: input.as_bytes(),
            pos: 0,
        };
        p.skip_ws();
        let v = p.parse_value(0)?;
        p.skip_ws();
        if p.pos != p.bytes.len() {
            return Err(format!("trailing characters at byte {}", p.pos));
        }
        Ok(v)
    }

    // ---------- serialization ----------

    /// Compact serialization.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Num(n) => {
                if n.is_finite() {
                    // Rust's Display for f64 emits the shortest string that
                    // round-trips, which is what we want.
                    out.push_str(&format!("{n}"));
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_escaped(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, lit: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", lit, self.pos))
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("maximum nesting depth exceeded".into());
        }
        self.skip_ws();
        match self.peek() {
            None => Err("unexpected end of input".into()),
            Some(b'{') => self.parse_object(depth),
            Some(b'[') => self.parse_array(depth),
            Some(b'"') => Ok(Json::Str(self.parse_string()?)),
            Some(b't') => {
                self.expect("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.expect("false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                self.expect("null")?;
                Ok(Json::Null)
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(format!("unexpected character {:?} at byte {}", c as char, self.pos)),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Json, String> {
        self.bump(); // '{'
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(format!("expected object key at byte {}", self.pos));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.bump() != Some(b':') {
                return Err(format!("expected ':' after key at byte {}", self.pos));
            }
            let value = self.parse_value(depth + 1)?;
            pairs.push((key, value));
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Json::Obj(pairs)),
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
            }
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<Json, String> {
        self.bump(); // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Json::Arr(items));
        }
        loop {
            let value = self.parse_value(depth + 1)?;
            items.push(value);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Ok(Json::Arr(items)),
                _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.bump(); // '"'
        let mut s = String::new();
        loop {
            let b = self.bump().ok_or("unterminated string")?;
            match b {
                b'"' => return Ok(s),
                b'\\' => {
                    let esc = self.bump().ok_or("unterminated escape")?;
                    match esc {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\u{08}'),
                        b'f' => s.push('\u{0C}'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => {
                            let hi = self.parse_hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                // High surrogate: require a following \uXXXX low surrogate.
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err("unpaired surrogate".into());
                                }
                                let lo = self.parse_hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err("invalid low surrogate".into());
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err("unpaired low surrogate".into());
                            } else {
                                hi
                            };
                            s.push(
                                char::from_u32(cp)
                                    .ok_or_else(|| format!("invalid codepoint U+{cp:X}"))?,
                            );
                        }
                        c => return Err(format!("invalid escape \\{}", c as char)),
                    }
                }
                b if b < 0x20 => {
                    return Err(format!("raw control character in string at byte {}", self.pos))
                }
                b => {
                    // Re-assemble UTF-8 multi-byte sequences verbatim.
                    if b < 0x80 {
                        s.push(b as char);
                    } else {
                        let extra = if b >= 0xF0 {
                            3
                        } else if b >= 0xE0 {
                            2
                        } else {
                            1
                        };
                        let start = self.pos - 1;
                        if self.bytes.len() < start + extra + 1 {
                            return Err("truncated UTF-8 sequence".into());
                        }
                        let chunk = &self.bytes[start..start + extra + 1];
                        let decoded =
                            std::str::from_utf8(chunk).map_err(|_| "invalid UTF-8".to_string())?;
                        s.push_str(decoded);
                        self.pos = start + extra + 1;
                    }
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, String> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.bump().ok_or("truncated \\u escape")?;
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| format!("invalid hex digit {:?}", b as char))?;
            v = (v << 4) | d;
        }
        Ok(v)
    }

    fn parse_number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        // Integer part: JSON forbids leading zeros ("0" or [1-9][0-9]*).
        match self.peek() {
            Some(b'0') => {
                self.bump();
            }
            Some(c) if c.is_ascii_digit() => {
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.bump();
                }
            }
            _ => return Err(format!("invalid number at byte {start}")),
        }
        // Fraction
        if self.peek() == Some(b'.') {
            self.bump();
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(format!("digit required after '.' at byte {}", self.pos));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.bump();
            }
        }
        // Exponent
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(format!("digit required in exponent at byte {}", self.pos));
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("number out of range: {text}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars() {
        assert_eq!(Json::parse("null").unwrap(), Json::Null);
        assert_eq!(Json::parse(" true ").unwrap(), Json::Bool(true));
        assert_eq!(Json::parse("false").unwrap(), Json::Bool(false));
        assert_eq!(Json::parse("-12.5").unwrap(), Json::Num(-12.5));
        assert_eq!(Json::parse("1e3").unwrap(), Json::Num(1000.0));
        assert_eq!(Json::parse("2.5E-2").unwrap(), Json::Num(0.025));
        assert_eq!(Json::parse("\"hi\"").unwrap(), Json::Str("hi".into()));
    }

    #[test]
    fn parses_nested_structures() {
        let src = r#"{"a":[1,{"b":null},"xy"],"c":{"d":true}}"#;
        let v = Json::parse(src).unwrap();
        assert_eq!(v.get("a").unwrap().as_arr().unwrap().len(), 3);
        assert_eq!(
            v.get("c").unwrap().get("d").unwrap(),
            &Json::Bool(true)
        );
        let inner = v.get("a").unwrap().as_arr().unwrap()[1]
            .get("b")
            .unwrap();
        assert!(inner.is_null());
    }

    #[test]
    fn handles_escapes_and_unicode() {
        let v = Json::parse(r#""line\nbreak A😀 tab\t""#).unwrap();
        assert_eq!(v, Json::Str("line\nbreak A\u{1F600} tab\t".into()));
        let roundtrip = Json::Str("quote\" back\\ slash/ 控制符\u{7}".to_string()).dump();
        assert_eq!(
            Json::parse(&roundtrip).unwrap(),
            Json::Str("quote\" back\\ slash/ 控制符\u{7}".into())
        );
    }

    #[test]
    fn rejects_invalid_documents() {
        for bad in [
            "", "{\"a\":}", "[1,]", "{\"a\" 1}", "01", "-", "\"unterminated",
            "nan", "Infinity", "{},1", "[1] trailing", "\"\u{7}\"",
        ] {
            assert!(Json::parse(bad).is_err(), "should reject: {bad:?}");
        }
    }

    #[test]
    fn deep_nesting_is_rejected_safely() {
        let deep = "[".repeat(10_000) + &"]".repeat(10_000);
        assert!(Json::parse(&deep).is_err());
    }

    #[test]
    fn dump_roundtrips_and_handles_nonfinite() {
        let src = r#"{"x":1,"y":[true,null,-3.25],"z":"s"}"#;
        let v = Json::parse(src).unwrap();
        assert_eq!(Json::parse(&v.dump()).unwrap(), v);

        assert_eq!(Json::Num(f64::NAN).dump(), "null");
        assert_eq!(Json::Num(f64::INFINITY).dump(), "null");
    }

    #[test]
    fn base64_helpers() {
        let payload: Vec<u8> = (0..255u8).collect();
        let j = Json::b64(&payload);
        assert_eq!(j.as_b64().unwrap(), payload);
    }

    #[test]
    fn vec3_accessor() {
        let v = Json::parse("[1.5,-2,0]").unwrap();
        let p = v.as_vec3().unwrap();
        assert_eq!(p.x, 1.5);
        assert_eq!(p.y, -2.0);
        assert_eq!(p.z, 0.0);
        assert!(Json::parse("[1,2]").unwrap().as_vec3().is_none());
    }
}
