use bevy::prelude::*;
use netvy::prelude::*;

use crate::client::Player;

pub struct DemoProtocolPlugin;

impl Plugin for DemoProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.register_component_with_sync_mode::<Player>(netvy::SyncMode::OnChange);
    }
}
