use crate::types::OrderId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("order not found: {0}")]
    OrderNotFound(OrderId),

    #[error("order already complete: {0}")]
    OrderAlreadyComplete(OrderId),

    #[error("invalid price: limit order must have a price")]
    MissingLimitPrice,

    #[error("invalid quantity: must be greater than zero")]
    InvalidQuantity,

    #[error("snapshot serialization failed: {0}")]
    SnapshotSerialize(#[from] postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn order_not_found_display() {
        let id = Uuid::new_v4();
        let err = EngineError::OrderNotFound(id);
        assert!(err.to_string().contains(&id.to_string()));
    }

    #[test]
    fn missing_limit_price_display() {
        let err = EngineError::MissingLimitPrice;
        assert!(!err.to_string().is_empty());
    }
}
