use bevy::prelude::*;

use crate::{client::DemoClientPlugin, protocol::DemoProtocolPlugin, server::DemoServerPlugin};

mod client;
mod protocol;
mod server;

fn main() {
    let Some(run_mode) = std::env::args().nth(1) else {
        eprintln!(
            "Please specify a run mode as first argument. Possible values: 'server' 'client'"
        );
        return;
    };

    let mut app = App::new();

    if run_mode == "client" {
        app.add_plugins(DemoClientPlugin);
    } else if run_mode == "server" {
        app.add_plugins(DemoServerPlugin);
    } else {
        eprintln!("Invalid run mode: {run_mode}. Possible values: 'server' 'client'");
        return;
    }

    // NOTE: Your protocol plugin should always be added, regardless whether you're running the
    // server or the client!
    // Note that it needs to be inserted AFTER the NetvyPlugin.
    app.add_plugins(DemoProtocolPlugin);

    app.run();
}
