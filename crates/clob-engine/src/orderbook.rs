use std::collections::{BTreeMap, VecDeque};

use slab::Slab;

use crate::order::{Order, OrderStatus};
use crate::types::{OrderId, Price, Quantity, Side};

/// Slab index type — stable as long as the entry is not removed.
pub type SlabKey = usize;

/// One price level: ordered queue of slab keys (price-time priority).
type Level = VecDeque<SlabKey>;

#[derive(Debug, Default)]
pub struct OrderBook {
    /// All live orders, keyed by slab index.
    pub(crate) orders: Slab<Order>,

    /// Reverse lookup: OrderId → slab key.
    index: std::collections::HashMap<OrderId, SlabKey>,

    /// Bids: highest price first (reverse iteration of BTreeMap).
    bids: BTreeMap<Price, Level>,

    /// Asks: lowest price first (forward iteration of BTreeMap).
    asks: BTreeMap<Price, Level>,
}

impl OrderBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a resting limit order into the book.
    /// Returns the slab key assigned to the order.
    pub fn insert_order(&mut self, order: Order) -> Result<SlabKey, InsertError> {
        let price = order.price.ok_or(InsertError::MarketOrderNotAllowed)?;

        if order.quantity <= Quantity::ZERO {
            return Err(InsertError::InvalidQuantity);
        }

        let id = order.id;
        let key = self.orders.insert(order);
        self.index.insert(id, key);

        let level = match self.orders[key].side {
            Side::Bid => self.bids.entry(price).or_default(),
            Side::Ask => self.asks.entry(price).or_default(),
        };
        level.push_back(key);

        Ok(key)
    }

    /// Cancel an order by its OrderId.
    /// Marks it Cancelled and removes it from the price level.
    pub fn cancel_order(&mut self, id: OrderId) -> Result<(), CancelError> {
        let &key = self.index.get(&id).ok_or(CancelError::NotFound(id))?;
        let order = &mut self.orders[key];

        if order.is_complete() {
            return Err(CancelError::AlreadyComplete(id));
        }

        let price = order.price.ok_or(CancelError::NotFound(id))?;
        let side = order.side;
        order.status = OrderStatus::Cancelled;

        // Remove from price level.
        let level_map = match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        if let Some(level) = level_map.get_mut(&price) {
            level.retain(|&k| k != key);
            if level.is_empty() {
                level_map.remove(&price);
            }
        }

        self.orders.remove(key);
        self.index.remove(&id);

        Ok(())
    }

    /// Best (highest) bid price.
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.keys().next_back().copied()
    }

    /// Best (lowest) ask price.
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.keys().next().copied()
    }

    /// Number of resting orders at a given price level on the given side.
    pub fn level_depth(&self, side: Side, price: Price) -> usize {
        match side {
            Side::Bid => self.bids.get(&price).map_or(0, |l| l.len()),
            Side::Ask => self.asks.get(&price).map_or(0, |l| l.len()),
        }
    }

    /// Peek at the front slab key of the best bid level (for matching).
    pub fn best_bid_front(&self) -> Option<SlabKey> {
        let price = self.best_bid()?;
        self.bids.get(&price)?.front().copied()
    }

    /// Peek at the front slab key of the best ask level (for matching).
    pub fn best_ask_front(&self) -> Option<SlabKey> {
        let price = self.best_ask()?;
        self.asks.get(&price)?.front().copied()
    }

    /// Remove the front order from a price level after it has been fully filled.
    pub fn pop_front(&mut self, side: Side, price: Price) {
        let level_map = match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        if let Some(level) = level_map.get_mut(&price) {
            if let Some(key) = level.pop_front() {
                if let Some(order) = self.orders.try_remove(key) {
                    self.index.remove(&order.id);
                }
            }
            if level.is_empty() {
                level_map.remove(&price);
            }
        }
    }

    pub fn get_order(&self, key: SlabKey) -> Option<&Order> {
        self.orders.get(key)
    }

    pub fn get_order_mut(&mut self, key: SlabKey) -> Option<&mut Order> {
        self.orders.get_mut(key)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InsertError {
    #[error("market orders cannot rest in the book")]
    MarketOrderNotAllowed,
    #[error("quantity must be greater than zero")]
    InvalidQuantity,
}

#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    #[error("order not found: {0}")]
    NotFound(OrderId),
    #[error("order already complete: {0}")]
    AlreadyComplete(OrderId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderStatus;
    use crate::types::OrderType;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn limit(side: Side, price: Price, qty: Quantity, seq: u64) -> Order {
        Order {
            id: Uuid::new_v4(),
            market_id: "SOL-USDC".to_string(),
            side,
            order_type: OrderType::Limit,
            price: Some(price),
            quantity: qty,
            filled: dec!(0),
            status: OrderStatus::Open,
            sequence: seq,
        }
    }

    #[test]
    fn insert_bid_and_ask() {
        let mut book = OrderBook::new();
        book.insert_order(limit(Side::Bid, dec!(100), dec!(1), 1))
            .expect("insert bid");
        book.insert_order(limit(Side::Ask, dec!(101), dec!(1), 2))
            .expect("insert ask");

        assert_eq!(book.best_bid(), Some(dec!(100)));
        assert_eq!(book.best_ask(), Some(dec!(101)));
    }

    #[test]
    fn best_bid_is_highest() {
        let mut book = OrderBook::new();
        book.insert_order(limit(Side::Bid, dec!(99), dec!(1), 1))
            .expect("insert");
        book.insert_order(limit(Side::Bid, dec!(101), dec!(1), 2))
            .expect("insert");
        book.insert_order(limit(Side::Bid, dec!(100), dec!(1), 3))
            .expect("insert");

        assert_eq!(book.best_bid(), Some(dec!(101)));
    }

    #[test]
    fn best_ask_is_lowest() {
        let mut book = OrderBook::new();
        book.insert_order(limit(Side::Ask, dec!(102), dec!(1), 1))
            .expect("insert");
        book.insert_order(limit(Side::Ask, dec!(100), dec!(1), 2))
            .expect("insert");
        book.insert_order(limit(Side::Ask, dec!(101), dec!(1), 3))
            .expect("insert");

        assert_eq!(book.best_ask(), Some(dec!(100)));
    }

    #[test]
    fn cancel_removes_from_level() {
        let mut book = OrderBook::new();
        let order = limit(Side::Bid, dec!(100), dec!(5), 1);
        let id = order.id;
        book.insert_order(order).expect("insert");

        book.cancel_order(id).expect("cancel");

        assert_eq!(book.best_bid(), None);
        assert_eq!(book.level_depth(Side::Bid, dec!(100)), 0);
    }

    #[test]
    fn cancel_not_found_errors() {
        let mut book = OrderBook::new();
        let id = Uuid::new_v4();
        let result = book.cancel_order(id);
        assert!(matches!(result, Err(CancelError::NotFound(_))));
    }

    #[test]
    fn level_depth_multiple_orders() {
        let mut book = OrderBook::new();
        book.insert_order(limit(Side::Ask, dec!(100), dec!(1), 1))
            .expect("insert");
        book.insert_order(limit(Side::Ask, dec!(100), dec!(2), 2))
            .expect("insert");
        book.insert_order(limit(Side::Ask, dec!(100), dec!(3), 3))
            .expect("insert");

        assert_eq!(book.level_depth(Side::Ask, dec!(100)), 3);
    }

    #[test]
    fn empty_book_returns_none() {
        let book = OrderBook::new();
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn market_order_insert_rejected() {
        let mut book = OrderBook::new();
        let order = Order {
            id: Uuid::new_v4(),
            market_id: "SOL-USDC".to_string(),
            side: Side::Bid,
            order_type: OrderType::Market,
            price: None,
            quantity: dec!(1),
            filled: dec!(0),
            status: OrderStatus::Open,
            sequence: 1,
        };
        let result = book.insert_order(order);
        assert!(matches!(result, Err(InsertError::MarketOrderNotAllowed)));
    }
}
