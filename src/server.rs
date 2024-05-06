use std::collections::HashMap;
use std::env;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

pub fn gen_master_id() -> String {
    let mut rnd = String::new();

    for _ in 0..40 {
        let seed = format!(
            "{:?}",
            SystemTime::elapsed(&SystemTime::UNIX_EPOCH).unwrap()
        );

        let mut v: u32 = 0;

        for b in seed.as_bytes() {
            v += *b as u32
        }

        let val = &format!("{:x}", v % 86 * 11)[0..1];

        rnd.push_str(val);
    }
    rnd
}

pub struct CliArgs {
    pub port: Option<u32>,
    pub replicaof: Option<(String, u32)>,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

impl CliArgs {
    pub fn from_args() -> Result<CliArgs> {
        let mut args = env::args().peekable();

        let _ = args.next();

        let mut port = None;
        let mut replicaof = None;

        while args.peek().is_some() {
            match args.next().unwrap().as_str() {
                "--port" => {
                    let p = args.next().unwrap();
                    port = Some(p.parse().unwrap())
                }
                "--replicaof" => {
                    let host = args.next().unwrap();
                    let port = args.next().unwrap();

                    replicaof = Some((host, port.parse().unwrap()))
                }
                a => return Err(format!("unexpected arg: {}", a).into()),
            }
        }
        Ok(Self { port, replicaof })
    }
}

#[derive(PartialEq, Debug)]
pub struct StoredValue {
    value: String,
    px: Option<Instant>,
}

impl StoredValue {
    pub fn new(value: String, px: Option<Instant>) -> StoredValue {
        Self { value, px }
    }
}

pub struct Replication {
    pub role: ServerRole,
    pub replicaof: Option<(String, u32)>,
    pub master_replid: String,
    pub master_repl_offset: u64,
}

impl Replication {
    fn serialize(&self) -> String {
        let role = self.role.as_str();
        let master_replid = format!("master_replid:{}", self.master_replid);

        let master_repl_offset = format!("master_repl_offset:{}", self.master_repl_offset);

        let serialized = [role, master_replid.as_str(), master_repl_offset.as_str()].join("\r\n");
        String::from_utf8(RespValue::BulkString(serialized).serialize().unwrap()).unwrap()
    }
}

impl Default for Replication {
    fn default() -> Self {
        Self {
            role: ServerRole::Master,
            replicaof: None,
            master_replid: gen_master_id(),
            master_repl_offset: 0,
        }
    }
}

pub enum ServerRole {
    Master,
    Slave,
}

impl ServerRole {
    fn as_str(&self) -> &str {
        match self {
            Self::Master => "role:master",
            Self::Slave => "role:slave",
        }
    }
}

pub struct Server {
    listener: TcpListener,
    streams: Vec<TcpStream>,
    to_close: Vec<usize>,
    shutdown: bool,
    storage: HashMap<String, StoredValue>,
    replication: Replication,
    master_stream: Option<TcpStream>,
}

use crate::commads::InfoType;
use crate::Command;
use crate::CommandParser;
use crate::RespParser;
use crate::RespValue;

impl Server {
    pub fn new<A: ToSocketAddrs>(address: A, replicaof: Option<(String, u32)>) -> Self {
        let listener = TcpListener::bind(address).unwrap();
        listener.set_nonblocking(true).unwrap();

        let mut replication = Replication::default();
        let mut master_stream = None;

        // Create a replica server
        if let Some(repl) = replicaof {
            replication.role = ServerRole::Slave;
            replication.replicaof = Some(repl.clone());

            let mut stream = TcpStream::connect(format!("{}:{}", repl.0, repl.1)).unwrap();

            // handshake 1
            let _ = stream.write(b"*1\r\n$4\r\nping\r\n").unwrap();

            let mut buf: [u8; 1024] = [0; 1024];

            let _ = stream.read(&mut buf).unwrap();

            println!("{}", String::from_utf8(buf.to_vec()).unwrap());

            master_stream = Some(stream);
        };

        Server {
            listener,
            streams: Vec::<TcpStream>::new(),
            to_close: Vec::<usize>::new(),
            shutdown: false,
            storage: HashMap::<String, StoredValue>::new(),
            replication,
            master_stream: master_stream,
        }
    }

