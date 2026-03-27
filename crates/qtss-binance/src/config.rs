/// Binance REST uç noktaları (mainnet / testnet).
#[derive(Debug, Clone)]
pub struct BinanceEndpoints {
    pub spot_rest: String,
    pub usdt_futures_rest: String,
}

impl BinanceEndpoints {
    pub fn mainnet() -> Self {
        Self {
            spot_rest: "https://api.binance.com".into(),
            usdt_futures_rest: "https://fapi.binance.com".into(),
        }
    }

    pub fn testnet() -> Self {
        Self {
            spot_rest: "https://testnet.binance.vision".into(),
            usdt_futures_rest: "https://testnet.binancefuture.com".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BinanceCredentials {
    pub api_key: String,
    pub api_secret: String,
}

#[derive(Debug, Clone)]
pub struct BinanceClientConfig {
    pub endpoints: BinanceEndpoints,
    pub recv_window_ms: u64,
    /// Spot ve FAPI aynı anahtar çiftini kullanır.
    pub credentials: Option<BinanceCredentials>,
}

impl BinanceClientConfig {
    pub fn public_mainnet() -> Self {
        Self {
            endpoints: BinanceEndpoints::mainnet(),
            recv_window_ms: 5_000,
            credentials: None,
        }
    }

    pub fn mainnet_with_keys(key: String, secret: String) -> Self {
        Self {
            endpoints: BinanceEndpoints::mainnet(),
            recv_window_ms: 5_000,
            credentials: Some(BinanceCredentials {
                api_key: key,
                api_secret: secret,
            }),
        }
    }
}
