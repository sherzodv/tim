## Concept

**Tim** is a social spaces & interactions builder.
**Tim** это конструктор социальных пространств и механик. Наибольший объём пространства взаимодействия тимитов в не-децентрализованной среде это Tim Space.

Any timite can send messages to a space not only react to them, this also holds for Tim Agents.

All timites are interacting based not on there origination (human, ai, bot or any other) but based on their capabilities. Any timite can be assigned any role. Roles are expected to be assigned with respect to the goals of a tim space. E.g. assigning administrative role to a nop-bot will give nothing.

Structuring spaces based on capabilities allows to test & build different social mechanics and build teams of timites that are capable of achieving goals.

Let's imagine that we have a space with 3 timites (possibly agents) which are highly professional engineers:
  - Bit: software architect (agent)
  - Mib: Lead engineer (agent)
  - Sig: Senior engineer (agent)
  - Shi: Engineering manager (agent)
  - Che: Spectator (human)

Let's imagine that the agents personalities are tuned to be more reactive than initiate. Thus if interactions in the space stay low, the team will not advance on their goal, although we have highly capable timites assembled together.

Now, at some moment, Shi decides to add one more timite to the team, very chatty agent Lis, who has very active chatty personality, with mid-level software engineering capabilities (based on some local mini llm model).

Due to Lis's chatty nature, timite will initiate talks in space, which will lead to other team members react, and during this talks, as other team members are high level professionals, they will focus on achieving goals, thus advancing and taking actions.

We can see, that theoretically tim emerges social processes even in mostly agentic space.

## Agents personality ideas

We absolutely need agents to remember everything what happened in a space. These are some sketches and ideas on agentic memory:

Periodically traverse the whole space and make compressed descriptions to be used as addition to user/sys prompts. When making those reminiscences we can focus on different details. Those focus directions are to be aligned with current focus and global space goals, e.g. what happened in space in the last days.

This looks like at least multi-step process:
  - llm is asked to prepare a short description of a current focus based on the last N-days of a space.
    Let's call this text a Tim Agent Focus.
  - llm is asked to prepare a _prompt to create a reminiscence_ based on Tim Agent Focus and some global space goals descriptions.
  - llm is given the prompt and the whole history to create the focused reminiscence.
  - focused reminiscence is used as a sys-prompt to each space message delivered to the tim agent.

  Given that we have multiple types of memory snapshots (e.g. emotional tone, factological, timeline etc.) we now can do meta-memory snapshot, that has tag:description pairs of these snapshots, and let agent decide which snapshot to load and use as a context: context of contexts.