//! Per-IP rate limiting (tower_governor) for the abuse-prone endpoints:
//! `/auth/login` (online brute force / enumeration) and the public submission
//! POST (DB flooding / CRM-quota spam).
//!
//! Keyed by peer IP (`PeerIpKeyExtractor`), which requires the server to be
//! served with `into_make_service_with_connect_info::<SocketAddr>()`. Behind a
//! reverse proxy every request would otherwise share the proxy's IP — deploy
//! with a proxy that sets a trusted forwarded header and switch to
//! `SmartIpKeyExtractor` (it trusts `X-Forwarded-For`, so the proxy MUST
//! overwrite that header).

use std::sync::Arc;
use std::time::Duration;

use governor::middleware::NoOpMiddleware;
use tower_governor::GovernorLayer;
use tower_governor::governor::{GovernorConfig, GovernorConfigBuilder};
use tower_governor::key_extractor::PeerIpKeyExtractor;

type PeerConfig = GovernorConfig<PeerIpKeyExtractor, NoOpMiddleware>;
pub type PeerGovernorLayer = GovernorLayer<PeerIpKeyExtractor, NoOpMiddleware>;

/// Build a per-IP governor layer (`burst_size` tokens, one replenished every
/// `per_second` seconds) and spawn a background task that periodically evicts
/// idle buckets so memory doesn't grow unboundedly with distinct IPs.
fn build_layer(per_second: u64, burst_size: u32) -> PeerGovernorLayer {
    let config: Arc<PeerConfig> = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(per_second)
            .burst_size(burst_size)
            .finish()
            .expect("valid governor rate-limit config"),
    );
    let limiter = config.limiter().clone();
    // Called from inside the tokio runtime (router built from async main).
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            limiter.retain_recent();
        }
    });
    GovernorLayer { config }
}

/// Strict limiter for `/auth/login`: ~5 attempts then 1 per 2s per IP.
pub fn login_layer() -> PeerGovernorLayer {
    build_layer(2, 5)
}

/// Looser limiter for the public form surface: burst 20 then 1/s per IP —
/// generous enough for a legitimate page load (schema GET + one submit) while
/// blunting floods.
pub fn public_layer() -> PeerGovernorLayer {
    build_layer(1, 20)
}
