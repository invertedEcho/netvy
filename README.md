# netvy

## Goals
- As usual, common cases require minimal code and providing sane defaults, but advanced control is still an option
- Very straightforward API, easy to understand
- Documentation everywhere
- As few dependencies as possible
- Helpful examples with minimal code

## Usage

> [!NOTE]
> This section will change during development and is heavily WIP.

1. Add this plugin to your project:

`cargo add netvy`

2. Add the plugin on your server:

```rust
fn main() {
    use netvy::prelude::*; // A prelude import imports everything required to use netvy.
    let mut app = App:new();
    app.add_plugins(NetvyPlugin(netvy::AppType::Server));
}
```

3. Add the plugin on your client:

```rust
fn main() {
    let mut app = App:new();
    app.add_plugins(NetvyPlugin(AppType::Client));
}
```

4. Add a protocol plugin. The protocol plugin is just a normal bevy plugin defined by you, that:
- registers components,
- registers messages

that should be sent across clients

### Syncing entities
In order for netvy to know which entities should be synced across clients, you will have to insert the `NetEntity` component into them:

```rust
commands.spawn((
    // components from you...
    NetEntity,
    // even more components from you...
));
```

Note that not every component of this entity will be synced across clients! Please read [Component registration](#component-registration)

### Component registration

In order for netvy to know which components in entities should be synced across clients, you will have to "register" them.
You do so by calling `register_component` on your bevy app:
```rust
#[derive(Component, Serialize, Deserialize)] // Note that your component must derive `Serialize` and `Deserialize`
pub struct YourComponent {
    pub demo: String,
    pub demo2: f32
};

fn main() {
    let mut app = App:new();
    app.register_component::<YourComponent>();
}
```

#### Sync modes (optional)

When registering your components, you can specify when updates should be sent. Currently, the following modes are supported:

```rust
// the component Player will only be sent to other clients whenever it changes.
app.register_component_with_sync_mode::<Player>(netvy::SyncMode::OnChange);

// the component ArbitraryPosition will be sent to other clients every 0.05 seconds, right now even when there were no changes to the component. This will probably change in the future.
app.register_component_with_sync_mode::<ArbitraryPosition>(SyncMode::FixedRate(0.05));
```

### Syncing position of entities

One of the most used aspects of multiplayer/network frameworks is probably syncing position of entities across clients.
You can do so by simply inserting the `SyncPosition` component into entities:

```rust
commands.spawn((
    // your components here..
    SyncPosition::default(), // right now, the default will enable linear interpolation, so updates look smoother
    // more components here..
));
```

Alternatively you can also disable linear interpolation:

```rust
commands.spawn((
    SyncPosition {
        linear_interpolation: false,
    }
))
```

### Network messages

You will most likely want to send your own defined messages across clients / servers.

Right now, you can send messages from:

 - a client to the server
 - the server to all connected clients
