use crate::{
    query, update, update_no_response, CancelOrderRequest, Exchange, MakeOrderRequest, Order,
    OrderType, Stats,
};
use async_trait::async_trait;
use candid::{CandidType, Nat, Principal};
use ic_agent::Agent;
use serde::Deserialize;
use std::time::Duration;

pub struct ICDex {
    agent: Agent,
    dex_canister_id: Principal,
    trader_canister_id: Principal,
}

impl ICDex {
    pub fn new(agent: Agent, dex_canister_id: Principal, trader_canister_id: Principal) -> Self {
        ICDex {
            agent,
            dex_canister_id,
            trader_canister_id,
        }
    }

    async fn latest_price(&self) -> Result<u64, String> {
        let response: StatsResponse =
            query(&self.agent, &self.dex_canister_id, "stats", ()).await?;

        Ok((response.price * 100000000f64) as u64)
    }

    async fn open_orders(&self) -> Result<Vec<Order>, String> {
        let orders: TrieList = query(
            &self.agent,
            &self.dex_canister_id,
            "pending",
            (
                self.trader_canister_id.to_string(),
                Option::<Nat>::None,
                Option::<Nat>::None,
            ),
        )
        .await?;

        Ok(orders.data.into_iter().map(|(_, o)| o.into()).collect())
    }

    async fn make_order(&self, order: MakeOrderRequest) -> Result<String, String> {
        let price = order.price as f64 / 100000000f64;
        let args = (
            self.dex_canister_id,
            Side::from(order.order_type),
            price,
            Nat(order.amount.into()),
        );

        let response: MakeOrderResponse =
            update(&self.agent, &self.trader_canister_id, "order", args).await?;

        match response {
            MakeOrderResponse::Ok(r) => Ok(hex::encode(r.txid)),
            MakeOrderResponse::Err(err) => Err(format!("{err:?}")),
        }
    }

    async fn cancel_order(&self, order: CancelOrderRequest) -> Result<(), String> {
        let id = hex::decode(order.id).unwrap();

        update_no_response(
            &self.agent,
            &self.trader_canister_id,
            "cancel",
            (self.dex_canister_id, id),
        )
        .await?;

        Ok(())
    }
}

#[async_trait]
impl Exchange for ICDex {
    async fn stats(&self) -> Result<Stats, String> {
        let open_orders = self.open_orders().await?;
        let latest_price = self.latest_price().await?;

        Ok(Stats {
            latest_price,
            open_orders,
        })
    }

    async fn make_orders(&self, orders: Vec<MakeOrderRequest>) -> Result<(), String> {
        for order in orders {
            self.make_order(order).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Ok(())
    }

    async fn cancel_orders(&self, orders: Vec<CancelOrderRequest>) -> Result<(), String> {
        for order in orders {
            self.cancel_order(order).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Ok(())
    }
}

#[derive(CandidType, Deserialize)]
struct TrieList {
    data: Vec<(Vec<u8>, TradingOrder)>,
    total: Nat,
    #[serde(rename = "totalPage")]
    total_page: Nat,
}

#[derive(CandidType, Deserialize)]
struct StatsResponse {
    price: f64,
}

#[derive(CandidType, Debug)]
enum Side {
    Buy,
    Sell,
}

impl From<OrderType> for Side {
    fn from(value: OrderType) -> Self {
        match value {
            OrderType::Bid => Side::Buy,
            OrderType::Ask => Side::Sell,
        }
    }
}

#[derive(CandidType, Deserialize)]
struct TradingOrder {
    remaining: OrderPrice,
    txid: Vec<u8>,
}

impl From<TradingOrder> for Order {
    fn from(value: TradingOrder) -> Self {
        let (order_type, amount) = match value.remaining.quantity {
            OrderQuantity::Buy(n, _) => (OrderType::Bid, n),
            OrderQuantity::Sell(n) => (OrderType::Ask, n),
        };
        let price: u64 = value.remaining.price.0.try_into().unwrap();
        Order {
            order_type,
            id: hex::encode(value.txid),
            price: price * 10, // TODO remove the '* 10' once fixed on their side
            amount: amount.0.try_into().unwrap(),
        }
    }
}

#[derive(CandidType, Deserialize)]
enum OrderQuantity {
    Buy(Nat, Nat),
    Sell(Nat),
}

#[derive(CandidType, Deserialize)]
struct OrderPrice {
    price: Nat,
    quantity: OrderQuantity,
}

#[derive(CandidType, Deserialize)]
enum MakeOrderResponse {
    #[serde(rename = "ok")]
    Ok(MakeOrderSuccess),
    #[serde(rename = "err")]
    Err(MakeOrderError),
}

#[derive(CandidType, Deserialize)]
struct MakeOrderSuccess {
    txid: Vec<u8>,
}

#[derive(CandidType, Deserialize, Debug)]
struct MakeOrderError {
    code: MakeOrderErrorCode,
    message: String,
}

#[derive(CandidType, Deserialize, Debug)]
enum MakeOrderErrorCode {
    NonceError,
    InvalidAmount,
    InsufficientBalance,
    TransferException,
    UnacceptableVolatility,
    TransactionBlocking,
    UndefinedError,
}
