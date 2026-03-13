use crate::types::{OrderId, OrderType, Price, Quantity, Side};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub maker_order_id: OrderId,
    pub taker_order_id: OrderId,
    pub price: Price,
    pub quantity: Quantity,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderEvent {
    OrderPlaced {
        order_id: OrderId,
        market_id: String,
        side: Side,
        order_type: OrderType,
        price: Option<Price>,
        quantity: Quantity,
        sequence: u64,
    },
    OrderCancelled {
        order_id: OrderId,
        sequence: u64,
    },
    OrderFilled {
        fill: Fill,
    },
    OrderPartiallyFilled {
        fill: Fill,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_fill() -> Fill {
        Fill {
            maker_order_id: Uuid::new_v4(),
            taker_order_id: Uuid::new_v4(),
            price: dec!(100.50),
            quantity: dec!(2.0),
            sequence: 42,
        }
    }

    #[test]
    fn fill_bincode_round_trip() {
        let fill = make_fill();
        let encoded = postcard::to_allocvec(&fill).expect("serialize");
        let decoded: Fill = postcard::from_bytes(&encoded).expect("deserialize");
        assert_eq!(fill.maker_order_id, decoded.maker_order_id);
        assert_eq!(fill.price, decoded.price);
        assert_eq!(fill.quantity, decoded.quantity);
    }

    #[test]
    fn order_placed_bincode_round_trip() {
        let event = OrderEvent::OrderPlaced {
            order_id: Uuid::new_v4(),
            market_id: "SOL-USDC".to_string(),
            side: Side::Bid,
            order_type: OrderType::Limit,
            price: Some(dec!(150.0)),
            quantity: dec!(5.0),
            sequence: 1,
        };
        let encoded = postcard::to_allocvec(&event).expect("serialize");
        let _decoded: OrderEvent = postcard::from_bytes(&encoded).expect("deserialize");
    }

    #[test]
    fn order_cancelled_bincode_round_trip() {
        let event = OrderEvent::OrderCancelled {
            order_id: Uuid::new_v4(),
            sequence: 2,
        };
        let encoded = postcard::to_allocvec(&event).expect("serialize");
        let _decoded: OrderEvent = postcard::from_bytes(&encoded).expect("deserialize");
    }

    #[test]
    fn order_filled_bincode_round_trip() {
        let event = OrderEvent::OrderFilled { fill: make_fill() };
        let encoded = postcard::to_allocvec(&event).expect("serialize");
        let _decoded: OrderEvent = postcard::from_bytes(&encoded).expect("deserialize");
    }

    #[test]
    fn order_partially_filled_bincode_round_trip() {
        let event = OrderEvent::OrderPartiallyFilled { fill: make_fill() };
        let encoded = postcard::to_allocvec(&event).expect("serialize");
        let _decoded: OrderEvent = postcard::from_bytes(&encoded).expect("deserialize");
    }
}
