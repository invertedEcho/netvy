# netvy

## Goals
- As usual, common cases require minimal code and providing sane defaults, but advanced control is still an option
- Very straightforward API, easy to understand
- Documentation everywhere
- As few dependencies as possible
- Helpful examples with minimal code
- Netvy provides detailed logs, as debugging networking in games is already not that easy

> [!NOTE]
> This documentation will change during development and is heavily WIP.

## Table of Contents

1. [Getting started](#getting-started)
2. [Running a client and server](#running-a-client-and-server)
3. [Component registration](#component-registration)
    - [Sync modes](#sync-modes)
4. [Syncing position of entities](#syncing-position-of-entities)
5. [Network messages](#network-messages)

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

1. First, you need to register the message so netvy knows about it:

For example, in your protocol plugin:
```rust
app.register_net_message::<DemoMessage>(MessageDirection::ServerToClients);
```
Note that there are several message directions available.

2. Now, in order to read and write net messages, you need to query for the `NetMessageReader<DemoMessage>` and `NetMessageWriter<DemoMessage>`, respectively. These are inserted into each client/server by netvy automatically, after setting up the client/server.

For each network message you register, one 'instance' of component is added.

This way, you can decide from which client you want to send a message from (allowing for multi-client applications), and also directly know from which client this net message came from, by adding `ClientId` to your query.

Reading a message:
```rust
fn read_demo_message(mut message_reader: MessageReader<DemoMessage>) {
    for message in message_reader.read() {
        info!("Received message from server: {:?}", message);
    }
}
```

Writing a message:
```rust
fn send_demo_message(mut message_writer: MessageWriter<DemoMessage>) {
    message_writer.write(DemoMessage("Hello from server!".to_string()));
}
```

## To-do

- [ ] Be able to send a network message to specific clients only
- [x] Host-Client (server and client at the same time)
- [ ] Allow configuring whether components/network messages should be sent unreliable or reliable
- Performance improvements
  - [ ] Only retry and store latest failed (sent & apply) component update of a component/entity pair
