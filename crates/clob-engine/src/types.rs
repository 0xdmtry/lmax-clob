use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Price = Decimal;
pub type Quantity = Decimal;
pub type OrderId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn price_bincode_round_trip() {
        let p: Price = dec!(123.45678901);
        let encoded = postcard::to_allocvec(&p).expect("serialize");
        let decoded: Price = postcard::from_bytes(&encoded).expect("deserialize");
        assert_eq!(p, decoded);
    }

    #[test]
    fn quantity_bincode_round_trip() {
        let q: Quantity = dec!(9999.00000001);
        let encoded = postcard::to_allocvec(&q).expect("serialize");
        let decoded: Quantity = postcard::from_bytes(&encoded).expect("deserialize");
        assert_eq!(q, decoded);
    }

    #[test]
    fn order_id_bincode_round_trip() {
        let id: OrderId = Uuid::new_v4();
        let encoded = postcard::to_allocvec(&id).expect("serialize");
        let decoded: OrderId = postcard::from_bytes(&encoded).expect("deserialize");
        assert_eq!(id, decoded);
    }

    #[test]
    fn side_bincode_round_trip() {
        for side in [Side::Bid, Side::Ask] {
            let encoded = postcard::to_allocvec(&side).expect("serialize");
            let decoded: Side = postcard::from_bytes(&encoded).expect("deserialize");
            assert_eq!(side, decoded);
        }
    }

    #[test]
    fn order_type_bincode_round_trip() {
        for ot in [OrderType::Limit, OrderType::Market] {
            let encoded = postcard::to_allocvec(&ot).expect("serialize");
            let decoded: OrderType = postcard::from_bytes(&encoded).expect("deserialize");
            assert_eq!(ot, decoded);
        }
    }
}
