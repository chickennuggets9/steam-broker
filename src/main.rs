use std::{
    cmp::min,
    io::{self, ErrorKind},
    net::{SocketAddr, SocketAddrV4, UdpSocket},
    process,
    str::{self, FromStr, SplitAsciiWhitespace, Utf8Error},
    thread::sleep,
    time::Duration,
};

use steamworks::{Client, SteamId, User};
use thiserror::Error;

#[derive(Error, Debug)]
enum BrokerError {
    #[error("send error, {0}")]
    Send(io::Error),
    #[error("invalid string, {0}")]
    Utf8(#[from] Utf8Error),
    #[error("invalid args, expect \"{0}\"")]
    Expect(&'static str),
    #[error("invalid args, missing \"{0}\"")]
    Missing(&'static str),
    #[error("failed to parse \"{0}\"")]
    Parse(&'static str),
}

// TODO:
// 1. Move steam iniitialization to sb_connect handler
// 2. ... and subsequently stop steam in sb_terminate
// 3. rename sb_terminate for something 10 byte size like sb_connect :)

fn expect(args: &mut SplitAsciiWhitespace, expect: &'static str) -> Result<(), BrokerError> {
    if args.next().ok_or(BrokerError::Missing(expect))? == expect {
        Ok(())
    } else {
        Err(BrokerError::Expect(expect))
    }
}

fn parse_next<T>(args: &mut SplitAsciiWhitespace, msg: &'static str) -> Result<T, BrokerError>
where
    T: FromStr,
{
    args.next()
        .ok_or(BrokerError::Missing(msg))?
        .parse()
        .map_err(|_| BrokerError::Parse(msg))
}

fn handle_connect(
    user: &User,
    buf: &[u8],
    from: SocketAddr,
    sock: &UdpSocket,
) -> Result<(), BrokerError> {
    let args = str::from_utf8(buf)?;
    println!("handle_connect: {args}");

    // sb_connect <ip:port> <server's steam id> <secure> <challenge>
    let args = &mut args.split_ascii_whitespace();
    expect(args, "sb_connect")?;
    let serveradr: SocketAddrV4 = parse_next(args, "ip addr")?;
    let game_server_steam_id: u64 = parse_next(args, "steam id")?;
    let secure: bool = parse_next(args, "secure")?;
    let challenge: i32 = parse_next(args, "challenge")?;

    println!("initiate_game_connection: {serveradr} {game_server_steam_id} {secure} {challenge}");
    let ticket = user.initiate_game_connection(
        SteamId::from_raw(game_server_steam_id),
        serveradr.ip().to_bits(),
        serveradr.port(),
        secure,
    );

    println!("steam ticket size: {:?}, sending to {from}", ticket.len());
    println!("ticket data: {:?}", ticket);

    // now construct response
    // sb_connect\n<4 byte challenge><8 byte steamid><unsigned 4 byte len><len bytes ticket>
    let mut response = Vec::from(b"\xff\xff\xff\xffsb_connect\n");
    response.extend(challenge.to_le_bytes());
    response.extend(user.steam_id().raw().to_le_bytes());
    response.extend((ticket.len() as u32).to_le_bytes());
    response.extend(ticket);

    sock.send_to(&response, from).map_err(BrokerError::Send)?;
    Ok(())
}

fn handle_terminate(user: &User, buf: &[u8]) -> Result<(), BrokerError> {
    let args = str::from_utf8(buf)?;

    // sb_terminate <ip:port> <challenge>
    let args = &mut args.split_ascii_whitespace();
    expect(args, "sb_terminate")?;
    let serveradr: SocketAddrV4 = parse_next(args, "ip addr")?;
    let _challenge: i32 = parse_next(args, "challenge")?;

    // TODO: validate server challenge

    user.terminate_game_connection(serveradr.ip().to_bits(), serveradr.port());
    Ok(())
}

fn main() {
    println!("Welcome to Steam Broker!");

    println!("Initializing Steam...");

    let client = Client::init().unwrap();

    let utils = client.utils();
    println!("Utils:");
    println!("AppId: {:?}", utils.app_id());

    let user = client.user();
    println!("User:");
    println!("SteamID: {:?}", user.steam_id());

    let addr = "127.0.0.1:27420";
    let sock = match UdpSocket::bind(addr) {
        Ok(x) => x,
        Err(e) => {
            println!("Error creating socket: {:?}", e);
            process::exit(1);
        }
    };
    println!("Started UDP server at {addr}");

    loop {
        client.run_callbacks();

        let mut buf = [0; 1024];
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(x) => x,
            Err(e) => match e.kind() {
                ErrorKind::TimedOut | ErrorKind::WouldBlock => continue,
                _ => break,
            },
        };

        if buf.starts_with(b"sb_connect") {
            println!("got sb_connect");
            if let Err(err) = handle_connect(&user, &buf[..n], from, &sock) {
                println!("error: {err}");
            }
        } else if buf.starts_with(b"sb_terminate") {
            println!("got sb_terminate");
            if let Err(err) = handle_terminate(&user, &buf[..n]) {
                println!("error: {err}");
            }
        } else {
            println!("Unknown packet: {:?}", &buf[0..min(n, 10)]);
        }

        // sleep for 100 msec
        sleep(Duration::from_millis(100));
    }
}
