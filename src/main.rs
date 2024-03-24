// Uncomment this block to pass the first stage
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    // Uncomment this block to pass the first stage
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

        // Read from and respond to connection if read
        for (idx, mut stream) in &mut streams.iter().enumerate() {
            match stream.read(&mut buf) {
                Ok(n) => {
                    if n > 0 {
                        // println!("Read from stream, responding!");
                        match stream.write(b"+PONG\r\n") {
                            Ok(_) => stream.flush().unwrap(),
                            Err(_) => {
                                stream.shutdown(std::net::Shutdown::Both).unwrap();
                                to_drop.push(idx);
                            }
                        }
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
