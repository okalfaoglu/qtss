use std::collections::BTreeMap;

use crate::config::BinanceCredentials;
use crate::error::BinanceError;
use crate::rest::RestCore;
use crate::types::{insert_opt, FuturesOrderType, OrderSide, TimeInForce};
use crate::BinanceClient;

impl BinanceClient {
    fn fapi_base(&self) -> &str {
        &self.cfg.endpoints.usdt_futures_rest
    }

    fn fapi_creds(&self) -> Result<&BinanceCredentials, BinanceError> {
        RestCore::require_creds(&self.cfg.credentials)
    }

    // --- Piyasa ---

    pub async fn fapi_ping(&self) -> Result<serde_json::Value, BinanceError> {
        self.core
            .get_public(self.fapi_base(), "/fapi/v1/ping", &[])
            .await
    }

    pub async fn fapi_time(&self) -> Result<serde_json::Value, BinanceError> {
        self.core
            .get_public(self.fapi_base(), "/fapi/v1/time", &[])
            .await
    }

    pub async fn fapi_exchange_info(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = format!("{}/fapi/v1/exchangeInfo", self.fapi_base().trim_end_matches('/'));
        let url = match symbol {
            None => b,
            Some(s) => format!("{}?symbol={}", b, urlencoding::encode(s)),
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_depth(&self, symbol: &str, limit: Option<u32>) -> Result<serde_json::Value, BinanceError> {
        let sym = urlencoding::encode(symbol);
        let b = self.fapi_base().trim_end_matches('/');
        let url = match limit {
            None => format!("{}/fapi/v1/depth?symbol={}", b, sym),
            Some(l) => format!("{}/fapi/v1/depth?symbol={}&limit={}", b, sym, l),
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_trades(&self, symbol: &str, limit: Option<u32>) -> Result<serde_json::Value, BinanceError> {
        let sym = urlencoding::encode(symbol);
        let b = self.fapi_base().trim_end_matches('/');
        let url = match limit {
            None => format!("{}/fapi/v1/trades?symbol={}", b, sym),
            Some(l) => format!("{}/fapi/v1/trades?symbol={}&limit={}", b, sym, l),
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_agg_trades(
        &self,
        symbol: &str,
        from_id: Option<u64>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut url = format!(
            "{}/fapi/v1/aggTrades?symbol={}",
            self.fapi_base().trim_end_matches('/'),
            urlencoding::encode(symbol)
        );
        if let Some(x) = from_id {
            url.push_str(&format!("&fromId={}", x));
        }
        if let Some(x) = start_time {
            url.push_str(&format!("&startTime={}", x));
        }
        if let Some(x) = end_time {
            url.push_str(&format!("&endTime={}", x));
        }
        if let Some(x) = limit {
            url.push_str(&format!("&limit={}", x));
        }
        self.core.get_url(&url).await
    }

    pub async fn fapi_klines(
        &self,
        symbol: &str,
        interval: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut url = format!(
            "{}/fapi/v1/klines?symbol={}&interval={}",
            self.fapi_base().trim_end_matches('/'),
            urlencoding::encode(symbol),
            urlencoding::encode(interval)
        );
        if let Some(x) = start_time {
            url.push_str(&format!("&startTime={}", x));
        }
        if let Some(x) = end_time {
            url.push_str(&format!("&endTime={}", x));
        }
        if let Some(x) = limit {
            url.push_str(&format!("&limit={}", x));
        }
        self.core.get_url(&url).await
    }

    pub async fn fapi_premium_index(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.fapi_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/fapi/v1/premiumIndex", b),
            Some(s) => format!(
                "{}/fapi/v1/premiumIndex?symbol={}",
                b,
                urlencoding::encode(s)
            ),
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_funding_rate(
        &self,
        symbol: Option<&str>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(s) = symbol {
            parts.push(format!("symbol={}", urlencoding::encode(s)));
        }
        if let Some(x) = start_time {
            parts.push(format!("startTime={}", x));
        }
        if let Some(x) = end_time {
            parts.push(format!("endTime={}", x));
        }
        if let Some(x) = limit {
            parts.push(format!("limit={}", x));
        }
        let base = format!("{}/fapi/v1/fundingRate", self.fapi_base().trim_end_matches('/'));
        let url = if parts.is_empty() {
            base
        } else {
            format!("{}?{}", base, parts.join("&"))
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_ticker_24h(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.fapi_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/fapi/v1/ticker/24hr", b),
            Some(s) => format!(
                "{}/fapi/v1/ticker/24hr?symbol={}",
                b,
                urlencoding::encode(s)
            ),
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_ticker_price(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.fapi_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/fapi/v1/ticker/price", b),
            Some(s) => format!(
                "{}/fapi/v1/ticker/price?symbol={}",
                b,
                urlencoding::encode(s)
            ),
        };
        self.core.get_url(&url).await
    }

    pub async fn fapi_book_ticker(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.fapi_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/fapi/v1/ticker/bookTicker", b),
            Some(s) => format!(
                "{}/fapi/v1/ticker/bookTicker?symbol={}",
                b,
                urlencoding::encode(s)
            ),
        };
        self.core.get_url(&url).await
    }

    // --- Hesap / imzalı ---

    pub async fn fapi_account(&self) -> Result<serde_json::Value, BinanceError> {
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v2/account", BTreeMap::new(), c)
            .await
    }

    pub async fn fapi_balance(&self) -> Result<serde_json::Value, BinanceError> {
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v2/balance", BTreeMap::new(), c)
            .await
    }

    pub async fn fapi_position_risk(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        insert_opt(&mut p, "symbol", symbol);
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v2/positionRisk", p, c)
            .await
    }

    pub async fn fapi_new_order_params(
        &self,
        p: BTreeMap<String, String>,
    ) -> Result<serde_json::Value, BinanceError> {
        let c = self.fapi_creds()?;
        self.core
            .post_signed_form(self.fapi_base(), "/fapi/v1/order", p, c)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn fapi_new_order(
        &self,
        symbol: &str,
        side: OrderSide,
        position_side: Option<&str>,
        order_type: FuturesOrderType,
        time_in_force: Option<TimeInForce>,
        quantity: Option<&str>,
        reduce_only: Option<bool>,
        price: Option<&str>,
        new_client_order_id: Option<&str>,
        stop_price: Option<&str>,
        close_position: Option<bool>,
        activation_price: Option<&str>,
        callback_rate: Option<&str>,
        working_type: Option<&str>,
        price_protect: Option<bool>,
        new_order_resp_type: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        p.insert("side".into(), side.as_str().into());
        p.insert("type".into(), order_type.as_str().into());
        insert_opt(&mut p, "positionSide", position_side);
        if let Some(t) = time_in_force {
            p.insert("timeInForce".into(), t.as_str().into());
        }
        insert_opt(&mut p, "quantity", quantity);
        if let Some(r) = reduce_only {
            p.insert("reduceOnly".into(), r.to_string());
        }
        insert_opt(&mut p, "price", price);
        insert_opt(&mut p, "newClientOrderId", new_client_order_id);
        insert_opt(&mut p, "stopPrice", stop_price);
        if let Some(r) = close_position {
            p.insert("closePosition".into(), r.to_string());
        }
        insert_opt(&mut p, "activationPrice", activation_price);
        insert_opt(&mut p, "callbackRate", callback_rate);
        insert_opt(&mut p, "workingType", working_type);
        if let Some(r) = price_protect {
            p.insert("priceProtect".into(), r.to_string());
        }
        insert_opt(&mut p, "newOrderRespType", new_order_resp_type);
        self.fapi_new_order_params(p).await
    }

    pub async fn fapi_batch_orders(&self, batch_orders_json: &str) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("batchOrders".into(), batch_orders_json.into());
        let c = self.fapi_creds()?;
        self.core
            .post_signed_form(self.fapi_base(), "/fapi/v1/batchOrders", p, c)
            .await
    }

    pub async fn fapi_cancel_order(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        orig_client_order_id: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        if let Some(id) = order_id {
            p.insert("orderId".into(), id.to_string());
        }
        insert_opt(&mut p, "origClientOrderId", orig_client_order_id);
        let c = self.fapi_creds()?;
        self.core
            .delete_signed(self.fapi_base(), "/fapi/v1/order", p, c)
            .await
    }

    pub async fn fapi_cancel_all_open_orders(&self, symbol: &str) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        let c = self.fapi_creds()?;
        self.core
            .delete_signed(self.fapi_base(), "/fapi/v1/allOpenOrders", p, c)
            .await
    }

    pub async fn fapi_query_order(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        orig_client_order_id: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        if let Some(id) = order_id {
            p.insert("orderId".into(), id.to_string());
        }
        insert_opt(&mut p, "origClientOrderId", orig_client_order_id);
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v1/order", p, c)
            .await
    }

    pub async fn fapi_open_orders(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        insert_opt(&mut p, "symbol", symbol);
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v1/openOrders", p, c)
            .await
    }

    pub async fn fapi_all_orders(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        if let Some(id) = order_id {
            p.insert("orderId".into(), id.to_string());
        }
        if let Some(t) = start_time {
            p.insert("startTime".into(), t.to_string());
        }
        if let Some(t) = end_time {
            p.insert("endTime".into(), t.to_string());
        }
        if let Some(l) = limit {
            p.insert("limit".into(), l.to_string());
        }
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v1/allOrders", p, c)
            .await
    }

    pub async fn fapi_user_trades(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        from_id: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        if let Some(id) = order_id {
            p.insert("orderId".into(), id.to_string());
        }
        if let Some(t) = start_time {
            p.insert("startTime".into(), t.to_string());
        }
        if let Some(t) = end_time {
            p.insert("endTime".into(), t.to_string());
        }
        if let Some(id) = from_id {
            p.insert("fromId".into(), id.to_string());
        }
        if let Some(l) = limit {
            p.insert("limit".into(), l.to_string());
        }
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v1/userTrades", p, c)
            .await
    }

    pub async fn fapi_income(
        &self,
        symbol: Option<&str>,
        income_type: Option<&str>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        insert_opt(&mut p, "symbol", symbol);
        insert_opt(&mut p, "incomeType", income_type);
        if let Some(t) = start_time {
            p.insert("startTime".into(), t.to_string());
        }
        if let Some(t) = end_time {
            p.insert("endTime".into(), t.to_string());
        }
        if let Some(l) = limit {
            p.insert("limit".into(), l.to_string());
        }
        let c = self.fapi_creds()?;
        self.core
            .get_signed(self.fapi_base(), "/fapi/v1/income", p, c)
            .await
    }

    // --- userDataStream (yalnızca API key) ---

    pub async fn fapi_user_data_stream_start(&self) -> Result<serde_json::Value, BinanceError> {
        let c = self.fapi_creds()?;
        self.core
            .post_api_key_only(self.fapi_base(), "/fapi/v1/listenKey", c)
            .await
    }

    pub async fn fapi_user_data_stream_keepalive(&self, listen_key: &str) -> Result<serde_json::Value, BinanceError> {
        let c = self.fapi_creds()?;
        self.core
            .put_api_key_query(
                self.fapi_base(),
                "/fapi/v1/listenKey",
                &[("listenKey", listen_key)],
                c,
            )
            .await
    }

    pub async fn fapi_user_data_stream_close(&self, listen_key: &str) -> Result<serde_json::Value, BinanceError> {
        let c = self.fapi_creds()?;
        self.core
            .delete_api_key_query(
                self.fapi_base(),
                "/fapi/v1/listenKey",
                &[("listenKey", listen_key)],
                c,
            )
            .await
    }
}
