# dln-x-demo

A demonstration of paying across two chains through a single Lightning
route, where only the middle node knows it happened.

```
   Alice ────── XBT channel ──────> Bob ────── BTC channel ──────> Clair
  holds XBT                   bridges both                    wants BTC
```

Clair writes an ordinary BTC invoice. Alice pays it with XBT. Bob sits in
the middle holding channels on both chains and converts mid-route. One
payment hash, one secret, ordinary Lightning atomicity.

**Clair runs stock software and never learns that XBT was involved.**

## Read this first

[`doc/scenario.md`](doc/scenario.md) — the scenario in full: the flow,
why it is atomic, why it is better than a bilateral swap, and what has to
be built to make it work.

## Status

Specification and demonstration. Nothing here is a product.

The atomicity is free — it is the same hash-lock every multi-hop
Lightning payment already uses. The work is in three other places:

| | Where |
|---|---|
| Forwarding across a denomination boundary | `dln-node` — the hard part |
| Quoting a rate before the payment is built | this repo, and a NIP |
| Pathfinding over a cross-asset hop | Alice's side, largely unexplored |

Of those, Nostr carries one, and it is the smallest. A specification that
presents this as a messaging protocol has described the wrong half.

## Related

| | |
|---|---|
| [`dln-node`](https://github.com/DarkWebDivingClub/dln-node) | the Lightning node this is built against |
| `nips/XZ.md` | the exchange protocol over Nostr |
| `diamond-x-e2e-test` | where the bilateral swap is already proved end to end |
