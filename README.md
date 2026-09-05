# netvy

netvy is a multiplayer framework for the [bevy game engine](https://bevy.org/), aiming to use as little dependencies as possible.
With netvy, you can implement multiplayer functionality into your bevy app/game.

Unlike other multiplayer solutions for bevy, it does not built on top of other technologies.
You also don't have to write your own I/O backend. netvy has everything you need.
Instead, everything from networking to replication is written from scratch.

There are currently only three dependencies:
- `bevy` itself
- serde (serialization)
- bincode

## Goals
- As usual, common cases require minimal code and providing sane defaults, but advanced control is still an option
- Very straightforward API, easy to understand
- Documentation everywhere
- As few dependencies as possible
- Helpful examples with minimal code
- Netvy provides detailed logs, as debugging networking in games is already not that easy

> [!NOTE]
> This documentation will change during development and is heavily WIP.

## Documentation

1. [Getting started](#getting-started)
2. [Running a client and server](#running-a-client-and-server)
3. [Component registration](#component-registration)
    - [Sync modes](#sync-modes)
4. [Syncing transform of entities](#syncing-transform-of-entities)
    - [Syncing position](#syncing-position)
    - [Syncing rotation](#syncing-rotation)
    - [Teleporting a client-authoritive entity on the server](#teleporting-a-client-authoritive-entity-on-the-server)
5. [Network messages](#network-messages)
6. [Disconnecting from server](#disconnecting-from-server)

### Getting started

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

### Running a server and a client

Now that the plugins are setup, you can start with creating a client and a server.

- To start a server, you first spawn an entity with required server components, and then trigger the `StartServer` event, using this entity:

```rust
fn start_server(mut commands: Commands) {
    let server_entity = commands
        .spawn((
            Server,
            TargetAddress {
                address: "0.0.0.0".to_string(),
                port: 8080,
            },
        ))
        .id();

    commands.trigger(StartServer { server_entity });
}
```

- To create a client and connect, you first spawn an entity with the required client components, and then trigger the `ConnectToServer` event, using this entity:

```rust
let client_entity = commands
    .spawn((
        Client,
        TargetAddress {
            address: "0.0.0.0".to_string(),
            port: SERVER_PORT,
        },
    ))
    .id();

commands.trigger(ConnectToServer { client_entity });
```

In order to know whether the client succesfully connected to the server, you can query for the ConnectionState component on your client_entity. This should be ConnectionState::Connected to indicate a successful connect.

In the near future, an event will be added, that can be observered, to know, when the connection was succesful.


### Replicating and syncing entities
In order for netvy to know which entities should be replicated and synced across clients, you will have to insert the `ReplicateEntity` component into them:

```rust
commands.spawn((
    // components from you...
    ReplicateEntity,
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

#### Sync modes

When registering your components, you can specify when updates should be sent. Currently, the following modes are supported:

```rust
// the component Player will only be sent to other clients whenever it changes.
app.register_component_with_sync_mode::<Player>(netvy::SyncMode::OnChange);

// the component ArbitraryPosition will be sent to other clients every 0.05 seconds, right now even when there were no changes to the component. This will probably change in the future.
app.register_component_with_sync_mode::<ArbitraryPosition>(SyncMode::FixedRate(0.05));
```

### Syncing transform of entities

#### Syncing position

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

#### Syncing rotation

You can do so by simply inserting the `SyncRotation` component into entities:

```rust
commands.spawn((
    // your components here..
    SyncRotation::default(), // right now, the default will enable linear interpolation, so updates look smoother
    // more components here..
));
```

Note that `SyncRotation` has a couple more fields available, such as locking specific axes.

If you want to use a different entity (such as a camera that is a child of an entity) as the source for the rotation of that net entity, insert the `AlternateSourceRotation` component into the entity which rotation should be used. You also need to specify the NetEntityId, so netvy knows for which entity this source rotation is.

The exact same goes for if you want to have netvy apply the rotation to another entity. Insert the `AlternateTargetRotation` component into the corresponding rotation.

#### Teleporting a client-authoritive entity on the server

If you want to "teleport" a net entity on the server, while the client has authority, queue the `TeleportNetEntity` command.
If you want to frequently move a net entity on the server, you should instead give the server authority, e.g. by inserting the `Authority` component.
This will change the position on all connected peers.

Usage:
```rust
fn move_once(mut commands: Commands) {
    commands.queue(TeleportNetEntity {
        net_entity_id,
        position
    });
}
```


### Network messages

You will most likely want to send your own defined messages across clients / servers.

1. First, you need to register the message so netvy knows about it:

For example, in your protocol plugin:
```rust
app.register_network_message::<DemoMessage>(MessageDirection::ServerToClients);
```
Note that there are several message directions available.

2. Now, in order to read and write network messages, you just use bevy's `MessageReader` and `MessageWriter`, respectively and wrap the network message in netvys message wrappers:
    - `MessageReader<FromClient<DemoMessage>>`: Read a network message from a client on the server and know from which client this network message came from
    - `MessageReader<FromServer<DemoMessage>>`: Read a network message from the server on the client
    - `MessageWriter<ToClients<DemoMessage>>`: Write a network message from the server to clients. Here, you can choose from two different `NetworkMessageTarget`'s:
      - `NetworkMessageTarget::All`: Sends the network message to all currently connected clients
      - `NetworkMessageTarget::ToClients(Vec<PeerId>)`: Sends the network message to all clients with the specified peer ids. You can also use this to send a network message to a single client only, by just specifying a single peer id.

Read a network message on the server and determine from which client this came from:
```rust
fn read_demo_message(mut message_reader: MessageReader<FromClient<DemoMessage>>) {
    for message in message_reader.read() {
        info!("Received message {:?} from client: {:?}", message.message, message.source_client);
    }
}
```

Write a network message from the server to all connected clients:
```rust
fn send_demo_message(mut message_writer: MessageWriter<ToClients<DemoMessage>>) {
    message_writer.write(ToClients {
        message: DemoMessage("Hello from server".to_string()),
        target: NetworkMessageTarget::All,
    });
}
```

### Disconnecting from server

To do a clean disconnect on a client from the server, trigger the `Disconnect` event:

```rust
fn disconnect_system(mut commands: Commands) {
    commands.trigger(Disconnect);
}
```

All net entities belonging to this client will be despawned on all clients / the server.

To be notified whenever a client disconnects, read the `ClientDisconnected` message:

```rust
fn handle_client_disconnected(mut message_reader: MessageReader<ClientDisconnected>) {
    for message in message_reader.read() {
        info!(?client = message.client, "A client disconnected!");
    } 
}
```

Netvy will automatically despawn clients and their entities that timed out (e.g. they didnt respond anymore within the configured time).
You can change this time with the `NetvyConfiguration` resource, using the `timeout_client_seconds` field.

In the best case scenario, a client triggered the `Disconnect` event. This has the advantage of a faster despawning of corresponding entities.

## Bevy versioning table

| bevy   | netvy         |
|--------|---------------|
| 0.19   | 0.3.0         |
| 0.18.x | 0.1.0 - 0.2.1 |

## To-do

- [x] Be able to send a network message to specific clients only
- [x] Host-Client (server and client at the same time)
- [ ] Allow configuring whether components/network messages should be sent unreliable or reliable
- Performance improvements
  - [ ] Only retry and store latest failed (sent & apply) component update of a component/entity pair
