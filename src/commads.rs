use std::{
    error::Error,
    fmt::Display,
    iter::Peekable,
    time::{Duration, Instant},
};

use crate::{resp::RespValue, server::StoredValue};

#[derive(PartialEq, Debug)]
pub struct SetCommand {
    pub key: String,
    pub value: StoredValue,
}

#[derive(PartialEq, Debug)]
pub enum InfoType {
    Replication,
}

impl InfoType {
    pub fn from_str(value: &str) -> Result<InfoType, CommandErr> {
        match value {
            "replication" => Ok(InfoType::Replication),
            s => Err(CommandErr {
                msg: format!("invalid info specifier {}", s),
            }),
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum ReplconfType {
    ListeningPort(u32),
    Capa(String),
}

#[derive(PartialEq, Debug)]
pub enum Command {
    Ping,
    Echo(RespValue),
    Llen,
    Shutdown,
    Set(SetCommand),
    Get(String),
    Info(InfoType),
    Replconf(ReplconfType),
}

#[derive(Debug)]
pub struct CommandErr {
    msg: String,
}

impl Error for CommandErr {}

pub type CommandParseResult = Result<Command, CommandErr>;

impl Display for CommandErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

pub struct CommandParser<I: Iterator<Item = RespValue>> {
    resp_it: Peekable<I>,
    idx: usize,
}

impl<I: Iterator<Item = RespValue>> CommandParser<I> {
    pub fn new(resp_it: I) -> Self {
        Self {
            resp_it: resp_it.peekable(),
            idx: 0,
        }
    }

    pub fn err(&mut self, msg: String) -> CommandParseResult {
        Err(CommandErr { msg: msg })
    }

    fn next(&mut self) -> Option<RespValue> {
        self.idx += 1;
        self.resp_it.next()
    }

    fn peek(&mut self) -> Option<&RespValue> {
        self.resp_it.peek()
    }

    pub fn echo(&mut self) -> CommandParseResult {
        match self.next() {
            Some(s) => Ok(Command::Echo(s)),
            None => self.err("item after 'ECHO' expected".into()),
        }
    }

    pub fn ping(&mut self) -> CommandParseResult {
        Ok(Command::Ping)
    }

    pub fn info(&mut self) -> CommandParseResult {
        let next_value = match self.next() {
            Some(RespValue::BulkString(s) | RespValue::SimpleString(s)) => s,
            Some(s) => return self.err(format!("invalid type expected SS or BS got: {:?}", s)),
            None => return self.err("expected infotype sepcifier after INFO command".into()),
        };

        let info_type = match InfoType::from_str(&next_value) {
            Ok(v) => v,
            Err(e) => return self.err(e.to_string()),
        };

        Ok(Command::Info(info_type))
    }

    pub fn set(&mut self) -> CommandParseResult {
        let key = match self.next() {
            Some(RespValue::BulkString(s) | RespValue::SimpleString(s)) => s,
            Some(s) => return self.err(format!("invalid typee xpected SS or BS got: {:?}", s)),
            None => return self.err("key expected after set".into()),
        };

        let value = match self.next() {
            Some(RespValue::BulkString(s) | RespValue::SimpleString(s)) => s,
            Some(s) => return self.err(format!("invalid typee xpected SS or BS got: {:?}", s)),
            None => return self.err("value expected after key in set".into()),
        };

        let px = match self.peek() {
            Some(RespValue::BulkString(s) | RespValue::SimpleString(s)) => match s.as_str() {
                "PX" => {
                    self.next().unwrap();
                    match self.next() {
                        Some(RespValue::Integer(i)) if i > 0 => {
                            Some(Instant::now() + Duration::from_millis(i as u64))
                        }
                        Some(r) => {
                            return self.err(format!(
                                "expected positive integer after PX in SET, got {:?}",
                                r
                            ))
                        }
                        None => return self.err("expected value after PX".into()),
                    }
                }
                _ => None,
            },
            Some(_) => None,
            None => None,
        };

        let set_command = SetCommand {
            key: key,
            value: StoredValue::new(value, px),
        };

        Ok(Command::Set(set_command))
    }

    pub fn get(&mut self) -> CommandParseResult {
        match self.next() {
            Some(RespValue::BulkString(s) | RespValue::SimpleString(s)) => Ok(Command::Get(s)),
            Some(s) => self.err(format!("invalid type,e xpected SS or BS got: {:?}", s)),
            None => self.err("key expected after get".into()),
        }
    }

    pub fn shutdown(&mut self) -> CommandParseResult {
        Ok(Command::Shutdown)
    }

    pub fn parse_next(&mut self) -> CommandParseResult {
        let raw_cmd = match self.next() {
            Some(RespValue::BulkString(s) | RespValue::SimpleString(s)) => s,
            _ => {
                return Err(CommandErr {
                    msg: "can only parse command from BulkString or SimpleString".into(),
                })
            }
        };

        let cmd = match raw_cmd.to_uppercase().as_str() {
            "PING" => self.ping()?,
            "ECHO" => self.echo()?,
            "SHUTDOWN" => self.shutdown()?,
            "SET" => self.set()?,
            "GET" => self.get()?,
            "INFO" => self.info()?,
            a => return self.err(format!("invalid command: '{}' provided", a)),
        };

        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_ping() {
        let resp_values = vec![RespValue::BulkString("ping".into())];
        let mut parser = CommandParser::new(resp_values.into_iter());

        assert_eq!(Command::Ping, parser.parse_next().unwrap());
    }

    #[test]
    fn test_echo() {
        let resp_values = vec![
            RespValue::BulkString("echo".into()),
            RespValue::BulkString("hello world".into()),
        ];
        let mut parser = CommandParser::new(resp_values.into_iter());

        assert_eq!(
            Command::Echo(RespValue::BulkString("hello world".into())),
            parser.parse_next().unwrap()
        );
    }
}
