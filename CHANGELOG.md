## [unreleased]

### Documentation

- Update bevy versioning table

### Miscellaneous Tasks

- Init git-cliff
## [0.5.0] - 2026-09-20

### Features

- Add ClientDisconnectedServer message to be read on server
- Teleport net entities from the server on client-authoritive entities
- Sync rotation (#14)

### Bug Fixes

- Duplicate component updates (#10)
- Duplicate net entities in netvy mode host client (#12)
- Disconnected clients still kept in ConnectedClients resource (#13)
- Properly cleanup all resources of netvy on disconnect
- Sync position not working properly
- Sync position integration test (#15)

### Documentation

- Update readme

### Miscellaneous Tasks

- Update nix flake (#11)
- Release v0.5.0
## [0.4.0] - 2026-08-20

### Features

- Store latest component updates of each possible pair and send it to new clients (#6)
- Add a few very basics integration tests and fix a couple of bugs (#5)
- Improve authority management, allow server to always make changes to net entities
- Disconnect (#7)
- Release 0.4.0 and upgrade to bevy 0.19.1 (#9)

### Bug Fixes

- AnnounceNewClient message was never handled on client
- Remove accidentally kept spammy log
- Dont fail component update if update sequence not present in FailedComponentUpdates
- Incoming component updates were never applied on the server, only forwarded to other clients
- Client-side SyncPosition entities never updated on server
- Sync position (#8)

### Miscellaneous Tasks

- Adjust lint rules to better fit bevy
## [0.2.0] - 2026-07-13

### Features

- Initial commit
- Send changed entity / component to server along with component type id
- Client / server architecture
- Client requests net entity from server and server confirms
- Notify new clients about any existing net entities
- First milestone reached! net entities are properly synced across all clients
- Sync mode which allows users to specify when component updates should be sent
- Update sequence number to ensure ordered component updates
- Add option to linear interpolate position synced entities
- Introduce clientconnectionstate
- Serde (#1)
- Network messages (#2)
- Introduce client component with clientid and change connect trigger to use this client entity
- Network message direction
- Network messages rework
- Add OwnedBy and Owned component
- Allow configuring the plugin via the `NetvyConfiguration` resource
- Host client mode (#3)
- Introduce Authority component to differentiate between authority and ownership
- Move to SocketAddr to support more connect cases

### Bug Fixes

- Switch from just deserialize fn to apply fn because we cant downcast if we just erased the type
- Architecture & wip on client requests net entity from server
- Client not spawning new entity for new net entities
- Duplicate entity spawned for new NetEntityId
- Component updates not applied correctly
- Re-sent component updates that failed
- Automatically insert EntityType::Local into relevant entities
- Only register internal types for debug builds
- Ensure update sequence always exists
- Drain the socket every tick instead of only receiving one packet each tick which causes buildup
- Dont bind to loopback
- Clamp sync position linear interpolation
- Dont retry failed sent component updates every tiick
- Errors should use error log
- Remove unused failed component updates timer
- Demo network message read
- Dont require network messages to derive from 'Message'
- Explicit panic when calling register_net_message before adding NetvyPlugin
- Panic if not yet connected
- Temporary net entities spawned on server
- Component updates on the server were not sent to clients
- Server-side spawned net-entities were not sent to clients
- Server spawned entities were sometimes not replicated to clients
- Net entities with Owned components on clients were still NetEntityType::Remote on these clients
- Panic when server gets created at a later time
- Panic when failing to bind socket
- SyncPosition incorrectly applied - authority issues

### Other

- Be able to register user defined components to be synced across clients
- Detect changes for registered components
- Component registry, mapping from rust type id to internal id, from internal id to deserialize fn
- Send & receive data on client <-> server
- Replicate registered component change to other connected clients
- Net entity mapping
- Notify all clients about new client & possible new entities
- Sync component updates to clients
- Retry failed component updates
- Sequence number for ordered updates
- Store sequence number per entity & component
- Authority

### Refactor

- Introduce DatagramType

### Documentation

- Add wip readme usage uide
- Add running client and server section, add table of contents
- Finish network messages section

### Miscellaneous Tasks

- Add vscode tasks
- Remove double section readme
- Remove no longer needed OurEntity component from demo
- Dont require Debug on registered components
- Update nix flake
