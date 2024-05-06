mod commads;
mod resp;
mod server;

use commads::{Command, CommandParser};
use resp::{RespParser, RespValue};
use server::{CliArgs, Replication, Server, ServerRole};

use std::env;

// const ADDR: &'static str = "127.0.0.1:6379";
fn main() -> std::io::Result<()> {
    let args = CliArgs::from_args().unwrap();

    let port = args.port.unwrap_or(6380);

    let mut server = Server::new(format! {"127.0.0.1:{}", port}, args.replicaof);
    server.run();

    Ok(())
}
