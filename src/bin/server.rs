//! Issue #225: the dedicated server of Fluffy Racers (`docs/SERVEUR.md`): the game without a
//! window, a renderer, sounds or a GPU, hosting the room « SERVEUR » for good, with its own
//! signaling. `fluffy-racers-server --help`.

#[cfg(not(target_arch = "wasm32"))]
use std::net::{Ipv4Addr, SocketAddr};

#[cfg(not(target_arch = "wasm32"))]
use fluffy_racers::net::dedicated::{DedicatedServer, SERVER_HELP, ServerOptions};
#[cfg(not(target_arch = "wasm32"))]
use fluffy_racers::net::signal_server::start_signaling;

/// No server in the browser: the wasm build only checks that this compiles.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let options = ServerOptions::from_args(std::env::args().skip(1));
    if options.help {
        println!("{SERVER_HELP}");
        return;
    }
    if !options.refused.is_empty() {
        eprintln!(
            "Options illisibles : {}\n\n{SERVER_HELP}",
            options.refused.join(", ")
        );
        std::process::exit(2);
    }
    let mut port = options.port;
    if options.signal.is_none() {
        // Every interface: the players reach it from the network.
        match start_signaling(SocketAddr::from((Ipv4Addr::UNSPECIFIED, options.port))) {
            Ok(addr) => {
                port = addr.port();
                println!("Mise en relation sur le port {port} (WebSocket).");
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    let mut server = DedicatedServer::new(options.room.clone(), options.signal_url(port));
    server.stun = options.stun.clone();
    println!("Partie « {} » : le serveur l'héberge.", options.room);
    fluffy_racers::server_app(server).run();
}
