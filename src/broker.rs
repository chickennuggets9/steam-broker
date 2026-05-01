use std::{
    cell::OnceCell,
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

#[derive(Copy, Clone, PartialEq, Eq)]
enum State {
    Ready,
    TicketRequsted { challenge: i32 },
}

pub struct Broker {
    sock: UdpSocket,
    api: OnceCell<SteamApi>,
    state: State,
}

impl Broker {
    pub fn new() -> Result<Self, BrokerError> {
        let addr = "127.0.0.1:27420";
        let sock = UdpSocket::bind(addr).map_err(BrokerError::CreateSocket)?;
        println!("Started UDP server at {addr}");

        Ok(Self {
            sock,
            api: OnceCell::new(),
            state: State::Ready,
        })
    }

    fn handle_connect(&mut self, args: &[u8], from: &SocketAddr) -> Result<(), BrokerError> {
        let mut args = Args::new(args)?;
        println!("handle_connect: {}", args.as_str());

        // sb_connect <ip:port> <server's steam id> <secure> <challenge>
        let serveradr: SocketAddrV4 = args.parse("ip addr")?;
        let game_server_steam_id: u64 = args.parse("steam id")?;
        let secure: bool = args.parse("secure")?;
        let challenge: i32 = args.parse("challenge")?;

        if self.state != State::Ready {
            // FIXME: what if we didn't receive a terminate request from a client?
            return Err(BrokerError::Custom("ticket already requested"));
        }

        // TODO: replace with OnceCell::get_or_try_init when stabilized
        let api = match self.api.get() {
            Some(api) => api,
            None => {
                let api = SteamApi::new()?;
                self.api.get_or_init(|| api)
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
        self.state = State::TicketRequsted { challenge };

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
        let challenge: i32 = args.parse("challenge")?;

        let State::TicketRequsted { challenge: c } = self.state else {
            return Err(BrokerError::Custom("ticket is not requested"));
        };

        // TODO: check client ip?

        if c != challenge {
            return Err(BrokerError::Custom("invalid challenge"));
        }

        self.api
            .get()
            .expect("initialized steam api")
            .user
            .terminate_game_connection(serveradr.ip().to_bits(), serveradr.port());
        self.state = State::Ready;

        Ok(())
    }

    pub fn run(&mut self) -> Result<(), BrokerError> {
        let mut buf = [0; 1024];

        loop {
            if let Some(api) = self.api.get() {
                api.client.run_callbacks();
            }

            // TODO: set socket timeout?
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
