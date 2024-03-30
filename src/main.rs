// Uncomment this block to pass the first stage
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::exit,
};

mod resp;

enum ResponseType {
    Echo(String),
    Ping,
    Error(String),
}

fn deserialize(data: &str) {
    let initial = data.chars().next().unwrap();

    match initial {
        '+' => {}
        '*' => {}
        '$' => {}
        '-' => {}
        ':' => {}
        '#' => {}
        ',' => {}
        '(' => {}
        '!' => {}
        '=' => {}
        '%' => {}
        '~' => {}
        '>' => {}
        _ => {}
    }
}

impl ResponseType {
    fn from_str(data: &str) -> Self {
        println!("{}", data);
        let mut parts = data.split(" ");

        let main_command = parts.next().unwrap();

        match main_command.to_lowercase().as_str() {
            "ping" => ResponseType::Ping,
            "echo" => {
                let echo_data = parts.collect::<Vec<&str>>().join(" ");
                let formatted = format!("${}\r\n{}\r\n", echo_data.len(), echo_data);
                ResponseType::Echo(formatted)
            }
            _ => ResponseType::Error("-ERR invalid command\r\n".to_string()),
        }
    }
}

fn main() {
    let a = "-10";

    let b = "+10";

    let c: i64 = a.parse().unwrap();
    let d: i64 = b.parse().unwrap();

    println!("{c}, {d}");
    exit(0);

    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut streams: Vec<TcpStream> = Vec::new();

    let mut to_drop = Vec::new();

    // Main event loop
    loop {
        let mut buf: [u8; 1024] = [0; 1024];

        // Pick up new connections
        match listener.accept() {
            Ok((mut _stream, _)) => {
                // println!("got connection");
                _stream.set_nonblocking(true).unwrap();
                streams.push(_stream);
            }
            Err(_) => {}
        }

        // Read from and respond to connection if readable
        for (idx, mut stream) in &mut streams.iter().enumerate() {
            match stream.read(&mut buf) {
                Ok(n) => {
                    if n > 0 {
                        let cmd = ResponseType::from_str(std::str::from_utf8(&buf).unwrap());
                        // println!("Read from stream, responding!");

                        match cmd {
                            ResponseType::Ping => {
                                stream.write(b"+PONG\r\n").unwrap();
                            }
                            ResponseType::Echo(echo) => {
                                stream.write(echo.as_bytes()).unwrap();
                            }
                            ResponseType::Error(err) => {
                                stream.write(err.as_bytes()).unwrap();
                            }
                        }
                        stream.flush().unwrap();
                        // match stream.write(b"+PONG\r\n") {
                        //     Ok(_) => stream.flush().unwrap(),
                        //     Err(_) => {
                        //         stream.shutdown(std::net::Shutdown::Both).unwrap();
                        //         to_drop.push(idx);
                        //     }
                        // }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => {
                    println!("Io error: {}", e);
                    stream.shutdown(std::net::Shutdown::Both).unwrap();
                    to_drop.push(idx);
                }
            }
        }

        // clear streams set for removal
        while let Some(idx) = to_drop.pop() {
            streams.remove(idx);
        }
    }
}
