use std::collections::BTreeMap;

use crate::config::BinanceCredentials;
use crate::error::BinanceError;
use crate::rest::RestCore;
use crate::types::{insert_opt, OrderSide, SpotOrderType, TimeInForce};
use crate::BinanceClient;

impl BinanceClient {
    fn spot_base(&self) -> &str {
        &self.cfg.endpoints.spot_rest
    }

    fn spot_creds(&self) -> Result<&BinanceCredentials, BinanceError> {
        RestCore::require_creds(&self.cfg.credentials)
    }

    // --- Genel / piyasa verisi ---

    pub async fn spot_ping(&self) -> Result<serde_json::Value, BinanceError> {
        self.core
            .get_public(self.spot_base(), "/api/v3/ping", &[])
            .await
    }

    pub async fn spot_time(&self) -> Result<serde_json::Value, BinanceError> {
        self.core
            .get_public(self.spot_base(), "/api/v3/time", &[])
            .await
    }

    pub async fn spot_exchange_info(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let base = format!("{}/api/v3/exchangeInfo", self.spot_base().trim_end_matches('/'));
        let url = match symbol {
            None => base,
            Some(s) => format!("{}?symbol={}", base, urlencoding::encode(s)),
        };
        self.core.get_url(&url).await
    }

    pub async fn spot_depth(&self, symbol: &str, limit: Option<u32>) -> Result<serde_json::Value, BinanceError> {
        let sym = urlencoding::encode(symbol);
        let b = self.spot_base().trim_end_matches('/');
        let url = match limit {
            None => format!("{}/api/v3/depth?symbol={}", b, sym),
            Some(l) => format!("{}/api/v3/depth?symbol={}&limit={}", b, sym, l),
        };
        self.core.get_url(&url).await
    }

    pub async fn spot_trades(&self, symbol: &str, limit: Option<u32>) -> Result<serde_json::Value, BinanceError> {
        let sym = urlencoding::encode(symbol);
        let b = self.spot_base().trim_end_matches('/');
        let url = match limit {
            None => format!("{}/api/v3/trades?symbol={}", b, sym),
            Some(l) => format!("{}/api/v3/trades?symbol={}&limit={}", b, sym, l),
        };
        self.core.get_url(&url).await
    }

