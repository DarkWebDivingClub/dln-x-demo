# dln-x-demo

A demonstration of paying across two chains through a single Lightning
route, where only the middle node knows it happened.

```
   Alice ────── XBT channel ──────> Bob ────── BTC channel ──────> Clair
  holds XBT                   bridges both                    wants BTC
```

Clair writes an ordinary BTC invoice. Alice pays Bob in XBT against the
same payment hash; Bob pays Clair. Two ordinary Lightning payments
sharing one hash, and Alice's money can only move if Clair was paid.

**Clair runs stock software and never learns that XBT was involved.**

## Read this first

[`doc/scenario.md`](doc/scenario.md) — the scenario in full: the flow,
why it is atomic, why it is better than a bilateral swap, and what has to
be built to make it work.

## Status

Specification and demonstration. Nothing here is a product.

The atomicity is free, and **nothing forwards across a denomination
boundary** — both legs are ordinary payments within their own chain, so
no node's forwarding logic changes. The work is:

| | Where |
|---|---|
| Three messages — offer, RFQ, quotation | `NIP-XZ`, and a client |
| Issuing a hold invoice and settling it on an observed preimage | `dln-node` |
| Quoting: route to the destination, price it, set the final CLTV | Bob's side — the only interesting logic |

## Related

| | |
|---|---|
| [`dln-node`](https://github.com/DarkWebDivingClub/dln-node) | the Lightning node this is built against |
| `nips/XZ.md` | the exchange protocol over Nostr |
| `diamond-x-e2e-test` | where the bilateral swap is already proved end to end |
