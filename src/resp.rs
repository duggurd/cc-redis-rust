use std::{
    error::Error,
    fmt::{format, Display},
    io::Read,
    iter::Peekable,
    str::{Chars, Split},
};

#[derive(Debug, PartialEq)]
enum RespValue {
    Array(Vec<RespValue>),
    BulkString(String),
    SimpleString(String),
    Integer(i64),
    Boolean(bool),
    SimpleError(String),
    // Error(RespError),
    None,
    Eof,
}

type RespParseResult = Result<RespValue, RespError>;

#[derive(Debug, PartialEq)]
struct RespError {
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
struct RespParser<I>
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

        Ok(RespValue::None)
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
}

// #[derive(PartialEq, Debug)]
// enum SerializationError {
//     // no data identifier in expected place
//     NoDatatype,

//     // string format is invalid
//     InvalidString,

//     //invalid resp serialization
//     InvalidResp,

//     // Array contains no item count
//     ArrayinvalidItemCount,

//     InvalidInteger,

//     InvalidBoolean,

//     InvalidBulkString,

//     BadLength,

//     MissigStringContent,
// }

// fn deserialize_resp(data: &str) -> RespValue {
//     match &data[0..1] {
//         "+" => parse_simple_str(&data[1..]),
//         "*" => parse_array(&data[1..]),
//         "$" => parse_bulk_str(&data[1..]),
//         // '-' => parse_error(chars),
//         ":" => parse_int(&data[1..]),
//         "#" => parse_boolean(&data[1..]),
//         // ',' => parse_double(chars),
//         // '(' => parse_big_numbers(chars),
//         // '!' => parse_bulk_errors(chars),
//         // '=' => parse_verbatim_strings(chars),
//         // '%' => parse_maps(chars),
//         // '~' => parse_sets(chars),
//         // '>' => parse_pushes(chars),

//         // try inline parsing
//         _ => RespValue::Error(SerializationError::NoDatatype),
//     }
// }

// fn parse_simple_str(data: &str) -> RespValue {
//     if data.chars().any(|c| c == '\r' || c == '\n') {
//         return RespValue::Error(SerializationError::InvalidString);
//     }

//     RespValue::SimpleString(data.into())
// }

// fn parse_array(data: &str) -> RespValue {
//     if &data[data.len() - 2..] != "\r\n" {
//         return RespValue::Error(SerializationError::InvalidResp);
//     }

//     let data = &data[..data.len() - 2];

//     let mut items = data.split("\r\n");

//     let item_count: usize = match items.next().unwrap().parse() {
//         Ok(v) => v,
//         Err(_) => return Resp::Error(SerializationError::ArrayinvalidItemCount),
//     };

//     if item_count == 0 {
//         return Resp::Array(Vec::new());
//     }

//     let values: Vec<RespValue> = items.map(|v| deserialize_resp(v)).collect();
//     RespValue::Array(values)
// }

// fn parse_bulk_str(data: &str) -> RespValue {
//     let mut parts = data.split("\r\n");

//     let size_str = match parts.next() {
//         Some(v) => v,
//         None => return RespValue::Error(SerializationError::InvalidBulkString),
//     };

//     let size: usize = match size_str.parse() {
//         Ok(v) => v,
//         Err(_) => return RespValue::Error(SerializationError::InvalidBulkString),
//     };

//     let str_content = match parts.next() {
//         Some(v) => v,
//         None => return RespValue::Error(SerializationError::MissigStringContent),
//     };

//     if str_content.len() != size {
//         return RespValue::Error(SerializationError::BadLength);
//     }

//     RespValue::BulkString(str_content.to_string())
// }

// fn parse_error(data: Chars) {
//     todo!()
// }

// fn parse_int(data: &str) -> RespValue {
//     match &data[0..1] {
//         "+" => match data[1..].parse() {
//             Ok(v) => RespValue::Integer(v),
//             Err(_) => RespValue::Error(SerializationError::InvalidInteger),
//         },
//         "-" => match data[..].parse() {
//             Ok(v) => RespValue::Integer(v),
//             Err(_) => RespValue::Error(SerializationError::InvalidInteger),
//         },
//         _ => match data[..].parse() {
//             Ok(v) => RespValue::Integer(v),
//             Err(_) => RespValue::Error(SerializationError::InvalidInteger),
//         },
//     }
// }

