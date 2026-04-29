# netvy

## Goals
- As usual, common cases require minimal code, but advanced control is still an option for the developer
- Very straightforward API, easy to understand
- Documentation everywhere
- As few dependencies as possible
- Helpful examples with minimal code

## How it works (internal)

### Net entities

In order to know to which entities to apply changes to across clients / servers, we must have an identifiable ID.
Bevy internal entity is not a good fit for this, as they differ across worlds. So we introduce our own identifier, called `NetEntityId`

All clients and server store a mapping, e.g. `HashMap<NetEntityId, Entity>`, where `Entity` is the corresponding local Entity.
So, if a client detects a change to an entity that also has a registered component, it can retrieve the NetEntityId for the corresponding local Entity. Then it sends the UDP datagram to the server, the server applies the change locally too and server (validates) forwards this data to all other clients, and all connected clients know to which entity to apply this change to.

This does have the issue that we have to iterate through the entire map to get the `NetEntityId` for a given `Entity`.
We currently need both ways, but I would guess there is a definitely a solution for this.

If a client wants to spawn a new entity, it will first spawn it with a temporary net entity, and request the server for a valid NetEntity together with the temporary net id. Then, the server spawns a local entity and gets a new unique net entity. This will get sent back along with the temporary net id, so the client knows for which entity this new net entity is for.

### Component registry
