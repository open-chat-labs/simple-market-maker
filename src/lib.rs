use async_trait::async_trait;
use itertools::Itertools;
use std::cmp::Reverse;
use std::collections::btree_map::Entry::Occupied;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;
use tokio::time::sleep;

mod icdex;
pub use icdex::ICDex;

#[async_trait]
pub trait Exchange {
    async fn stats(&self) -> Result<Stats, String>;
    async fn make_orders(&self, orders: Vec<MakeOrderRequest>) -> Result<(), String>;
    async fn cancel_orders(&self, orders: Vec<CancelOrderRequest>) -> Result<(), String>;
}

pub struct Config {
    pub increment: u64,
    pub order_size: u64,
    pub min_order_size: u64,
    pub max_buy_price: u64,
    pub min_sell_price: u64,
    pub max_orders_per_direction: u64,
    pub max_orders_to_make_per_iteration: usize,
    pub max_orders_to_cancel_per_iteration: usize,
    pub iteration_interval: Duration,
}

pub struct Stats {
    latest_price: u64,
    open_orders: Vec<Order>,
}

#[derive(Copy, Clone, Debug)]
pub enum OrderType {
    Bid,
    Ask,
}

pub struct Order {
    order_type: OrderType,
    id: String,
    price: u64,
    amount: u64,
}

pub struct MakeOrderRequest {
    order_type: OrderType,
    price: u64,
    amount: u64,
}

pub struct CancelOrderRequest {
    id: String,
}

pub async fn run<E: Exchange>(exchange: &E, config: &Config) {
    loop {
        let _ = run_once(exchange, config).await;

        sleep(config.iteration_interval).await;
    }
}

async fn run_once<E: Exchange>(exchange: &E, config: &Config) -> Result<(), String> {
    let stats = exchange.stats().await?;

    let target_orders = build_orders(stats.latest_price, config);

    let orders_to_cancel = calculate_orders_to_cancel(
        &stats.open_orders,
        &target_orders,
        config.max_orders_to_cancel_per_iteration,
    );

    let orders_to_make = calculate_orders_to_make(
        &stats.open_orders,
        target_orders,
        config.min_order_size,
        config.max_orders_to_make_per_iteration,
    );

    futures::future::try_join(
        exchange.make_orders(orders_to_make),
        exchange.cancel_orders(orders_to_cancel),
    )
    .await?;

    Ok(())
}

fn calculate_orders_to_make(
    open_orders: &[Order],
    target_orders: Vec<MakeOrderRequest>,
    min_order_size: u64,
    max_orders_to_make: usize,
) -> Vec<MakeOrderRequest> {
    let mut bids_to_make = BTreeMap::new();
    let mut asks_to_make = BTreeMap::new();
    for order in target_orders {
        match order.order_type {
            OrderType::Bid => bids_to_make.insert(order.price, order),
            OrderType::Ask => asks_to_make.insert(order.price, order),
        };
    }

    for order in open_orders {
        if let Occupied(mut e) = match order.order_type {
            OrderType::Bid => bids_to_make.entry(order.price),
            OrderType::Ask => asks_to_make.entry(order.price),
        } {
            let entry = e.get_mut();
            entry.amount = entry.amount.saturating_sub(order.amount);
            if entry.amount < min_order_size {
                e.remove();
            }
        }
    }

    bids_to_make
        .into_values()
        .rev()
        .interleave(asks_to_make.into_values())
        .take(max_orders_to_make)
        .collect()
}

fn calculate_orders_to_cancel(
    open_orders: &[Order],
    target_orders: &[MakeOrderRequest],
    max_orders_to_cancel: usize,
) -> Vec<CancelOrderRequest> {
    let mut target_bid_prices = HashSet::new();
    let mut target_ask_prices = HashSet::new();
    for order in target_orders {
        match order.order_type {
            OrderType::Bid => target_bid_prices.insert(order.price),
            OrderType::Ask => target_ask_prices.insert(order.price),
        };
    }

    let mut bids = Vec::new();
    let mut asks = Vec::new();
    for order in open_orders {
        match order.order_type {
            OrderType::Bid => {
                if !target_bid_prices.contains(&order.price) {
                    bids.push(order);
                }
            }
            OrderType::Ask => {
                if !target_ask_prices.contains(&order.price) {
                    asks.push(order);
                }
            }
        };
    }

    bids.sort_unstable_by_key(|b| Reverse(b.price));
    asks.sort_unstable_by_key(|a| a.price);

    bids.iter()
        .interleave(asks.iter())
        .take(max_orders_to_cancel)
        .map(|o| CancelOrderRequest { id: o.id.clone() })
        .collect()
}

fn build_orders(latest_price: u64, config: &Config) -> Vec<MakeOrderRequest> {
    let starting_bid = starting_bid(latest_price, config.increment);
    let starting_ask = starting_ask(latest_price, config.increment);

    let bids = (0..config.max_orders_per_direction)
        .map(|i| starting_bid - (i * config.increment))
        .skip_while(|p| *p <= config.max_buy_price)
        .map(|p| MakeOrderRequest {
            order_type: OrderType::Bid,
            price: p,
            amount: config.order_size,
        });

    let asks = (0..config.max_orders_per_direction)
        .map(|i| starting_ask + (i * config.increment))
        .skip_while(|p| *p >= config.min_sell_price)
        .map(|p| MakeOrderRequest {
            order_type: OrderType::Ask,
            price: p,
            amount: config.order_size,
        });

    Vec::from_iter(bids.chain(asks))
}

fn starting_bid(latest_price: u64, increment: u64) -> u64 {
    ((latest_price / increment) - 1) * increment
}

fn starting_ask(latest_price: u64, increment: u64) -> u64 {
    (((latest_price - 1) / increment) + 2) * increment
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn build_orders_tests() {
        let config = Config {
            increment: 10,
            order_size: 100,
            min_order_size: 10,
            max_buy_price: 1000,
            min_sell_price: 500,
            max_orders_per_direction: 10,
            max_orders_to_make_per_iteration: 10,
            max_orders_to_cancel_per_iteration: 10,
            iteration_interval: Duration::from_secs(1),
        };
    }

    #[test_case(100, 10, 90)]
    #[test_case(1001, 100, 900)]
    #[test_case(2999, 10, 2980)]
    #[test_case(100011, 2, 100008)]
    fn starting_bid_tests(latest_price: u64, increment: u64, expected: u64) {
        assert_eq!(starting_bid(latest_price, increment), expected)
    }

    #[test_case(100, 10, 110)]
    #[test_case(1001, 100, 1200)]
    #[test_case(2999, 10, 3010)]
    #[test_case(100011, 2, 100014)]
    fn starting_ask_tests(latest_price: u64, increment: u64, expected: u64) {
        assert_eq!(starting_ask(latest_price, increment), expected)
    }
}
