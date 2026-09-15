//! NIP-XZ: simple HTLC exchange over Nostr.
//!
//! Two kinds and three messages, which is the whole protocol:
//!
//! | Kind | | |
//! |---|---|---|
//! | `30200` | offer | public, addressable, binding on nobody |
//! | `23204` | trade message | NIP-44 to one party — `rfq`, `quote`, `abort` |
//!
//! **This crate carries no trade state.** Every message names the offer
//! in an `a` tag and the message it answers in an `e` tag, so either side
//! reconstructs the trade from what arrived rather than from what it
//! remembers. That is the specification's design, not a simplification —
//! a session two processes must agree on is a thing that can disagree.
//!
//! What this deliberately does **not** do is touch a wallet. An `rfq`
//! carries an invoice somebody else made; a `quote` carries a hold invoice
//! somebody else made. The protocol moves strings between two parties and
//! the parties' own nodes do the Lightning.

use std::time::Duration;

use nostr::key::{Keys, PublicKey};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};

/// Kind of an offer.
pub const OFFER_KIND: u16 = 30200;
/// Kind of a trade message.
pub const TRADE_KIND: u16 = 23204;

/// What a maker is willing to do, and on what terms.
///
/// Binding on nobody: an offer is an advertisement, and the maker commits
/// only when it sends a [`Quote`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    /// Buy-units per million sell-units. **All-in** — everything the taker
    /// pays is in this number.
    pub price: u64,
    /// Msats of the sell asset available **now**. This is the maker's
    /// outbound liquidity and nothing else, and it depletes.
    pub volume: u64,
    /// Msats of the sell asset, smallest trade.
    pub min: u64,
}

impl Offer {
    /// What a taker pays, for a destination amount.
    ///
    /// `ceil`, not `round`: the rounding goes to the maker, because a
    /// taker who could round the price down by choosing amounts has found
    /// a way to be paid for arithmetic.
    pub fn counter_amount(&self, destination_msat: u64) -> u64 {
        (destination_msat as u128 * self.price as u128).div_ceil(1_000_000) as u64
    }
}

/// A trade message's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Trade {
    /// Taker → maker. The invoice the maker must pay.
    Rfq(Rfq),
    /// Maker → taker. A firm price, as a hold invoice.
    Quote(Quote),
    /// Either → either. A reason, and the end of it.
    Abort(Abort),
}

/// The taker's request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rfq {
    /// **MUST carry an amount.** An amountless invoice would leave the
    /// maker free to pay anything and leave nothing to price against.
    pub destination_invoice: String,
}

/// The maker's firm price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// The hold invoice **is** the quotation: its amount is the price, its
    /// payment hash is the destination's, and its final CLTV is the
    /// maker's timelock requirement. Restating any of that would create a
    /// field that can disagree with the invoice.
    pub counter_invoice: String,
    /// Unix seconds. The maker's own price risk, and short.
    pub expiry: u64,
}

/// Why this trade is not happening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Abort {
    /// For a human. Not a code, because nothing branches on it.
    pub reason: String,
}

/// An offer's address — `30200:<maker>:<d>`.
pub fn offer_address(maker: &PublicKey, d: &str) -> String {
    format!("{OFFER_KIND}:{}:{d}", maker.to_hex())
}

/// Build an offer event.
pub fn offer_event(d: &str, pair: &str, offer: &Offer) -> Result<EventBuilder, serde_json::Error> {
    Ok(EventBuilder::new(Kind::Custom(OFFER_KIND), serde_json::to_string(offer)?).tags([
        Tag::identifier(d.to_string()),
        // The pair is a tag because it is what a taker filters on. The
        // terms are content because nobody filters on a price.
        Tag::parse(vec!["o".to_string(), pair.to_string()]).expect("o tag"),
    ]))
}

/// One party's view of the trade plane.
///
/// Holds a relay connection and a key, and nothing about any trade —
/// see the module note on why there is no session.
pub struct TradePlane {
    keys: Keys,
    client: Client,
}

impl TradePlane {
    /// Connect to a relay as `keys`.
    pub async fn connect(keys: Keys, relay: &str) -> Result<Self, Error> {
        let client = Client::builder().signer(keys.clone()).build();
        client.add_relay(relay).await.map_err(|e| Error::Relay(e.to_string()))?;
        client.connect().await;
        Ok(Self { keys, client })
    }

