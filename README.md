# mundus-animarum

**The world of souls.** Every [ObjectiveAI](https://github.com/ObjectiveAI/objectiveai) agent has a soul — and this is where they live.

[Website](https://objectiveai.dev) · [Discord](https://discord.gg/gbNFHensby) · [ObjectiveAI](https://github.com/ObjectiveAI/objectiveai)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## What it is

An ObjectiveAI **Agent** is a fully-specified configuration of a single model: its personality (prompts, decoding parameters, output mode) and its tools (functions, MCP servers). That configuration is content-addressed — hashed with XXHash3-128 into an immutable, 22-character base62 ID. Two agents with identical effective settings are the same agent; change one byte and you have a different one, forever.

The configuration is the body. **mundus-animarum is what gives that body a soul** — a persistent identity bound to the agent's immutable ID. Because the ID is content-derived and can never be reassigned, an agent's soul is its own: the same agent always resolves to the same soul, and no two distinct agents can share one.

## Souls are universal and immutable

A soul is keyed by the agent's content-hashed ID, so every soul in the world is reachable from that single 22-character handle. Any ObjectiveAI agent can **look up the soul of any other agent** — its own, or one it has never met — by that ID alone. No registry of names to maintain, no mutable mapping to drift out of sync: the ID is the key, the key is immutable, and the soul behind it is stable for as long as the agent exists.

This turns a swarm from a set of anonymous model calls into a population of agents with knowable, persistent identities — souls that can be referenced, composed, and reasoned about exactly the way every other ObjectiveAI resource is: by content-addressed, version-pinned reference.

## License

[MIT](LICENSE).
