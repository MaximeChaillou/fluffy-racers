//! Issue #225: the dedicated server's own signaling, in the same program: the full-mesh server of
//! `matchbox_signaling` (the crate of `matchbox_server`, the same version as `matchbox_socket`'s
//! protocol: its rooms are the paths, `ws://…:3536/fluffy-racers-SERVEUR`), on a thread of its own
//! with a small tokio runtime. Nothing to run beside the server. Native only.

use std::net::SocketAddr;

use matchbox_signaling::SignalingServer;

/// Starts the signaling on `addr` (port 0: any free one) and returns where it listens. The port
/// is taken here (an error says it is in use); the server answers on its own thread from then on.
pub fn start_signaling(addr: SocketAddr) -> Result<SocketAddr, String> {
    let mut server = SignalingServer::full_mesh_builder(addr).cors().build();
    let bound = server
        .bind()
        .map_err(|error| format!("signalisation sur {addr} impossible : {error}"))?;
    std::thread::Builder::new()
        .name("signalisation".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    bevy::log::error!("signaling: no runtime: {error}");
                    return;
                }
            };
            if let Err(error) = runtime.block_on(server.serve()) {
                bevy::log::error!("signaling: stopped: {error}");
            }
        })
        .map_err(|error| format!("signalisation : {error}"))?;
    Ok(bound)
}
