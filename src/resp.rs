use std::{error::Error, fmt::Display, iter::Peekable};

#[derive(Debug, PartialEq)]
pub enum RespValue {
    Array(Vec<RespValue>),
    BulkString(String),
    SimpleString(String),
    Integer(i64),
    Boolean(bool),
    SimpleError(String),
    // Error(RespError),
    Nil,
    Eof,
}

impl RespValue {
    pub fn serialize_value(&self) -> Result<String, RespError> {
        let serialized = match self {
            RespValue::SimpleString(s) => RespValue::serialize_simple_string(s),
            RespValue::Integer(i) => RespValue::serialize_int(i),
            RespValue::BulkString(s) => RespValue::serialize_bulk_string(s),
            RespValue::Boolean(b) => RespValue::serialize_boolean(b),
            RespValue::Array(ref a) => RespValue::serialize_array(a)?,
            RespValue::SimpleError(e) => RespValue::serialize_simple_error(e),
            RespValue::Nil => {
                todo!();
            }
            RespValue::Eof => {
                todo!();
            }
        };

        Ok(serialized)
    }

    pub fn serialize_int(i: &i64) -> String {
        format!(":{}\r\n", i)
    }

    pub fn serialize_simple_string(s: &str) -> String {
        format!("+{}\r\n", s)
    }

    pub fn serialize_bulk_string(s: &str) -> String {
        format!("${}\r\n{}\r\n", s.len(), s)
    }

    pub fn serialize_boolean(b: &bool) -> String {
        let v = match b {
            true => 't',
            false => 'f',
        };

        format!("#{}\r\n", v)
    }

    pub fn serialize_simple_error(e: &str) -> String {
        format!("-{}\r\n", e)
    }

    pub fn serialize_array(a: &Vec<RespValue>) -> Result<String, RespError> {
        let parts = a
            .iter()
            .map(|v| RespValue::serialize_value(v))
            .collect::<Result<Vec<String>, _>>()?
            .join("");

        Ok(format!("*{}\r\n{}", a.len(), parts))
    }

    pub fn serialize(&mut self) -> Result<Vec<u8>, RespError> {
        let serialized = self.serialize_value()?;
        Ok(serialized.as_bytes().to_vec())
    }
}

pub type RespParseResult = Result<RespValue, RespError>;

#[derive(Debug, PartialEq)]
pub struct RespError {
    msg: String,
    idx: usize,
    char: char,
}

impl Error for RespError {}

impl Display for RespError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}, char {}", self.msg, self.idx, self.char)
    }
}

/// The main parser for RESP
///
/// It parses one item at a time, ie. from the next item type (`:, +, #, ...`) identifier to the next `\r\n``
///
/// Built on a iterator
///
/// Parsing happens step-wise, and methods reflect this as they are broken down into common operators
pub struct RespParser<I>
where
    I: Iterator<Item = char>,
{
    chars: Peekable<I>,
    idx: usize,
}

impl<I: Iterator<Item = char>> RespParser<I> {
    pub fn new(it: I) -> Self {
        Self {
            chars: it.peekable(),
            idx: 0,
        }
    }

    /// Construct a error message and return a [`RespParseResult`]
    pub fn err(&mut self, msg: String) -> RespParseResult {
        Err(RespError {
            msg,
            idx: self.idx,
            char: 'a',
        })
    }

    /// Unexpected EOF
    pub fn unexpected_eof(&mut self) -> RespParseResult {
        self.err(format!("unexpected eof"))
    }

    /// Consume and return the next item in `char` iterator
    pub fn next(&mut self) -> Option<char> {
        self.idx += 1;
        match self.chars.next() {
            Some(c) => return Some(c),
            None => None,
        }
    }

    /// Pek at next `char` in iterator
    pub fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    /// Check that next two chars are `\r\n', if yes consume them
    fn correct_sep(&mut self) -> RespParseResult {
        match self.next() {
            Some('\r') => {}
            Some(c) => return self.err(format!("\\r separator expected, found {}", c)),
            None => return self.unexpected_eof(),
        };

        match self.next() {
            Some('\n') => {}
            Some(c) => return self.err(format!("\\n separator expected, found {}", c)),
            None => return self.unexpected_eof(),
        };

        Ok(RespValue::Nil)
    }

    /// Parse an arbitraty constant
    pub fn parse_constant(&mut self, s: &str) -> Option<String> {
        for c in s.chars() {
            match self.next() {
                Some(x) if x != c => {
                    let msg = format!("unexpected value {} while parsing {} of {:?}", x, c, s);
                    return Some(msg);
                }
                Some(_) => {}
                None => return Some("unexpected eof".into()),
            }
        }
        None
    }

