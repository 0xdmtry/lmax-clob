use crate::types::{OrderId, OrderType, Price, Quantity, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub market_id: String,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Option<Price>,
    pub quantity: Quantity,
    pub filled: Quantity,
    pub status: OrderStatus,
    pub sequence: u64,
}

impl Order {
    pub fn remaining(&self) -> Quantity {
        self.quantity - self.filled
    }

    pub fn is_complete(&self) -> bool {
        self.status == OrderStatus::Filled || self.status == OrderStatus::Cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_limit_order() -> Order {
        Order {
            id: Uuid::new_v4(),
            market_id: "SOL-USDC".to_string(),
            side: Side::Bid,
            order_type: OrderType::Limit,
            price: Some(dec!(150.00)),
            quantity: dec!(10.0),
            filled: dec!(0),
            status: OrderStatus::Open,
            sequence: 1,
        }
    }

    fn make_market_order() -> Order {
        Order {
            id: Uuid::new_v4(),
            market_id: "SOL-USDC".to_string(),
            side: Side::Ask,
            order_type: OrderType::Market,
            price: None,
            quantity: dec!(5.0),
            filled: dec!(0),
            status: OrderStatus::Open,
            sequence: 2,
        }
    }

    #[test]
    fn limit_order_bincode_round_trip() {
        let order = make_limit_order();
        let encoded = postcard::to_allocvec(&order).expect("serialize");
        let decoded: Order = postcard::from_bytes(&encoded).expect("deserialize");
        assert_eq!(order.id, decoded.id);
        assert_eq!(order.price, decoded.price);
        assert_eq!(order.quantity, decoded.quantity);
        assert_eq!(order.status, decoded.status);
    }

    #[test]
    fn market_order_bincode_round_trip() {
        let order = make_market_order();
        let encoded = postcard::to_allocvec(&order).expect("serialize");
        let decoded: Order = postcard::from_bytes(&encoded).expect("deserialize");
        assert_eq!(order.id, decoded.id);
        assert_eq!(order.price, decoded.price);
    }

    #[test]
    fn remaining_quantity() {
        let mut order = make_limit_order();
        order.filled = dec!(3.0);
        assert_eq!(order.remaining(), dec!(7.0));
    }

    #[test]
    fn is_complete_filled() {
        let mut order = make_limit_order();
        order.status = OrderStatus::Filled;
        assert!(order.is_complete());
    }

    #[test]
    fn is_complete_cancelled() {
        let mut order = make_limit_order();
        order.status = OrderStatus::Cancelled;
        assert!(order.is_complete());
    }

    #[test]
    fn is_not_complete_open() {
        let order = make_limit_order();
        assert!(!order.is_complete());
    }
}
