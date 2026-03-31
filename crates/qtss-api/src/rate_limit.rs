//! Rate limit anahtarı: doğrudan peer IP veya güvenilen vekil + `X-Forwarded-For`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::http::Request;
use ipnet::IpNet;
use tower_governor::key_extractor::KeyExtractor;

use qtss_storage::resolve_system_string;

pub const X_FORWARDED_FOR: &str = "x-forwarded-for";

#[derive(Clone, Debug)]
enum TrustEntry {
    Addr(IpAddr),
    Net(IpNet),
}

impl TrustEntry {
    fn contains(&self, ip: IpAddr) -> bool {
        match self {
            TrustEntry::Addr(a) => *a == ip,
            TrustEntry::Net(n) => n.contains(&ip),
        }
    }
}

fn is_trusted_peer(peer: IpAddr, trusted: &[TrustEntry]) -> bool {
    trusted.iter().any(|t| t.contains(peer))
}

fn first_forwarded_client(header_val: &str) -> Option<IpAddr> {
    header_val.split(',').next()?.trim().parse().ok()
}

/// `QTSS_TRUSTED_PROXIES`: virgülle ayrılmış IP veya CIDR (`10.0.0.0/8`).
/// **Boş string** = vekil güveni yok (yalnızca TCP peer IP). Tanımsız = yalnızca loopback vekil.
#[derive(Clone)]
pub struct ForwardedIpKeyExtractor {
    trusted: Arc<Vec<TrustEntry>>,
}

impl ForwardedIpKeyExtractor {
    pub async fn from_config(pool: &sqlx::PgPool) -> Self {
        let raw = resolve_system_string(pool, "api", "trusted_proxies_csv", "QTSS_TRUSTED_PROXIES", "").await;
        let trusted = if raw.trim().is_empty() {
            // Empty means: do not trust any proxies (only peer IP).
            Arc::new(vec![])
        } else {
            Arc::new(
                raw.split(',')
                    .filter_map(|p| {
                        let p = p.trim();
                        if p.is_empty() {
                            return None;
                        }
                        if let Ok(net) = p.parse::<IpNet>() {
                            return Some(TrustEntry::Net(net));
                        }
                        p.parse::<IpAddr>().ok().map(TrustEntry::Addr)
                    })
                    .collect::<Vec<_>>(),
            )
        };
        Self { trusted }
    }
}

impl KeyExtractor for ForwardedIpKeyExtractor {
    type Key = String;

    fn extract<B>(&self, req: &Request<B>) -> Result<Self::Key, tower_governor::GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());

        let ip = match peer {
            Some(peer_ip) if is_trusted_peer(peer_ip, &self.trusted) => req
                .headers()
                .get(X_FORWARDED_FOR)
                .and_then(|v| v.to_str().ok())
                .and_then(first_forwarded_client)
                .unwrap_or(peer_ip),
            Some(peer_ip) => peer_ip,
            None => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        };

        Ok(ip.to_string())
    }
}
