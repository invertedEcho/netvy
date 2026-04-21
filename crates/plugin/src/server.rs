use bevy::prelude::*;

/// This message gets written whenever a new server is listening
#[derive(Message)]
pub struct ServerListening(pub u16);
