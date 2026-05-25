use bevy::prelude::*;
use netvy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::client::Player;

pub struct DemoProtocolPlugin;

impl Plugin for DemoProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.register_component_with_sync_mode::<Player>(netvy::SyncMode::OnChange);

        app.add_message::<DemoMessage>();
        app.register_net_message::<DemoMessage>();
    }
}

#[derive(Message, Serialize, Deserialize, Debug)]
pub struct DemoMessage(pub String);