    pub async fn spot_agg_trades(
        &self,
        symbol: &str,
        from_id: Option<u64>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut url = format!(
            "{}/api/v3/aggTrades?symbol={}",
            self.spot_base().trim_end_matches('/'),
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

    pub async fn spot_klines(
        &self,
        symbol: &str,
        interval: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut url = format!(
            "{}/api/v3/klines?symbol={}&interval={}",
            self.spot_base().trim_end_matches('/'),
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

    pub async fn spot_avg_price(&self, symbol: &str) -> Result<serde_json::Value, BinanceError> {
        let url = format!(
            "{}/api/v3/avgPrice?symbol={}",
            self.spot_base().trim_end_matches('/'),
            urlencoding::encode(symbol)
        );
        self.core.get_url(&url).await
    }

    pub async fn spot_ticker_24h(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.spot_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/api/v3/ticker/24hr", b),
            Some(s) => format!("{}/api/v3/ticker/24hr?symbol={}", b, urlencoding::encode(s)),
        };
        self.core.get_url(&url).await
    }

    pub async fn spot_ticker_price(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.spot_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/api/v3/ticker/price", b),
            Some(s) => format!("{}/api/v3/ticker/price?symbol={}", b, urlencoding::encode(s)),
        };
        self.core.get_url(&url).await
    }

    pub async fn spot_book_ticker(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let b = self.spot_base().trim_end_matches('/');
        let url = match symbol {
            None => format!("{}/api/v3/ticker/bookTicker", b),
            Some(s) => format!(
                "{}/api/v3/ticker/bookTicker?symbol={}",
                b,
                urlencoding::encode(s)
            ),
        };
        self.core.get_url(&url).await
    }

    // --- Hesap / imzalı ---

    pub async fn spot_account(&self) -> Result<serde_json::Value, BinanceError> {
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/account", BTreeMap::new(), c)
            .await
    }

    pub async fn spot_new_order_params(
        &self,
        p: BTreeMap<String, String>,
    ) -> Result<serde_json::Value, BinanceError> {
        let c = self.spot_creds()?;
        self.core
            .post_signed_form(self.spot_base(), "/api/v3/order", p, c)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn spot_new_order(
        &self,
        symbol: &str,
        side: OrderSide,
        order_type: SpotOrderType,
        time_in_force: Option<TimeInForce>,
        quantity: Option<&str>,
        quote_order_qty: Option<&str>,
        price: Option<&str>,
        new_client_order_id: Option<&str>,
        stop_price: Option<&str>,
        iceberg_qty: Option<&str>,
        new_order_resp_type: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        p.insert("side".into(), side.as_str().into());
        p.insert("type".into(), order_type.as_str().into());
        if let Some(t) = time_in_force {
            p.insert("timeInForce".into(), t.as_str().into());
        }
        insert_opt(&mut p, "quantity", quantity);
        insert_opt(&mut p, "quoteOrderQty", quote_order_qty);
        insert_opt(&mut p, "price", price);
        insert_opt(&mut p, "newClientOrderId", new_client_order_id);
        insert_opt(&mut p, "stopPrice", stop_price);
        insert_opt(&mut p, "icebergQty", iceberg_qty);
        insert_opt(&mut p, "newOrderRespType", new_order_resp_type);
        self.spot_new_order_params(p).await
    }

    pub async fn spot_cancel_order(
        &self,
        symbol: &str,
        order_id: Option<u64>,
        orig_client_order_id: Option<&str>,
        new_client_order_id: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        if let Some(id) = order_id {
            p.insert("orderId".into(), id.to_string());
        }
        insert_opt(&mut p, "origClientOrderId", orig_client_order_id);
        insert_opt(&mut p, "newClientOrderId", new_client_order_id);
        let c = self.spot_creds()?;
        self.core
            .delete_signed(self.spot_base(), "/api/v3/order", p, c)
            .await
    }

    pub async fn spot_cancel_all_open_orders(&self, symbol: &str) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        let c = self.spot_creds()?;
        self.core
            .delete_signed(self.spot_base(), "/api/v3/openOrders", p, c)
            .await
    }

    pub async fn spot_query_order(
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
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/order", p, c)
            .await
    }

    pub async fn spot_open_orders(&self, symbol: Option<&str>) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        insert_opt(&mut p, "symbol", symbol);
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/openOrders", p, c)
            .await
    }

    pub async fn spot_all_orders(
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
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/allOrders", p, c)
            .await
    }

    pub async fn spot_my_trades(
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
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/myTrades", p, c)
            .await
    }

    pub async fn spot_new_oco(&self, p: BTreeMap<String, String>) -> Result<serde_json::Value, BinanceError> {
        let c = self.spot_creds()?;
        self.core
            .post_signed_form(self.spot_base(), "/api/v3/order/oco", p, c)
            .await
    }

    pub async fn spot_cancel_oco(
        &self,
        symbol: &str,
        order_list_id: Option<i64>,
        list_client_order_id: Option<&str>,
        new_client_order_id: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        p.insert("symbol".into(), symbol.into());
        if let Some(id) = order_list_id {
            p.insert("orderListId".into(), id.to_string());
        }
        insert_opt(&mut p, "listClientOrderId", list_client_order_id);
        insert_opt(&mut p, "newClientOrderId", new_client_order_id);
        let c = self.spot_creds()?;
        self.core
            .delete_signed(self.spot_base(), "/api/v3/orderList", p, c)
            .await
    }

    pub async fn spot_query_oco(
        &self,
        order_list_id: Option<i64>,
        orig_client_order_id: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        if let Some(id) = order_list_id {
            p.insert("orderListId".into(), id.to_string());
        }
        insert_opt(&mut p, "origClientOrderId", orig_client_order_id);
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/orderList", p, c)
            .await
    }

    pub async fn spot_open_oco_orders(
        &self,
        symbol: Option<&str>,
    ) -> Result<serde_json::Value, BinanceError> {
        let mut p = BTreeMap::new();
        insert_opt(&mut p, "symbol", symbol);
        let c = self.spot_creds()?;
        self.core
            .get_signed(self.spot_base(), "/api/v3/openOrderList", p, c)
            .await
    }

    // --- userDataStream: yalnızca X-MBX-APIKEY (HMAC yok) ---

    pub async fn spot_user_data_stream_start(&self) -> Result<serde_json::Value, BinanceError> {
        let c = self.spot_creds()?;
        self.core
            .post_api_key_only(self.spot_base(), "/api/v3/userDataStream", c)
            .await
    }

    pub async fn spot_user_data_stream_keepalive(&self, listen_key: &str) -> Result<serde_json::Value, BinanceError> {
        let c = self.spot_creds()?;
        self.core
            .put_api_key_query(
                self.spot_base(),
                "/api/v3/userDataStream",
                &[("listenKey", listen_key)],
                c,
            )
            .await
    }

    pub async fn spot_user_data_stream_close(&self, listen_key: &str) -> Result<serde_json::Value, BinanceError> {
        let c = self.spot_creds()?;
        self.core
            .delete_api_key_query(
                self.spot_base(),
                "/api/v3/userDataStream",
                &[("listenKey", listen_key)],
                c,
            )
            .await
    }
}
