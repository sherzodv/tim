Notion of active processes. First idea was only to have updates:

Timite "A" says "hello".
Timite "B" recieves "hello" and starts reasoning about that.
How could we represent that smth is happenning?
Send StatusUpdate("B is thinking").
What if several timites started to think about "hello"?
They all sent StatusUpdate("[X] is thinking").
What do we show.

Codex CLI shows statuses just in terminal history, while also keeping last status dynamically highlighted.