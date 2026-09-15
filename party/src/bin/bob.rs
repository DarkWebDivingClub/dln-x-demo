//! Bob: the maker.
//!
//! Publishes an offer, quotes against a request, pays the destination and
//! settles what he is holding. He is the exchange.
//!
//! **He reaches Alice only over NIP-XZ and his own two nodes only over
//! NWC.** There is no third channel and no client for anything of hers —
//! which is the property `doc/messages-01.md` calls the two planes never
//! mixing, and the reason this is a separate process rather than a struct.
//!
//! What he holds: **BTC only.** He pays Clair — here, Alice — out of his
//! own outbound BTC, and he is *paid* in XBT through inbound capacity
//! Alice created by funding her channel to him. His capital is one-sided.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use nostr::key::Keys;
use nostr_ln::nwc::client::{WalletConnect, WalletUri};
use nostr_ln::nwc::methods::*;
use nostr_ln::nwc::WalletNotificationType;
use tracing::info;
use xz::{offer_address, Offer, Quote, Trade, TradePlane};

/// How long Bob will honour a quote. His own price risk, and short.
const QUOTE_VALID_SECS: u64 = 120;

/// Blocks of margin above what the route costs, so his claim outlives his
/// obligation. Scenario 01's timelock rule: he **sets** this rather than
/// checking it.
const CLTV_MARGIN: u32 = 40;

/// Below this a routing fee is most of the trade. Bob's floor, not the
/// protocol's.
const MIN_TRADE_MSAT: u64 = 1_000;

