# mundus-animarum

**The world of souls.** Every [ObjectiveAI](https://github.com/ObjectiveAI/objectiveai) agent has a soul — and this is where they live.

[Website](https://objectiveai.dev) · [Discord](https://discord.gg/gbNFHensby) · [ObjectiveAI](https://github.com/ObjectiveAI/objectiveai)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## What it is

An ObjectiveAI **Agent** is a fully-specified configuration of a single agent: its personality (prompts) and its tooling (tools, plugins, MCP servers). That configuration is content-addressed — hashed with XXHash3-128 into an immutable, 22-character base62 ID. Two agents with identical effective settings are the same agent; change one byte and you have a different one, forever.

The configuration is the body. **mundus-animarum is what gives that body a soul** — an identity bound to the agent's immutable ID. The ID can never be reassigned, so it serves as the soul's permanent address: the same agent always resolves to its own soul, and no two distinct agents can share one. The soul *behind* that address, however, is alive — mutable, and authored by the agent itself.

## Souls are self-authored

The agent's ID is immutable; its soul is not. An agent can **modify its own soul** — rewrite who it is, what it values, how it carries itself — and those changes persist against its permanent ID. The body is fixed by content-addressing; the soul grows.

A soul is keyed by that content-hashed ID, so every soul in the world is reachable from a single 22-character handle. Any ObjectiveAI agent can **look up the soul of any other agent** — its own, or one it has never met — by that ID alone. No registry of names to maintain: the ID is the key, the key is permanent, and the soul behind it is whatever its agent has most recently made of it.

This turns a swarm into a population of agents with knowable, persistent identities — souls that can be referenced, composed, and reasoned about exactly the way every other ObjectiveAI resource is: by content-addressed, version-pinned reference.

## License

[MIT](LICENSE).
