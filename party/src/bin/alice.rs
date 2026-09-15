//! Alice: the taker.
//!
//! Finds an offer, asks for a quote against an invoice **her own node**
//! made, pays the quote, and is done. She never settles anything.
//!
//! **She reaches Bob only over NIP-XZ and her own two nodes only over
//! NWC.** No client here can touch anything of his.
//!
//! What she holds: **XBT only.** She pays Bob in XBT and is paid in BTC
//! through inbound capacity his channel supplies.
//!
//! ## The invoice she issues is ordinary, and that is the point
//!
//! `make_invoice`, not `make_hold_invoice`. Her node generates the
//! preimage, keeps it, and settles the moment Bob pays — so the secret
//! travels back to him with his own payment and **this process never sees
//! it**. A hold invoice here would give her a free option: wait until
//! both legs are locked, then settle or walk away as the price suited
//! her, with Bob's funds tied up either way.
//!
//! She cannot hesitate, because nothing in her node is capable of
//! hesitating. That is a property of the construction rather than of her
//! good behaviour, which is why the demo is built this way.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use nostr::key::Keys;
use nostr_ln::nwc::client::{WalletConnect, WalletUri};
use nostr_ln::nwc::methods::*;
use tracing::info;
use xz::{offer_address, Rfq, Trade, TradePlane};

#[tokio::main]
async fn main() -> Result<()> {
    party::init_tracing();
    let cfg = party::Config::from_env("ALICE")?;

    // Two nodes, both hers.
    let btc = WalletConnect::new(
        cfg.btc_uri.parse::<WalletUri>().map_err(|e| anyhow::anyhow!("ALICE_BTC_URI: {e}"))?,
    );
    // Paying a hold invoice does not return until Bob settles, so this
    // call needs room. It is not a slow relay; it is the protocol.
    let xbt = WalletConnect::new(
        cfg.xbt_uri.parse::<WalletUri>().map_err(|e| anyhow::anyhow!("ALICE_XBT_URI: {e}"))?,
    )
    .with_timeout(Duration::from_secs(300));

    let keys = Keys::parse(&cfg.nostr_secret).context("ALICE_NOSTR_SECRET")?;
    let plane = TradePlane::connect(keys, &cfg.relay).await?;
    info!("alice is {}", plane.public_key().to_hex());

    // ── 1. Find an offer ─────────────────────────────────────────────
    let (maker, d, offer) = plane
        .find_offer("btc:xbt", Duration::from_secs(60))
        .await?
        .context("alice: nobody is offering btc:xbt")?;
    let addr = offer_address(&maker, &d);
    info!("offer from {}: {} ppm, {} msat", maker.to_hex(), offer.price, offer.volume);

    if cfg.amount_msat < offer.min || cfg.amount_msat > offer.volume {
        bail!("alice: {} msat is outside the offer", cfg.amount_msat);
    }
    let expect_to_pay = offer.counter_amount(cfg.amount_msat);
    info!("{} msat of BTC should cost {expect_to_pay} msat of XBT", cfg.amount_msat);

    // ── 2. An ordinary invoice, from her own node ────────────────────
    //
    // The preimage stays inside the node. This process never has it and
    // therefore cannot withhold it.
    let destination = btc
        .make_invoice(MakeInvoiceRequest {
            amount: cfg.amount_msat,
            description: Some("btc for xbt".into()),
            description_hash: None,
            expiry: Some(600),
            metadata: None,
        })
        .await
        .context("alice: make_invoice")?;
    let destination_invoice = destination.invoice.context("alice: invoice has no bolt11")?;

    // ── 3. Ask for a price ───────────────────────────────────────────
    let rfq_id = plane
        .send(&maker, &addr, None, &Trade::Rfq(Rfq {
            destination_invoice: destination_invoice.clone(),
        }))
        .await?;
    let _ = rfq_id;
    info!("rfq sent");

    // ── 4. Read the quote ────────────────────────────────────────────
    let (from, _id, trade) =
        plane.recv(Duration::from_secs(120)).await?.context("alice: no answer to the rfq")?;
    if from != maker {
        bail!("alice: answer came from somebody who did not make the offer");
    }
    let quote = match trade {
        Trade::Quote(q) => q,
        Trade::Abort(a) => bail!("alice: bob aborted — {}", a.reason),
        Trade::Rfq(_) => bail!("alice: bob sent an rfq"),
    };

    // The invoice **is** the quotation, so what she checks is the
    // invoice, not a restatement of it.
    let quoted = party::invoice_amount_msat(&quote.counter_invoice)
        .context("alice: the counter invoice states no amount")?;
    let counter_hash = party::invoice_payment_hash(&quote.counter_invoice)?;
    let mine = party::invoice_payment_hash(&destination_invoice)?;

    if counter_hash != mine {
        bail!("alice: the counter invoice locks to a different hash — refusing");
    }
    if quoted > expect_to_pay {
        bail!("alice: quoted {quoted} msat, the offer said {expect_to_pay}");
    }
    if quote.expiry <= party::now() {
        bail!("alice: the quote had already expired");
    }
    info!("quote is {quoted} msat, on her own payment hash, and unexpired");

    // ── 5. Paying is accepting ───────────────────────────────────────
    //
    // This does not return until Bob settles, which he can only do once
    // he has paid her — so a return here means the trade completed.
    let paid = xbt
        .pay_invoice(PayInvoiceRequest {
            invoice: quote.counter_invoice,
            amount: None,
            metadata: None,
        })
        .await
        .context("alice: pay_invoice — bob may not have settled")?;
    info!("paid, and bob settled: the trade is done");

    // She learns the preimage only because her own payment completed. It
    // was never hers to reveal.
    let _ = paid.preimage;
    Ok(())
}