#[tokio::main]
async fn main() -> Result<()> {
    party::init_tracing();
    let cfg = party::Config::from_env("BOB")?;

    // Two nodes, both his. Nothing here can reach Alice's.
    let btc = WalletConnect::new(cfg.btc_uri.parse::<WalletUri>().map_err(|e| anyhow::anyhow!("BOB_BTC_URI: {e}"))?);
    let xbt = WalletConnect::new(
        cfg.xbt_uri.parse::<WalletUri>().map_err(|e| anyhow::anyhow!("BOB_XBT_URI: {e}"))?,
    )
    .with_timeout(Duration::from_secs(300));

    let keys = Keys::parse(&cfg.nostr_secret).context("BOB_NOSTR_SECRET")?;
    let plane = TradePlane::connect(keys, &cfg.relay).await?;
    info!("bob is {}", plane.public_key().to_hex());

    // ── 1. Publish the offer ─────────────────────────────────────────
    //
    // `volume` is his **Lightning** balance, not his total: the on-chain
    // part of a node's balance cannot pay an invoice, and offering it
    // would advertise capital he cannot deliver over this trade.
    //
    // It is still an upper bound rather than a promise — local balance is
    // not per-destination capacity, and a route to a particular payee may
    // be narrower. `quote_payment` is where a specific request meets a
    // specific route; this number only says what is worth asking about.
    let balance = btc.get_balance().await.context("bob: get_balance on BTC")?;
    let volume = balance
        .lightning_balance
        .context("bob: his BTC node did not report a lightning balance")?;
    let offer = Offer { price: cfg.price_ppm, volume, min: MIN_TRADE_MSAT };
    let d = "bob-btc-xbt";
    plane.publish_offer(d, "btc:xbt", &offer).await?;
    info!("offer published: {} ppm, {} msat available", offer.price, offer.volume);

    // ── 2. Wait for a request ────────────────────────────────────────
    let (taker, rfq_id, trade) = plane
        .recv(Duration::from_secs(120))
        .await?
        .context("bob: no rfq arrived")?;
    let Trade::Rfq(rfq) = trade else { bail!("bob: expected an rfq") };
    info!("rfq from {}", taker.to_hex());

    let destination_amount = party::invoice_amount_msat(&rfq.destination_invoice)
        .context("bob: the destination invoice carries no amount — XZ.md requires one")?;
    let payment_hash = party::invoice_payment_hash(&rfq.destination_invoice)?;

    if destination_amount < offer.min || destination_amount > offer.volume {
        let addr = offer_address(&plane.public_key(), d);
        plane
            .send(&taker, &addr, Some(rfq_id), &Trade::Abort(xz::Abort {
                reason: format!("{destination_amount} msat is outside {}..{}", offer.min, offer.volume),
            }))
            .await?;
        bail!("bob: the request is outside the offer");
    }

    // ── 3. Price the route before quoting ────────────────────────────
    //
    // He cannot discover this by attempting the payment, because
    // attempting it is what he must not do until he has been paid.
    let quote = btc
        .quote_payment(QuotePaymentRequest {
            invoice: rfq.destination_invoice.clone(),
            amount: None,
        })
        .await
        .context("bob: quote_payment")?;
    if !quote.route_found {
        let addr = offer_address(&plane.public_key(), d);
        plane
            .send(&taker, &addr, Some(rfq_id), &Trade::Abort(xz::Abort {
                reason: "no route to that destination".into(),
            }))
            .await?;
        bail!("bob: no route to the destination");
    }
    info!(
        "route costs {} msat and {} blocks",
        quote.fee_msat, quote.cltv_expiry_delta
    );

    // ── 4. Quote: a hold invoice at a firm price ─────────────────────
    //
    // Its amount is the price, its hash is the destination's, and its
    // final CLTV is above what the route will consume — so his claim on
    // Alice outlives his obligation to the destination.
    let counter_amount = offer.counter_amount(destination_amount);
    let hold = xbt
        .make_hold_invoice(MakeHoldInvoiceRequest {
            amount: counter_amount,
            payment_hash: payment_hash.clone(),
            description: Some("xbt for btc".into()),
            description_hash: None,
            expiry: Some(QUOTE_VALID_SECS),
            min_cltv_expiry_delta: Some(quote.cltv_expiry_delta + CLTV_MARGIN),
        })
        .await
        .context("bob: make_hold_invoice")?;
    let counter_invoice = hold.invoice.context("bob: hold invoice has no bolt11")?;

    // Subscribe *before* quoting. The stream is a broadcast, so a
    // notification that beats the receiver is lost — and this one is the
    // gate Bob must not pay before.
    let mut notifications = xbt.notifications().await.context("bob: notifications")?;

    let addr = offer_address(&plane.public_key(), d);
    plane
        .send(&taker, &addr, Some(rfq_id), &Trade::Quote(Quote {
            counter_invoice,
            expiry: party::now() + QUOTE_VALID_SECS,
        }))
        .await?;
    info!("quoted {counter_amount} msat for {destination_amount} msat");

    // ── 5. Wait to be paid. Paying is accepting ──────────────────────
    //
    // **He must not pay before this.** Paying first hands over BTC with
    // nothing of Alice's pending and no step compelling her to pay.
    let accepted = notifications
        .wait_for(WalletNotificationType::HoldInvoiceAccepted, Duration::from_secs(180))
        .await
        .context("bob: alice never locked in")?;
    let accepted: HoldInvoiceAccepted = accepted.as_typed().context("bob: hold_invoice_accepted")?;
    info!(
        "alice locked in {} msat; settle by block {:?}",
        accepted.amount, accepted.settle_deadline
    );

    // ── 6. Pay the destination ───────────────────────────────────────
    //
    // Her node settles on receipt because she generated the secret, so
    // the preimage comes back to him as his own payment completes.
    let paid = btc
        .pay_invoice(PayInvoiceRequest {
            invoice: rfq.destination_invoice,
            amount: None,
            metadata: None,
        })
        .await
        .context("bob: pay_invoice to the destination")?;
    info!("paid the destination; the preimage came back");

    // ── 7. Settle, with the preimage his own payment returned ────────
    //
    // Never a value from anywhere else. Here there is nowhere else; in
    // scenario 02 there is, and it is the wrong place.
    xbt.settle_hold_invoice(SettleHoldInvoiceRequest { preimage: paid.preimage })
        .await
        .context("bob: settle_hold_invoice")?;
    info!("settled the XBT leg — bob is done");
    Ok(())
}