    pub fn poll_streams(&mut self) {
        // Read from and respond to connection if readable
        for (idx, mut stream) in self.streams.iter().enumerate() {
            if self.shutdown {
                println!("shutting down stream");
                stream.shutdown(Shutdown::Both).unwrap();
                continue;
            }

            let mut buf: [u8; 1024] = [0; 1024];

            match stream.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let a = buf;
                    println!("{}", String::from_utf8(a.to_vec()).unwrap());

                    let parsed_resp =
                        match RespParser::new(String::from_utf8(buf.to_vec()).unwrap().chars())
                            .parse_next()
                        {
                            Ok(r) => r,
                            Err(e) => {
                                let _ = stream.write(e.to_string().as_bytes()).unwrap();
                                stream.flush().unwrap();
                                continue;
                            }
                        };

                    println!("parsed value: {:?}", parsed_resp);

                    let inner_cmd = match parsed_resp {
                        RespValue::Array(a) => a,
                        _ => {
                            let _ = stream
                                .write(
                                    format!("invalid type expected Array, got {:?}", parsed_resp)
                                        .as_bytes(),
                                )
                                .unwrap();
                            stream.flush().unwrap();
                            continue;
                        }
                    };

                    let cmd = match CommandParser::new(inner_cmd.into_iter()).parse_next() {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = stream.write(e.to_string().as_bytes()).unwrap();
                            stream.flush().unwrap();
                            continue;
                        }
                    };

                    let resp = match cmd {
                        Command::Ping => {
                            RespValue::SimpleString("PONG".into()).serialize().unwrap()
                        }
                        Command::Echo(mut s) => s.serialize().unwrap(),
                        Command::Llen => todo!(),
                        Command::Shutdown => {
                            self.shutdown = true;
                            RespValue::SimpleString("OK".into()).serialize().unwrap()
                        }
                        Command::Set(set_command) => {
                            self.storage.insert(set_command.key, set_command.value);
                            RespValue::BulkString("OK".into()).serialize().unwrap()
                        }
                        Command::Get(key) => {
                            let v = match self.storage.get(&key) {
                                Some(v) => &v.value,
                                None => "",
                            };
                            RespValue::BulkString(v.to_string()).serialize().unwrap()
                        }
                        Command::Info(t) => match t {
                            InfoType::Replication => {
                                self.replication.serialize().as_bytes().to_vec()
                            }
                        },
                        Command::Replconf(_s) => vec![0],
                    };

                    let _ = stream.write(&resp[0..]).unwrap();
                    stream.flush().unwrap();
                }
                // 0 bytes
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => {
                    println!("Io error: {}", e);
                    stream.shutdown(std::net::Shutdown::Both).unwrap();
                    self.to_close.push(idx);
                }
            }

            sleep(Duration::from_millis(10));
        }

        // clear streams set for removal
        while let Some(idx) = self.to_close.pop() {
            self.streams.remove(idx);
        }
    }

    /// veru good optimization :)
    pub fn remove_expired(&mut self) {
        let mut to_remove = Vec::new();
        for (k, v) in self.storage.iter() {
            if v.px.is_some_and(|px| px <= Instant::now()) {
                to_remove.push(k.to_string());
            }
        }

        for k in to_remove {
            self.storage.remove(&k);
        }
    }

    pub fn run(&mut self) {
        loop {
            // Pick up new connections
            if let Ok((stream, _)) = self.listener.accept() {
                println!("got connection");
                stream.set_nonblocking(true).unwrap();
                self.streams.push(stream);
            }

            self.poll_streams();
            self.remove_expired();

            //cleanup was done in poll, safe to break
            if self.shutdown {
                println!("shuttding down server");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread::{self, JoinHandle};

    use super::*;

    const ADDR: &'static str = "127.0.0.1:6379";

    fn stream_helper(to_send: &str) -> Result<String> {
        let mut stream = TcpStream::connect(tests::ADDR).unwrap();
        let mut buf: Vec<u8> = Vec::new();
        stream.write(to_send.as_bytes()).unwrap();
        stream.flush().unwrap();

        match stream.read_to_end(&mut buf) {
            Ok(_) => Ok(String::from_utf8(buf).unwrap()),
            Err(_) => Err("an error".into()),
        }
    }

    /// Creates and runs the server
    /// use a stream to write to the server
    /// Join on the returned [`JoinHandle`]
    fn server_helper() -> JoinHandle<()> {
        let mut server = Server::new(ADDR, None);

        let handle = thread::spawn(move || {
            server.run();
        });

        return handle;
    }

    #[test]
    fn test_server_creation() {
        let handle = server_helper();

        let _ = stream_helper("*1\r\n+SHUTDOWN\r\n");

        handle.join().unwrap();
    }

    #[test]
    fn test_ping_command() {
        let handle = server_helper();

        let resp = stream_helper("*1\r\n$4\r\nPING\r\n").unwrap();

        let _ = stream_helper("*1\r\n+SHUTDOWN\r\n");

        handle.join().unwrap();

        assert_eq!(resp, String::from("+PONG\r\n"));
    }

    #[test]
    fn test_echo_command() {
        let handle = server_helper();

        let resp = stream_helper("*1\r\n$4\r\nECHO\r\n$2\r\nOK\r\n").unwrap();
        let _ = stream_helper("*1\r\n+SHUTDOWN\r\n");

        handle.join().unwrap();
        assert_eq!(resp, "$2\r\nOK\r\n")
    }
}
