//! What both parties need and neither owns.
//!
//! Configuration, logging, and two BOLT11 readers. **Nothing here knows
//! about a trade**, and nothing here is shared *state* — Alice and Bob are
//! separate processes and this is a library they each link, not a channel
//! between them.

use anyhow::{Context, Result};

/// One party's environment.
///
/// Each party is given connection URIs for **its own two nodes only**.
/// There is deliberately no field for the counterparty's anything: what
/// one needs from the other arrives as a trade message or not at all.
pub struct Config {
    /// This party's node on the BTC chain.
    pub btc_uri: String,
    /// This party's node on the XBT chain.
    pub xbt_uri: String,
    /// The relay both planes run over.
    pub relay: String,
    /// This party's Nostr identity on the trade plane.
    pub nostr_secret: String,
    /// Parts per million. Read by the maker; ignored by the taker, who
    /// learns the price from the offer.
    pub price_ppm: u64,
    /// Msats the taker wants at the destination.
    pub amount_msat: u64,
}

impl Config {
    /// Read it, prefixed — `ALICE_` or `BOB_`.
    pub fn from_env(who: &str) -> Result<Self> {
        let get = |k: &str| -> Result<String> {
            std::env::var(format!("{who}_{k}"))
                .with_context(|| format!("{who}_{k} must be in the environment"))
        };
        Ok(Self {
            btc_uri: get("BTC_URI")?,
            xbt_uri: get("XBT_URI")?,
            relay: get("RELAY")?,
            nostr_secret: get("NOSTR_SECRET")?,
            price_ppm: get("PRICE_PPM").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000),
            amount_msat: get("AMOUNT_MSAT").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000),
        })
    }
}

/// Logging, on stderr, so a party's own output stays readable.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,nostr_relay_pool=warn,nostr_sdk=warn".into()),
        )
        .try_init();
}

/// Unix seconds.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The amount a BOLT11 states, in msats.
///
/// `None` where it states none — which for a destination invoice is a
/// protocol error rather than a default, because it would leave the maker
/// free to pay anything.
pub fn invoice_amount_msat(invoice: &str) -> Option<u64> {
    use lightning_invoice::Bolt11Invoice;
    use std::str::FromStr;
    Bolt11Invoice::from_str(invoice).ok()?.amount_milli_satoshis()
}

/// The payment hash a BOLT11 carries, as lowercase hex.
pub fn invoice_payment_hash(invoice: &str) -> Result<String> {
    use lightning_invoice::Bolt11Invoice;
    use std::str::FromStr;
    let inv = Bolt11Invoice::from_str(invoice)
        .map_err(|e| anyhow::anyhow!("not a bolt11 invoice: {e:?}"))?;
    Ok(inv.payment_hash().to_string())
}
