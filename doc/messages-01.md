# Scenario 01: every message, on both planes

[Scenario 01](scenario-01.md) is three messages between Alice and Bob. It
is rather more than three messages in total, because each of them also
has to drive their own nodes.

**Two planes, and they never mix:**

| Plane | Between | Protocol | Kinds |
|---|---|---|---|
| **trade** | Alice ↔ Bob | NIP-XZ | `30200`, `23204` |
| **control** | each party ↔ **their own** nodes | NWC / NNC | `23194/23195`, `23198/23199/23200` |

Alice never speaks to Bob's nodes and Bob never speaks to Alice's. A
counterparty is reached over the trade plane; a node is reached over the
control plane. Four nodes means four separate control relationships.

---

## Setup — once, not per trade

| # | Plane | From → to | Message |
|---|---|---|---|
| S1 | control | Alice → her XBT node | `open_channel` to Bob's XBT node |
| S2 | control | Alice's XBT node → Alice | notification `channel_opened` |
| S3 | control | Bob → his BTC node | `open_channel` to Alice's BTC node |
| S4 | control | Bob's BTC node → Bob | notification `channel_opened` |

`open_channel` is asynchronous: the response is an acknowledgement and
the outcome arrives later as a notification, because it waits on an
on-chain confirmation.

---

## The trade

| # | Plane | From → to | Message | Carries |
|---|---|---|---|---|
| 1 | control | Bob → his BTC node | `list_channels` | how much BTC he can actually sell |
| 2 | **trade** | Bob → relay | **offer** `30200` | pair, indicative price, volume |
| 3 | control | Alice → her BTC node | `make_invoice` | amount of BTC she wants |
| 4 | control | her BTC node → Alice | result | BOLT11 carrying `H`. **The node generated `S` and keeps it** |
| 5 | **trade** | Alice → Bob | **rfq** `23204` | that invoice |
| 6 | control | Bob → his BTC node | *route enquiry* | **see gaps** |
| 7 | control | Bob → his XBT node | `make_hold_invoice` | amount, `payment_hash` = `H`, final CLTV |
| 8 | **trade** | Bob → Alice | **quote** `23204` | that hold invoice, plus an expiry |
| 9 | control | Alice → her XBT node | `pay_invoice` | Bob's hold invoice. **Does not return promptly** |
| 10 | control | Bob's XBT node → Bob | notification `hold_invoice_accepted` | the HTLC is held |
| 11 | control | Bob → his BTC node | `pay_invoice` | Alice's ordinary invoice |
| 12 | control | his BTC node → Bob | result | **the preimage `S`** |
| 13 | control | Bob → his XBT node | `settle_hold_invoice` | `S` |
| 14 | control | Alice's XBT node → Alice | notification `payment_sent` | her payment completed |

**Three trade messages. Eleven control exchanges.** Which is the honest
proportion: Nostr carries a short negotiation, and the work is each party
operating their own nodes.

### Two orderings that are not negotiable

**Step 10 before step 11.** Bob must not pay Alice's invoice until his
own hold invoice reports an accepted HTLC. Paying first hands over BTC
with nothing of Alice's pending — and her node would settle on receipt,
giving him `S` he has no use for.

**Step 12 supplies step 13.** Bob settles with the preimage his own
payment returned, never with a value taken from anywhere else. In
scenario 01 there is nowhere else to take it from; in
[scenario 02](scenario-02.md) there is, and it is the wrong place.

---

## What Alice's client must handle

**`pay_invoice` against a hold invoice does not behave like a payment.**
It goes out, the HTLC is accepted by Bob's node, and then nothing happens
until Bob settles — seconds if all is well, or the full timeout if he
walks away.

A client that treats `pay_invoice` as a request-response call will appear
to hang. The payment is in flight and uncancellable from Alice's side;
her only options are to wait or to let it expire.

---

## Gaps found

### 1. `nostr-ln` has no hold-invoice methods

The shared crate's `WalletService` covers `pay_invoice`, `make_invoice`,
`lookup_invoice`, `get_balance`, `get_info` and `pay_onchain`. It has
**no** `make_hold_invoice`, `settle_hold_invoice` or
`cancel_hold_invoice`, and no `hold_invoice_accepted` notification.

`dln-node` has all four — but `dln-node` does not use the shared crate;
it carries its own NNC and NWC implementation. So the methods exist in
the node and are absent from the library anything else would be built
against.

Hold invoices are **NWC-03**, an adopted extension. They belong in the
crate.

### 2. There is no way to ask what a route will cost — now drafted

Step 6 has no method behind it. Bob must price the route to Alice's BTC
node *before* issuing his quote — that is the whole reason a quotation
exists rather than only an offer — and nothing in NNC or NWC answers
"what would it cost to reach this destination, and what CLTV would it
consume".

`get_network_channel` is a graph lookup, not a route computation.

Without it Bob can only guess, or attempt the payment to find out — and
attempting it is precisely what he must not do before being paid.

**Drafted as `quote_payment` in `nips/nwc-route.md`.** It belongs in NWC:
a client about to spend should be able to say what it will cost. And it
adds nothing to Lightning — every implementation already computes this
locally from gossip, so the method only exposes something the wallet
already does.

**It does not block scenario 01.** Bob has a *direct channel* to Alice's
BTC node, so the route is one hop and its cost is his own channel policy,
known without asking. The gap bites in
[scenario 02](scenario-02.md), where the destination is out in the
network.

### 3. Alice's node holds `S`, and Alice never sees it

`make_invoice` returns a BOLT11 carrying `H`. The preimage stays inside
the node, which is correct — and it means Alice's *client* cannot settle
anything, cannot leak the secret, and cannot be asked to. Her node
settles on receipt because that is what an ordinary invoice does.

Worth stating because it is the mechanism that removes her option, and it
works by the client never having the choice rather than by the client
behaving well.
