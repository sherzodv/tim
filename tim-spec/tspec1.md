## Entities

Tim Space - Space
Tim Node - Node
Tim Agent - Agent

Timite is an actor: human user, ai agent or similar essense capable of sending/recieving messages and performing any actions.
Timites interact in a single Space.
Internally a space is a local folder and a tim-code instance which operates within this folder.
Node is a tim space capable to interact with other spaces via tim-cloud instance.
Timites send messages to a space and subscribe to space updates.
Agent is a tim-code internal timite and works inside a tim-code instance. Agents can appear dynamically and there can be many.

Example timite:
  tim-web-ui on behalf of a user connects to a tim-code instance.
  subs to the space updates of the node
  sends messages to the space

examples of agents (timites):
  open ai chatgpt agent:
    connects to grpc endpoint of a node
    subs to the space updates of the node
  dummy echo agent:
    connects to grpc endpoint of a node
    subs to the space updates of the node

## Architecture notes

For the purpose of this timspec, everything happens realtime, no persistency and state replay is needed.

Space has updates queue. If timite1 sends a message to a space, all the timites of that same space, including timite1 will get an update SpaceNewMessage. If timite is human used web ui it will show it's own message in the space after recieving an update. When timite is a gpt agent it should ignore updates with it's own messages.

UpdatesDispatcher should dispatch updates to space subscribers in a separate thread.

Connection/transport layer should be completely isolated.

## Sessions

Timite sends Timite & ClientInfo on Authenticate. tim-code backend responses back with Session. Client sends session id in each subsequent request withing grpc metadata.