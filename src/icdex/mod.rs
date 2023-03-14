use crate::{CancelOrderRequest, Exchange, MakeOrderRequest, OrderType, Stats};
use async_trait::async_trait;
use candid::{CandidType, Principal};
use ic_agent::Agent;

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

    async fn make_order(&self, order: MakeOrderRequest) -> Result<(), String> {
        let price = order.price as f64 / 100000000f64;

        self.agent
            .update(&self.trader_canister_id, "order")
            .with_arg(
                candid::encode_args((
                    self.dex_canister_id,
                    Side::from(order.order_type),
                    price,
                    candid::Nat(order.amount.into()),
                ))
                .unwrap(),
            )
            .call_and_wait()
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    async fn cancel_order(&self, order: CancelOrderRequest) -> Result<(), String> {
        self.agent
            .update(&self.trader_canister_id, "cancel")
            .with_arg(candid::encode_args((self.dex_canister_id, order.id.as_bytes())).unwrap())
            .call_and_wait()
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}

#[async_trait]
impl Exchange for ICDex {
    async fn stats(&self) -> Result<Stats, String> {
        todo!()
    }

    async fn make_orders(&self, orders: Vec<MakeOrderRequest>) -> Result<(), String> {
        futures::future::try_join_all(orders.into_iter().map(|o| self.make_order(o))).await?;
        Ok(())
    }

    async fn cancel_orders(&self, orders: Vec<CancelOrderRequest>) -> Result<(), String> {
        futures::future::try_join_all(orders.into_iter().map(|o| self.cancel_order(o))).await?;
        Ok(())
    }
}

#[derive(CandidType)]
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
