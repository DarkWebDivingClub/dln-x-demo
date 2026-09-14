# dln-x-demo

A demonstration of paying across two chains, where only the middle node
knows it happened.

```
   Alice ────── XBT channel ──────> Bob ────── BTC channel ──────> Clair
  holds XBT                   bridges both                    wants BTC
```

Clair writes an ordinary BTC invoice. Alice pays Bob in XBT against the
same payment hash; Bob pays Clair. Two ordinary Lightning payments
sharing one hash, and Alice's money can only move if Clair was paid.

**Clair runs stock software and never learns that XBT was involved.**

## Read this first

| | |
|---|---|
| [`doc/scenario-01.md`](doc/scenario-01.md) | **Alice buys BTC from Bob.** No third party — Alice is her own destination. The simplest thing that exercises every mechanism, and what to build first |
| [`doc/scenario-02.md`](doc/scenario-02.md) | **Alice pays Clair.** The same trade with a real destination, who implements nothing and never learns a second asset was involved |
| [`doc/messages-01.md`](doc/messages-01.md) | Every message scenario 01 sends, on both planes — the trade over NIP-XZ and each party driving their own nodes over NWC/NNC. Three trade messages, eleven control exchanges, and the gaps found |

The mechanics are identical. What changes between them is whose node
issues the ordinary invoice — and that is the difference between a swap
and a payment.

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
