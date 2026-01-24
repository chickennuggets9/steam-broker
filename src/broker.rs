use std::{
    io::{Cursor, ErrorKind, Write},
    net::{SocketAddr, SocketAddrV4, UdpSocket},
    thread::sleep,
    time::Duration,
};

use steamworks::{Client, SteamId, User};

use crate::{BrokerError, args::Args};

struct SteamApi {
    client: Client,
    user: User,
}

impl SteamApi {
    fn new() -> Result<Self, BrokerError> {
        println!("Initializing Steam...");

        let client = Client::init()?;

        let utils = client.utils();
        println!("Utils:");
        println!("AppId: {:?}", utils.app_id());

        let user = client.user();
        println!("User:");
        println!("SteamID: {:?}", user.steam_id());

        Ok(Self { client, user })
    }
}

pub struct Broker {
    sock: UdpSocket,
    api: Option<SteamApi>,
}

impl Broker {
    pub fn new() -> Result<Self, BrokerError> {
        let addr = "127.0.0.1:27420";
        let sock = UdpSocket::bind(addr).map_err(BrokerError::CreateSocket)?;
        println!("Started UDP server at {addr}");

        Ok(Self { sock, api: None })
    }

    fn handle_connect(&mut self, args: &[u8], from: &SocketAddr) -> Result<(), BrokerError> {
        let mut args = Args::new(args)?;
        println!("handle_connect: {}", args.as_str());

        // sb_connect <ip:port> <server's steam id> <secure> <challenge>
        let serveradr: SocketAddrV4 = args.parse("ip addr")?;
        let game_server_steam_id: u64 = args.parse("steam id")?;
        let secure: bool = args.parse("secure")?;
        let challenge: i32 = args.parse("challenge")?;

        let api = match &self.api {
            Some(api) => {
                // FIXME: what if api is already initialized?
                println!("warning: steam api is already initialized");
                api
            }
            None => {
                self.api = Some(SteamApi::new()?);
                self.api.as_ref().unwrap()
            }
        };

        println!(
            "initiate_game_connection: {serveradr} {game_server_steam_id} {secure} {challenge}"
        );
        let ticket = api.user.initiate_game_connection(
            SteamId::from_raw(game_server_steam_id),
            serveradr.ip().to_bits(),
            serveradr.port(),
            secure,
        );

        println!("steam ticket size: {:?}, sending to {from}", ticket.len());
        println!("ticket data: {:?}", ticket);

        // now construct response
        // sb_connect\n<4 byte challenge><8 byte steamid><unsigned 4 byte len><len bytes ticket>
        let mut cur = Cursor::new(Vec::with_capacity(1024));
        cur.write_all(b"\xff\xff\xff\xffsb_connect\n")?;
        cur.write_all(&challenge.to_le_bytes())?;
        cur.write_all(&api.user.steam_id().raw().to_le_bytes())?;
        cur.write_all(&(ticket.len() as u32).to_le_bytes())?;
        cur.write_all(&ticket)?;
        self.sock
            .send_to(&cur.into_inner(), from)
            .map_err(BrokerError::Send)?;

        Ok(())
    }

    fn handle_terminate(&mut self, args: &[u8], _from: &SocketAddr) -> Result<(), BrokerError> {
        let mut args = Args::new(args)?;

        // sb_terminate <ip:port> <challenge>
        let serveradr: SocketAddrV4 = args.parse("ip addr")?;
        let _challenge: i32 = args.parse("challenge")?;

        // TODO: validate server challenge

        if let Some(api) = &self.api {
            api.user
                .terminate_game_connection(serveradr.ip().to_bits(), serveradr.port());
        } else {
            // FIXME: what if api is not initialized?
            println!("warning: steam api is not initialized");
        }

        Ok(())
    }

    pub fn run(&mut self) -> Result<(), BrokerError> {
        let mut buf = [0; 1024];

        loop {
            if let Some(api) = &self.api {
                api.client.run_callbacks();
            }

            let (buf, from) = match self.sock.recv_from(&mut buf) {
                Ok((n, from)) => (&buf[..n], from),
                Err(e) => match e.kind() {
                    ErrorKind::TimedOut | ErrorKind::WouldBlock => continue,
                    _ => return Ok(()),
                },
            };

            let handlers = &[
                ("sb_connect", Self::handle_connect as fn(_, _, _) -> _),
                ("sb_terminate", Self::handle_terminate),
            ];

            let result = handlers.iter().find_map(|(prefix, handler)| {
                let args = buf.strip_prefix(prefix.as_bytes())?;
                Some((prefix, handler, args))
            });

            if let Some((prefix, handler, args)) = result {
                println!("got {prefix}");
                if let Err(err) = handler(self, args, &from) {
                    println!("error: {err}");
                }
            } else {
                println!("Unknown packet: {:?}", &buf[..buf.len().min(10)]);
            }

            // sleep for 100 msec
            sleep(Duration::from_millis(100));
        }
    }
}