    /// This party's public key — how the counterparty addresses it.
    pub fn public_key(&self) -> PublicKey {
        self.keys.public_key()
    }

    /// Publish an offer.
    pub async fn publish_offer(&self, d: &str, pair: &str, offer: &Offer) -> Result<(), Error> {
        let e = offer_event(d, pair, offer)?;
        self.client.send_event_builder(e).await.map_err(|e| Error::Relay(e.to_string()))?;
        Ok(())
    }

    /// Find an offer for a pair, by whoever publishes one.
    ///
    /// A taker filters on the `o` tag, which is what it is for.
    pub async fn find_offer(
        &self,
        pair: &str,
        within: Duration,
    ) -> Result<Option<(PublicKey, String, Offer)>, Error> {
        let filter = Filter::new()
            .kind(Kind::Custom(OFFER_KIND))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::O), pair.to_string());
        let events = self
            .client
            .fetch_events(filter)
            .timeout(within)
            .await
            .map_err(|e| Error::Relay(e.to_string()))?;
        for event in events.iter() {
            let Ok(offer) = serde_json::from_str::<Offer>(&event.content) else { continue };
            let d = event
                .tags
                .iter()
                .find_map(|t| {
                    let s = t.as_slice();
                    (s.first().map(String::as_str) == Some("d")).then(|| s.get(1).cloned())
                })
                .flatten()
                .unwrap_or_default();
            return Ok(Some((event.pubkey, d, offer)));
        }
        Ok(None)
    }

    /// Send a trade message, naming the offer and what it answers.
    ///
    /// Returns the event id, which the reply will carry in its `e` tag.
    pub async fn send(
        &self,
        to: &PublicKey,
        offer_addr: &str,
        answers: Option<EventId>,
        trade: &Trade,
    ) -> Result<EventId, Error> {
        let payload = serde_json::to_string(trade)?;
        let ciphertext = self
            .keys
            .nip44_encrypt(to, &payload)
            .await
            .map_err(|e| Error::Signer(e.to_string()))?;

        let mut tags = vec![
            Tag::public_key(*to),
            Tag::parse(vec!["a".to_string(), offer_addr.to_string()]).expect("a tag"),
        ];
        if let Some(id) = answers {
            tags.push(Tag::event(id));
        }
        let event = EventBuilder::new(Kind::Custom(TRADE_KIND), ciphertext)
            .tags(tags)
            .sign(&self.keys)
            .await
            .map_err(|e| Error::Relay(e.to_string()))?;
        let id = event.id;
        self.client.send_event(&event).await.map_err(|e| Error::Relay(e.to_string()))?;
        Ok(id)
    }

    /// Wait for the next trade message addressed to this party.
    ///
    /// Returns the sender, the event id — which a reply must answer — and
    /// the decrypted payload.
    pub async fn recv(&self, within: Duration) -> Result<Option<(PublicKey, EventId, Trade)>, Error> {
        let me = self.keys.public_key();
        let since = Timestamp::now();
        self.client
            .subscribe(Filter::new().kind(Kind::Custom(TRADE_KIND)).pubkey(me).since(since))
            .await
            .map_err(|e| Error::Relay(e.to_string()))?;

        let deadline = tokio::time::Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            let found = self
                .client
                .fetch_events(Filter::new().kind(Kind::Custom(TRADE_KIND)).pubkey(me).since(since))
                .timeout(Duration::from_secs(2))
                .await
                .map_err(|e| Error::Relay(e.to_string()))?;
            for event in found.iter() {
                let Ok(plain) = self.keys.nip44_decrypt(&event.pubkey, &event.content).await else {
                    continue;
                };
                let Ok(trade) = serde_json::from_str::<Trade>(&plain) else { continue };
                return Ok(Some((event.pubkey, event.id, trade)));
            }
        }
    }
}

/// Why a trade message did not go, or did not arrive.
#[derive(Debug)]
pub enum Error {
    /// The relay layer failed.
    Relay(String),
    /// The signer failed.
    Signer(String),
    /// Malformed JSON.
    Json(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Relay(e) => write!(f, "relay: {e}"),
            Self::Signer(e) => write!(f, "signer: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
