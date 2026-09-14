# Scenario 01: Alice buys BTC from Bob

The simplest thing that exercises every mechanism.

Alice holds XBT and wants BTC. Bob holds BTC and will sell it. There is no
third party: Alice is both the payer and the party being paid.

```
   Alice ────── XBT channel ──────> Bob
  (XBT node)                     (XBT node)

   Alice <───── BTC channel ─────── Bob
  (BTC node)                     (BTC node)
```

Four nodes, two channels. Alice and Bob each run one node per chain,
because **a Lightning node exists on exactly one network** and cannot hold
channels on two.

---

## Before anything happens

Both channels are open and funded on the side that will pay:

| Channel | Funded by | Why |
|---|---|---|
| Alice → Bob, XBT | **Alice** | she is paying XBT |
| Bob → Alice, BTC | **Bob** | he is paying BTC |

Opening a channel is an on-chain transaction taking minutes to an hour.
Nothing in a priced negotiation should wait on a confirmation, so this is
setup, not part of the trade. The trade itself is entirely off-chain and
takes seconds.

Note what neither party needs: **Alice needs no BTC and Bob needs no
XBT.** Each funds the channel they pay over; each is *paid* through
inbound capacity the other created. Their capital is one-sided.

---

## The flow

**1. Bob publishes an offer.** The pair, an indicative price, the volume
he can serve. Public, standing, binding on nobody.

**2. Alice's BTC node issues an ordinary invoice** for the BTC she wants,
carrying payment hash `H = SHA256(S)`. **She generated `S`.**

**3. Alice sends Bob a request for quotation** carrying that invoice.

**4. Bob quotes.** He can see the amount and the route to Alice's BTC
node, so he can price the trade. He replies with a **hold invoice** on the
XBT side for the amount he wants, carrying **the same `H`**.

**5. Alice pays Bob's hold invoice. Paying is accepting.** It holds — Bob
cannot settle it, because settling needs `S` and he does not have it.

**6. Bob pays Alice's ordinary invoice.** Her node settles it
**automatically**, because she generated the preimage — and `S` travels
back to Bob as his payment completes.

**7. Bob settles Alice's held payment with `S`.**

Alice has swapped XBT for BTC. Bob has swapped BTC for XBT.

---

## Why only one hold invoice

This is the part worth understanding, because the obvious design has two.

> **A hold invoice is needed on every leg whose payee is not free to
> settle immediately.**

| Leg | Payee | Has `S`? | Free to settle? | Invoice |
|---|---|---|---|---|
| Alice → Bob (XBT) | Bob | no | **cannot** | **hold** |
| Bob → Alice (BTC) | Alice | **yes** | **yes** | **ordinary** |

Bob's leg must hold because he has no preimage: an ordinary invoice
settles automatically *because its payee generated the secret*, and Bob
did not. His node has `H` and nothing else.

Alice's leg is ordinary because she *did* generate it, and her node will
release it the moment it is paid.

### The ordinary invoice is what removes Alice's option

Alice knows `S`. In the usual two-hold construction, that makes her the
party who decides whether to complete *after* seeing both legs locked —
she settles if the price still suits her and walks away if it does not,
with Bob's funds locked until timeout either way. A free option, written
by Bob for nothing.

**An ordinary invoice takes that choice away.** A hold invoice is
precisely the ability to decide *when*; without one, her node settles on
receipt whether it still suits her or not. She cannot hesitate, because
nothing in her node is capable of hesitating.

So the asymmetry is not an optimisation. It is what makes the trade fair.

### What Bob cannot verify

He cannot tell in advance that Alice's invoice really is ordinary. She
could issue a hold invoice for `H` and stall.

That is **griefing, not theft**: his payment to her sits pending, he never
claims her XBT, and both expire. Her own XBT is locked for the same
period, so it costs her as much as him. Unprofitable and recoverable —
worth knowing about, not worth defending against here.

---

## Why it is atomic

One hash, two invoices, and Alice's funds held against a preimage that
does not exist in the open.

> **Alice's XBT can only move if Bob has `S`, and Bob can only have `S` if
> he paid her BTC.**

| If | Then |
|---|---|
| Bob never quotes | Nothing has moved |
| Alice never pays | Nothing has moved |
| Bob never pays Alice | Her payment expires and refunds. **No loss; liquidity locked until timeout** |
| Bob pays but cannot claim | **Bob loses.** Prevented by the timelock |

### Alice pays first. Always.

If Bob paid first, Alice's node would settle on receipt and reveal `S` —
but Bob would have handed over BTC with nothing of Alice's pending, and no
step compelling her to pay. `S` is worth nothing to a maker with no held
HTLC to claim.

### The timelock

Bob's exposure is the window between paying Alice's BTC invoice and
settling her XBT payment. He does not hope it is long enough — **he sets
it**, through the final CLTV in the invoice he issues:

1. Alice's invoice arrived in the request for quotation.
2. So Bob can route to her BTC node and total the CLTV delta **before
   issuing anything**.
3. He sets his own invoice's final CLTV above that, plus a margin.

Whatever route Alice takes only *adds* delta on top of that floor. **Alice
is safe regardless** — her funds cannot move unless Bob holds `S`. Bob is
the one who loses by getting this wrong.

---

## What this demonstrates, and what it does not

**Demonstrates** — every mechanism the full design needs:

- the three-message negotiation: offer, RFQ, quotation
- a hold invoice issued on one chain against a hash generated on another
- settling on a preimage **observed**, never on a local copy
- the payment ordering, and the timelock that protects the maker
- two nodes per party, one per chain

**Does not demonstrate** the property the full design turns on: that **the
destination need not participate at all.** Here Alice is the destination,
so nothing proves a stranger could be paid without implementing anything.
That is [scenario 02](scenario-02.md), and the only thing that changes is
whose node issues the ordinary invoice.

---

## Open questions

1. **The harness will get the invoices wrong.** `swap_on_signets` uses
   **two** hold invoices, correct for the bilateral swap it tests. Reusing
   its helper here would make Alice's leg a hold invoice too — which
   still passes, and quietly tests a protocol with a free option in it.

2. **Quoting needs a route before it can price.** Bob must find his path
   to Alice's BTC node at quotation time. This is the first thing here
   that is real work rather than message passing.

3. **Bob's position depletes.** Every trade moves BTC out and accumulates
   XBT. One direction only, and restoring it needs flow the other way or
   a trade made elsewhere.