// fn parse_boolean(data: &str) -> RespValue {
//     match data.to_lowercase().as_str() {
//         "true" => RespValue::Boolean(true),
//         "false" => RespValue::Boolean(false),
//         _ => RespValue::Error(SerializationError::InvalidBoolean),
//     }
// }

// fn parse_nulls(data: Chars) {
//     todo!()
// }

// fn parse_double(data: Chars) {
//     todo!()
// }

// fn parse_big_numbers(data: Chars) {
//     todo!()
// }

// fn parse_bulk_errors(data: Chars) {
//     todo!()
// }

// fn parse_verbatim_strings(data: Chars) {
//     todo!()
// }

// fn parse_maps(data: Chars) {
//     todo!()
// }

// fn parse_sets(data: Chars) {
//     todo!()
// }

// fn parse_pushes(data: Chars) {
//     todo!()
// }

// #[cfg(test)]
// mod tests {
//     use super::*;
//     #[test]
//     fn parse_simple_strings() {
//         assert_eq!(deserialize_resp("+OK"), Resp::SimpleString("OK".into()));

//         assert_eq!(
//             deserialize_resp("+A larger string"),
//             Resp::SimpleString("A larger string".into())
//         );

//         assert_eq!(
//             deserialize_resp("+Some string\n"),
//             Resp::Error(SerializationError::InvalidString)
//         );

//         assert_eq!(
//             deserialize_resp("+Some strin\rg2"),
//             Resp::Error(SerializationError::InvalidString)
//         );

//         assert_eq!(
//             deserialize_resp("+another invalid_string\r"),
//             Resp::Error(SerializationError::InvalidString)
//         );

//         assert_eq!(
//             deserialize_resp("invalid"),
//             Resp::Error(SerializationError::NoDatatype)
//         );
//     }

//     #[test]
//     fn parse_simple_int() {
//         assert_eq!(deserialize_resp(":1"), Resp::Integer(1));
//         assert_eq!(deserialize_resp(":32"), Resp::Integer(32));
//         assert_eq!(deserialize_resp(":112313"), Resp::Integer(112313));
//         assert_eq!(deserialize_resp(":-12"), Resp::Integer(-12));
//         assert_eq!(deserialize_resp(":+15"), Resp::Integer(15));
//     }

//     #[test]
//     fn parse_boolean() {
//         assert_eq!(deserialize_resp("#true"), Resp::Boolean(true));
//         assert_eq!(deserialize_resp("#false"), Resp::Boolean(false));
//         assert_eq!(
//             deserialize_resp("#notaboolean"),
//             Resp::Error(SerializationError::InvalidBoolean)
//         );
//     }

//     #[test]
//     fn parse_bulk_string() {
//         assert_eq!(deserialize_resp("$2\r\nOK"), Resp::BulkString("OK".into()));

//         assert_eq!(
//             deserialize_resp("$44\r\nA quite long string that is quite long, yes!"),
//             Resp::BulkString("A quite long string that is quite long, yes!".into())
//         );

//         assert_eq!(
//             deserialize_resp("$12\r\nwith \r and \n"),
//             Resp::BulkString("with \r and \n".into())
//         );

//         assert_eq!(
//             deserialize_resp("$10\r\nnop"),
//             Resp::Error(SerializationError::BadLength)
//         );
//     }

//     #[test]
//     fn parse_array() {
//         let empty_aray = "*0\r\n";
//         let raw_array_1 = "*1\r\n+OK\r\n";
//         let raw_array_2 = "*2\r\n:1\r\n:2\r\n";
//         let raw_array_3 = "*2\r\n#true\r\n:10\r\n";

//         let raw_array_4 = "*3\r\n#true\r\n:10\r\n$5\r\nhello\r\n";

//         assert_eq!(
//             deserialize_resp(empty_aray),
//             Resp::Array(Vec::<Resp>::new())
//         );

//         assert_eq!(
//             deserialize_resp(raw_array_1),
//             Resp::Array(vec![Resp::SimpleString("OK".into())])
//         );

//         assert_eq!(
//             deserialize_resp(raw_array_2),
//             Resp::Array(vec![Resp::Integer(1), Resp::Integer(2)])
//         );

//         assert_eq!(
//             deserialize_resp(raw_array_3),
//             Resp::Array(vec![Resp::Boolean(true), Resp::Integer(10)])
//         );

//         assert_eq!(
//             deserialize_resp(raw_array_4),
//             Resp::Array(vec![
//                 Resp::Boolean(true),
//                 Resp::Integer(10),
//                 Resp::BulkString("hello".into())
//             ])
//         );
//     }
// }
