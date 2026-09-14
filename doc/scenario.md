# The scenario: paying across chains without anyone noticing

Alice holds XBT. Clair wants BTC. They have never met, share no channel,
and neither runs anything unusual.

Bob does.

```
   Alice ────── XBT channel ──────> Bob ────── BTC channel ──────> Clair
  holds XBT                   bridges both                    wants BTC
```

Bob has a channel to Alice denominated in XBT and a channel to Clair
denominated in BTC. **He is the exchange, and the exchange happens as a
routing hop.**

---

## The flow

1. **Clair writes an ordinary BTC invoice.** She generates a secret `S`,
   computes `H = SHA256(S)`, and asks for *X* msat. This is a completely
   normal Lightning invoice. She is running stock software and has no
   idea anything unusual is about to happen.

2. **Alice gets the invoice** and wants to pay it, but holds no BTC.

3. **Alice needs a rate.** How much XBT buys *X* BTC? Bob quotes it —
   rate *R*, valid until some time, up to some capacity.

4. **Alice offers an HTLC to Bob on the XBT channel**: amount
   *Y = X × R*, locked to `H`, expiring at `T_alice`.

5. **Bob offers an HTLC to Clair on the BTC channel**: amount *X*, locked
   to **the same `H`**, expiring at `T_clair`, where
   `T_clair < T_alice − Δ`.

6. **Clair settles**, revealing `S` and taking her *X* BTC. From her side
   the payment simply arrived.

7. **Bob learns `S`** — it comes back down his outgoing HTLC, exactly as
   it would on any forwarded payment.

8. **Bob settles Alice's HTLC** with `S`, taking *Y* XBT.

Alice paid XBT. Clair received BTC. The two assets never met except
inside Bob.

---

## Why it is atomic

There is **one secret and one hash** for the whole path. That is the
entire trick, and it is not new — it is how every multi-hop Lightning
payment already works. The only unusual thing is that the two hops are
denominated in different assets.

| If | Then |
|---|---|
| Clair never settles | Both HTLCs expire. Everyone refunded |
| Bob never forwards | Alice's HTLC expires. Alice refunded, Clair never knew |
| Bob forwards but cannot claim | Impossible while `T_alice > T_clair + Δ` — he learns `S` the moment Clair takes his money |
| The relay carrying the rate quote dies | Irrelevant. Settlement is on Lightning |

Bob's safety is the ordinary forwarding condition: **his incoming HTLC
must outlive his outgoing one.** That is `cltv_expiry_delta`, which every
routing node already enforces. Nothing new is required to make this safe.

---

## Why this is better than a bilateral swap

The obvious alternative is for Alice and Bob to swap directly: two
invoices, two payments, both locked to one hash, each party settling
their own leg. That construction works, and it is what
[NIP-XZ](../../nips/XZ.md) originally described. This is better in three
ways.

**Nobody holds a free option.** In a bilateral swap the party who knows
the secret decides whether to settle *after* seeing both legs held. They
settle if the trade still suits them and walk away if it does not, and
the counterparty's funds are locked until timeout either way. Here the
secret holder is Clair — **the party being paid** — and she has every
reason to settle. The option evaporates because the secret is held by
someone with nothing to gain from stalling.

**No special timelock rule.** A bilateral swap needs the exposed party's
incoming leg to outlive their outgoing leg by a margin, and getting that
backwards loses money. Here it is just `cltv_expiry_delta` at Bob's hop —
the same discipline, already implemented, already understood.

**Clair is unmodified.** She writes an invoice and gets paid. She does
not implement a swap protocol, does not negotiate, does not need to know
XBT exists. A protocol that requires both ends to adopt it is a protocol
that needs a network before it is useful; this one needs only Bob.

---

## What has to be built

The atomicity is free. The work is everywhere else.

### 1. Bob's node must forward across a denomination boundary

This is the hard part, and it is not a Nostr problem.

In BOLT forwarding, the onion tells Bob `amt_to_forward`, and Bob checks
that his incoming HTLC covers it plus his fee. With XBT coming in and BTC
going out, **incoming and outgoing amounts are in different units.** A
stock LND or LDK node compares them, finds the forward underpaid, and
fails the HTLC.

So Bob's node must:

- know that its two channels are denominated differently
- apply a **rate** rather than a proportional fee
- decide whether an incoming amount is sufficient *after conversion*

That is a change at the HTLC layer of the node, which is why this demo
lives against `dln-node` rather than stock implementations.

### 2. Alice must be able to learn the rate before she pays

She constructs the payment, so she needs the number first. A rate quote
needs:

| Field | Why |
|---|---|
| pair | which two assets |
| rate | how much of one buys the other |
| capacity | above which Bob will not quote |
| expiry | after which the quote is void |

Bob carries the price risk between quoting and settling. A short expiry
protects him; too short and payments fail in flight.

### 3. Somewhere to publish the quote

This is the part Nostr is for, and the only part of the scenario that
touches it. It is also the smallest part.

> **Note the proportions.** Of the three things to build, Nostr carries
> one, and it is the easy one. The scenario is mostly a Lightning node
> change, and any specification that presents it as a messaging protocol
> has described the wrong half.

---

## What each party knows

Worth stating, because it is the measure of how good the construction is.

| | Knows about the other chain | Runs special software |
|---|---|---|
| **Clair** | nothing | no |
| **Alice** | that Bob quotes a rate | pathfinding must accept a cross-asset hop |
| **Bob** | everything | yes — this is the whole job |

Bob is the only party carrying complexity, and he is the party being paid
to carry it. That is the correct place for it to sit.

---

## What this is not

**It is not a swap between Alice and Bob**, even though Bob ends up with
XBT and less BTC. There is no negotiated trade, no acceptance, no pair of
invoices. There is one payment that changes denomination in transit.

**It is not trustless price discovery.** Alice takes Bob's quote or does
not pay. Whether that quote is fair is a market question, and the market
does not exist yet.

**It is not a new atomicity mechanism.** Everything that makes it safe
was already in Lightning. What is new is one node being willing to hold
two kinds of money and quote between them.

---

## Prior art

Lightning Labs' Taproot Assets does something structurally similar —
assets converted to BTC at the edges of a route, with the BTC core of the
network unaware. The differences here are that both legs are their own
chain rather than an asset issued on one, and that the bridging node is
the explicit subject rather than an implementation detail.

Submarine swaps solve an adjacent problem — moving between on-chain and
off-chain — with the same hash-lock trick and the same asymmetry about
who learns the secret first.

Neither of those makes this novel. It is worth knowing they exist, and
worth knowing why this is not simply either of them.

---

## Open questions

1. **Does Alice's pathfinding cope?** She must construct a route whose
   first hop is denominated differently from the destination amount. The
   onion has to carry enough for Bob to do the conversion, and Alice has
   to be able to reason about a path whose hops are not commensurable.
   This is the piece most likely to be harder than it looks.

2. **What happens to the quote when the payment is slow?** Bob honours
   a rate for the life of the HTLC, which may be minutes. That is an
   option written by Bob, priced into the spread — the same free-option
   problem as the bilateral swap, moved to where someone is being paid
   to bear it.

3. **Multiple bridging nodes.** One Bob is a demo. Several Bobs quoting
   different rates is a market, and needs discovery, comparison and
   failure handling that none of this describes.
