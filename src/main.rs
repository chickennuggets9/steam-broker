use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::net::UdpSocket;
use std::io::ErrorKind;
use std::thread::sleep;
use std::time::Duration;
use std::cmp::min;

use steamworks::SteamId;
use steamworks::{ Client, User};

// TODO:
// 1. Move steam iniitialization to sb_connect handler
// 2. ... and subsequently stop steam in sb_terminate
// 3. rename sb_terminate for something 10 byte size like sb_connect :)

fn handle_connect(user: &User, buf: &[u8], from: SocketAddr, sock: &UdpSocket) {
    // sb_connect <ip:port> <server's steam id> <secure> <challenge>

    match std::str::from_utf8(buf) {
        Ok(x) => {
            println!("handle_connect: {x}");

            let args: Vec<&str> = x.split(" ").collect();

            // bruh refactor that
            let serveradr: SocketAddrV4 = match args[1].parse() {
                Ok(x) => x,
                _ => {
                    println!("can't parse ip addr");
                    return
                }
            };

            let game_server_steam_id = SteamId::from_raw(match args[2].parse() {
                Ok(x) => x,
                _ => {
                    println!("can't parse steam id");
                    return;
                },
            });

            let secure: bool = match args[3].parse() {
                Ok(x) => x,
                _ => {
                    println!("can't parse secure");
                    return;
                },
            };

            let challenge: i32 = match args[4].parse() {
                Ok(x) => x,
                _ => {
                    println!("can't parse challenge");
                    return;
                },
            };

            println!("initiate_game_connection: {serveradr} {:?} {secure} {challenge}", game_server_steam_id.raw());
            let ticket = user.initiate_game_connection(game_server_steam_id, serveradr.ip().to_bits(), serveradr.port(), secure);

            println!("steam ticket size: {:?}, sending to {from}", ticket.len());
            println!("ticket data: {:?}", ticket);

            // now construct response
            // sb_connect\n<4 byte challenge><8 byte steamid><unsigned 4 byte len><len bytes ticket>
            let mut response = Vec::from(b"\xff\xff\xff\xffsb_connect\n");
            response.extend(challenge.to_le_bytes());
            response.extend(user.steam_id().raw().to_le_bytes());
            response.extend((ticket.len() as u32).to_le_bytes());
            response.extend(ticket);

            match sock.send_to(&response, from) {
                Err(e) => {
                    println!("error sending: {e}");
                    return;
                },
                Ok(x) => x
            };
        },
        _ => return,
    }
}

fn handle_terminate(user: &User, buf: &[u8]) {
    // sb_terminate <ip:port> <challenge>
    match std::str::from_utf8(buf) {
        Ok(x) => {
            let args: Vec<&str> = x.split(" ").collect();

            // bruh refactor that
            let serveradr: SocketAddrV4 = match args[1].parse() {
                Ok(x) => x,
                _ => return,
            };

            // TODO: validate server challenge
            // let challenge: i32 = match args[2].parse() {
            //     Ok(x) => x,
            //     _ => return,
            // };

            user.terminate_game_connection(serveradr.ip().to_bits(), serveradr.port());
        },
        _ => return,
    }
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
            std::process::exit(1);
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
            handle_connect(&user, &buf[..n], from, &sock);
        } else if buf.starts_with(b"sb_terminate") {
            println!("got sb_terminate");
            handle_terminate(&user, &buf[..n]);
        } else {
            println!("Unknown packet: {:?}", &buf[0..min(n, 10)]);
        }

        // sleep for 100 msec
        sleep(Duration::from_millis(100));
    }
}