    /// Parse an integer
    pub fn parse_int(&mut self) -> RespParseResult {
        let mut s = String::new();

        match self.peek() {
            Some('-' | '+') => {
                s.push(self.next().unwrap());
            }
            Some('0'..='9') => {}
            Some(c) => return self.err(format!("invalid character while parsing integer '{}'", c)),
            None => {}
        }

        while Some('\r') != self.peek() {
            match self.peek() {
                Some('0'..='9') => s.push(self.next().unwrap()),
                Some(c) => {
                    return self.err(format!("invalid char '{}' found while parsing integer", c));
                }
                None => return self.err(String::from("Unterminated integer")),
            }
        }

        self.correct_sep()?;

        return Ok(RespValue::Integer(s.parse::<i64>().unwrap()));
    }

    /// Parse a boolean value
    pub fn parse_bool(&mut self) -> RespParseResult {
        match self.next() {
            Some('f') => {
                self.correct_sep()?;
                return Ok(RespValue::Boolean(false));
            }
            Some('t') => {
                self.correct_sep()?;
                return Ok(RespValue::Boolean(true));
            }
            Some(c) => return self.err(format!("invalid value for boolean: '{}'", c)),
            None => return self.unexpected_eof(),
        }
    }

    /// Parse a simple string
    pub fn parse_simple_string(&mut self) -> RespParseResult {
        let mut s = String::new();

        while let Some(c) = self.peek() {
            if c == '\r' {
                self.correct_sep()?;
                break;
            } else {
                s.push(self.next().unwrap());
            }
        }

        return Ok(RespValue::SimpleString(s));
    }

    /// Parse a bulk string
    pub fn parse_bulk_string(&mut self) -> RespParseResult {
        let mut s = String::new();

        while let Some('0'..='9') = self.peek() {
            s.push(self.next().unwrap());
        }

        let size = match s.parse::<usize>() {
            Ok(v) => v,
            Err(_) => return self.err(format!("failed to parse bulk string size {}", s)),
        };

        self.correct_sep()?;

        let mut blk_string = String::with_capacity(size);

        for _ in 0..size {
            match self.next() {
                Some(c) => blk_string.push(c),
                None => return self.unexpected_eof(),
            }
        }

        self.correct_sep()?;

        return Ok(RespValue::BulkString(blk_string));
    }

    pub fn parse_array(&mut self) -> RespParseResult {
        let size = match self.parse_int()? {
            RespValue::Integer(c) => c,
            _ => return self.err("invalid size".into()),
        };

        let mut arr: Vec<RespValue> = Vec::with_capacity(size as usize);

        for _ in 0..size {
            let v = self.parse_next()?;
            arr.push(v);
        }

        Ok(RespValue::Array(arr))
    }

    pub fn parse_simple_error(&mut self) -> RespParseResult {
        let simple_error = match self.parse_simple_string() {
            Ok(RespValue::SimpleString(s)) => s,
            Err(_) | Ok(_) => return self.err("Failed to parse simple error".into()),
        };

        Ok(RespValue::SimpleError(simple_error))
    }

