use serde::{Deserialize, Serialize};

use crate::order::{Order, OrderStatus};
use crate::orderbook::OrderBook;
use crate::types::{OrderId, OrderType, Side};

/// Portable snapshot — owns all data, no slab indices (they are not stable across processes).
/// On restore, orders are re-inserted to rebuild a fresh Slab + index.
#[derive(Debug, Serialize, Deserialize)]
pub struct BookSnapshot {
    pub market_id: String,
    pub sequence: u64,
    /// All resting orders at snapshot time, in price-time order per level.
    pub orders: Vec<SnapshotOrder>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotOrder {
    pub id: OrderId,
    pub market_id: String,
    pub side: Side,
    pub order_type: OrderType,
    pub price: rust_decimal::Decimal,
    pub quantity: rust_decimal::Decimal,
    pub filled: rust_decimal::Decimal,
    pub status: OrderStatus,
    pub sequence: u64,
}

pub fn serialize_book(
    book: &OrderBook,
    market_id: &str,
    sequence: u64,
) -> Result<Vec<u8>, postcard::Error> {
    // Collect orders in price-time order so restore is deterministic.
    let mut orders: Vec<SnapshotOrder> = Vec::with_capacity(book.orders.len());
    for (_, order) in &book.orders {
        orders.push(SnapshotOrder {
            id: order.id,
            market_id: order.market_id.clone(),
            side: order.side,
            order_type: order.order_type,
            price: order.price.unwrap_or_default(),
            quantity: order.quantity,
            filled: order.filled,
            status: order.status,
            sequence: order.sequence,
        });
    }

    let snapshot = BookSnapshot {
        market_id: market_id.to_string(),
        sequence,
        orders,
    };

    postcard::to_allocvec(&snapshot)
}

pub fn deserialize_book(bytes: &[u8]) -> Result<(OrderBook, BookSnapshot), postcard::Error> {
    let snapshot: BookSnapshot = postcard::from_bytes(bytes)?;
    let mut book = OrderBook::new();

    for so in &snapshot.orders {
        let order = Order {
            id: so.id,
            market_id: so.market_id.clone(),
            side: so.side,
            order_type: so.order_type,
            price: Some(so.price),
            quantity: so.quantity,
            filled: so.filled,
            status: so.status,
            sequence: so.sequence,
        };
        // Re-insert only resting orders (not Filled/Cancelled).
        if !order.is_complete() {
            let _ = book.insert_order(order);
        }
    }

    Ok((book, snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderStatus;
    use crate::orderbook::OrderBook;
    use crate::types::{OrderType, Side};
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn limit(
        side: Side,
        price: rust_decimal::Decimal,
        qty: rust_decimal::Decimal,
        seq: u64,
    ) -> Order {
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
    fn snapshot_round_trip_empty_book() {
        let book = OrderBook::new();
        let bytes = serialize_book(&book, "SOL-USDC", 0).expect("serialize");
        let (restored, meta) = deserialize_book(&bytes).expect("deserialize");
        assert_eq!(meta.market_id, "SOL-USDC");
        assert_eq!(meta.sequence, 0);
        assert_eq!(restored.best_bid(), None);
        assert_eq!(restored.best_ask(), None);
    }

    #[test]
    fn snapshot_round_trip_with_orders() {
        let mut book = OrderBook::new();
        book.insert_order(limit(Side::Bid, dec!(100), dec!(5), 1))
            .expect("insert");
        book.insert_order(limit(Side::Bid, dec!(99), dec!(3), 2))
            .expect("insert");
        book.insert_order(limit(Side::Ask, dec!(101), dec!(2), 3))
            .expect("insert");

        let bytes = serialize_book(&book, "SOL-USDC", 42).expect("serialize");
        let (restored, meta) = deserialize_book(&bytes).expect("deserialize");

        assert_eq!(meta.sequence, 42);
        assert_eq!(restored.best_bid(), Some(dec!(100)));
        assert_eq!(restored.best_ask(), Some(dec!(101)));
    }

    #[test]
    fn snapshot_excludes_cancelled_orders() {
        let mut book = OrderBook::new();
        let order = limit(Side::Bid, dec!(100), dec!(5), 1);
        let id = order.id;
        book.insert_order(order).expect("insert");
        book.cancel_order(id).expect("cancel");

        let bytes = serialize_book(&book, "SOL-USDC", 10).expect("serialize");
        // cancelled order is no longer in book at snapshot time
        let (restored, _) = deserialize_book(&bytes).expect("deserialize");
        assert_eq!(restored.best_bid(), None);
    }

    #[test]
    fn snapshot_state_identical_after_round_trip() {
        let mut book = OrderBook::new();
        book.insert_order(limit(Side::Bid, dec!(100), dec!(10), 1))
            .expect("insert");
        book.insert_order(limit(Side::Ask, dec!(102), dec!(5), 2))
            .expect("insert");

        let bytes = serialize_book(&book, "SOL-USDC", 99).expect("serialize");
        let (restored, _) = deserialize_book(&bytes).expect("deserialize");

        assert_eq!(book.best_bid(), restored.best_bid());
        assert_eq!(book.best_ask(), restored.best_ask());
        assert_eq!(
            book.level_depth(Side::Bid, dec!(100)),
            restored.level_depth(Side::Bid, dec!(100))
        );
        assert_eq!(
            book.level_depth(Side::Ask, dec!(102)),
            restored.level_depth(Side::Ask, dec!(102))
        );
    }
}