    pub fn parse_next(&mut self) -> RespParseResult {
        match self.next() {
            Some('+') => self.parse_simple_string(),
            Some(':') => self.parse_int(),
            Some('#') => self.parse_bool(),
            Some('$') => self.parse_bulk_string(),
            Some('*') => self.parse_array(),
            Some('-') => self.parse_simple_error(),
            Some(c) => return self.err(format!("invalid type identifier found: '{}'", c)),
            // Expected EOF
            None => Ok(RespValue::Eof),
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn parse_simple_string() {
        let mut parser = RespParser::new("Testing\r\n".chars());
        let out = parser.parse_simple_string().unwrap();

        assert_eq!(out, RespValue::SimpleString(String::from("Testing")));

        let mut parser = RespParser::new("Test ing\r\n".chars());
        let out = parser.parse_simple_string().unwrap();

        assert_eq!(out, RespValue::SimpleString(String::from("Test ing")));
    }

    #[test]
    fn parse_int() {
        let mut parser = RespParser::new("89\r\n".chars());
        let out = parser.parse_int().unwrap();

        assert_eq!(out, RespValue::Integer(89));

        let mut parser = RespParser::new("+32\r\n".chars());
        let out = parser.parse_int().unwrap();

        assert_eq!(out, RespValue::Integer(32));

        let mut parser = RespParser::new("-1223\r\n".chars());
        let out = parser.parse_int().unwrap();

        assert_eq!(out, RespValue::Integer(-1223));
    }

    #[test]
    fn parse_bool() {
        let mut parser = RespParser::new("t\r\n".chars());
        let out = parser.parse_bool().unwrap();

        assert_eq!(out, RespValue::Boolean(true));

        let mut parser = RespParser::new("f\r\n".chars());
        let out = parser.parse_bool().unwrap();

        assert_eq!(out, RespValue::Boolean(false));
    }

    #[test]
    fn parse_bulk_string() {
        let mut parser = RespParser::new("2\r\nOK\r\n".chars());
        let out = parser.parse_bulk_string().unwrap();
        assert_eq!(out, RespValue::BulkString("OK".into()));

        let mut parser = RespParser::new("24\r\nthis is a \rlonge\nr value\r\n".chars());
        let out = parser.parse_bulk_string().unwrap();
        assert_eq!(
            out,
            RespValue::BulkString("this is a \rlonge\nr value".into())
        );
    }
    #[test]
    fn parse_basic_array() {
        let mut parser = RespParser::new("2\r\n:32\r\n+test\r\n".chars());
        let out = parser.parse_array().unwrap();

        assert_eq!(
            out,
            RespValue::Array(vec![
                RespValue::Integer(32),
                RespValue::SimpleString("test".into())
            ])
        );

        let mut parser = RespParser::new("4\r\n:32\r\n+test\r\n$2\r\nOK\r\n#t\r\n".chars());
        let out = parser.parse_array().unwrap();

        assert_eq!(
            out,
            RespValue::Array(vec![
                RespValue::Integer(32),
                RespValue::SimpleString("test".into()),
                RespValue::BulkString("OK".into()),
                RespValue::Boolean(true)
            ])
        );
    }

    #[test]
    fn parse_nested_array() {
        let mut parser =
            RespParser::new("2\r\n*3\r\n:1\r\n:2\r\n:3\r\n*2\r\n+Hello\r\n-World\r\n".chars());

        let out = parser.parse_array().unwrap();

        assert_eq!(
            out,
            RespValue::Array(vec![
                RespValue::Array(vec![
                    RespValue::Integer(1),
                    RespValue::Integer(2),
                    RespValue::Integer(3),
                ]),
                RespValue::Array(vec![
                    RespValue::SimpleString("Hello".into()),
                    RespValue::SimpleError("World".into())
                ])
            ])
        );
    }

    #[test]
    fn serialize_int() {
        assert_eq!(
            b":10\r\n".to_vec(),
            RespValue::Integer(10).serialize().unwrap()
        );

        assert_eq!(
            b":-24\r\n".to_vec(),
            RespValue::Integer(-24).serialize().unwrap()
        )
    }

    #[test]
    fn serialize_boolean() {
        assert_eq!(
            b"#t\r\n".to_vec(),
            RespValue::Boolean(true).serialize().unwrap()
        );
        assert_eq!(
            b"#f\r\n".to_vec(),
            RespValue::Boolean(false).serialize().unwrap()
        );
    }

    #[test]
    fn serialize_simple_string() {
        assert_eq!(
            b"+Hello World\r\n".to_vec(),
            RespValue::SimpleString("Hello World".into())
                .serialize()
                .unwrap()
        )
    }

    #[test]
    fn serialize_bulk_string() {
        assert_eq!(
            b"$2\r\nOK\r\n".to_vec(),
            RespValue::BulkString("OK".into()).serialize().unwrap()
        )
    }
    #[test]
    fn serialize_array() {
        assert_eq!(
            b"*2\r\n:32\r\n:-5\r\n".to_vec(),
            RespValue::Array(vec![RespValue::Integer(32), RespValue::Integer(-5)])
                .serialize()
                .unwrap()
        )
    }

    #[test]
    fn serialize_nested_array() {
        assert_eq!(
            b"*2\r\n*3\r\n:1\r\n:2\r\n:3\r\n*2\r\n+Hello\r\n-World\r\n".to_vec(),
            RespValue::Array(vec![
                RespValue::Array(vec![
                    RespValue::Integer(1),
                    RespValue::Integer(2),
                    RespValue::Integer(3),
                ]),
                RespValue::Array(vec![
                    RespValue::SimpleString("Hello".into()),
                    RespValue::SimpleError("World".into())
                ])
            ])
            .serialize()
            .unwrap()
        )
    }
}
