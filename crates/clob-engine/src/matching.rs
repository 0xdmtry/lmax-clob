use rust_decimal::Decimal;

use crate::error::EngineError;
use crate::events::Fill;
use crate::order::{Order, OrderStatus};
use crate::orderbook::OrderBook;
use crate::types::{OrderId, OrderType, Price, Quantity, Side};

pub struct MatchingEngine {
    pub book: OrderBook,
    pub market_id: String,
    sequence: u64,
}

#[derive(Debug)]
pub enum ProcessResult {
    Fills(Vec<Fill>),
    Cancelled,
    Resting,
}

impl MatchingEngine {
    pub fn new(market_id: impl Into<String>) -> Self {
        Self {
            book: OrderBook::new(),
            market_id: market_id.into(),
            sequence: 0,
        }
    }

    pub fn next_sequence(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }

    /// Main entry point. Routes to match or cancel.
    pub fn process_order(&mut self, order: Order) -> Result<ProcessResult, EngineError> {
        match order.order_type {
            OrderType::Limit => self.process_limit(order),
            OrderType::Market => self.process_market(order),
        }
    }

    pub fn cancel(&mut self, id: OrderId) -> Result<ProcessResult, EngineError> {
        self.book
            .cancel_order(id)
            .map_err(|_| EngineError::OrderNotFound(id))?;
        Ok(ProcessResult::Cancelled)
    }

    // ── Limit order ──────────────────────────────────────────────────────────

    fn process_limit(&mut self, mut taker: Order) -> Result<ProcessResult, EngineError> {
        if taker.quantity <= Decimal::ZERO {
            return Err(EngineError::InvalidQuantity);
        }

        let mut fills = Vec::new();
        self.match_against_book(&mut taker, &mut fills);

        // If not fully filled, rest the remainder.
        if taker.remaining() > Decimal::ZERO {
            self.book
                .insert_order(taker)
                .map_err(|_| EngineError::InvalidQuantity)?;
            if fills.is_empty() {
                return Ok(ProcessResult::Resting);
            }
        }

        Ok(ProcessResult::Fills(fills))
    }

    // ── Market order ─────────────────────────────────────────────────────────

    fn process_market(&mut self, mut taker: Order) -> Result<ProcessResult, EngineError> {
        if taker.quantity <= Decimal::ZERO {
            return Err(EngineError::InvalidQuantity);
        }

        let mut fills = Vec::new();
        self.sweep_book(&mut taker, &mut fills);

        // Market orders do not rest — any unfilled quantity is discarded.
        Ok(ProcessResult::Fills(fills))
    }

    // ── Core matching loop ───────────────────────────────────────────────────

    /// Match a limit taker against the opposite side, respecting price.
    fn match_against_book(&mut self, taker: &mut Order, fills: &mut Vec<Fill>) {
        loop {
            if taker.remaining() <= Decimal::ZERO {
                break;
            }

            let (maker_key, maker_price) = match taker.side {
                Side::Bid => {
                    let price = match self.book.best_ask() {
                        Some(p) => p,
                        None => break,
                    };
                    // Limit buy: only match if ask ≤ bid price.
                    if price > taker.price.unwrap_or(Decimal::ZERO) {
                        break;
                    }
                    match self.book.best_ask_front() {
                        Some(k) => (k, price),
                        None => break,
                    }
                }
                Side::Ask => {
                    let price = match self.book.best_bid() {
                        Some(p) => p,
                        None => break,
                    };
                    // Limit sell: only match if bid ≥ ask price.
                    if price < taker.price.unwrap_or(Decimal::MAX) {
                        break;
                    }
                    match self.book.best_bid_front() {
                        Some(k) => (k, price),
                        None => break,
                    }
                }
            };

            self.execute_fill(taker, maker_key, maker_price, fills);
        }
    }

    /// Sweep the book for a market order — no price check.
    fn sweep_book(&mut self, taker: &mut Order, fills: &mut Vec<Fill>) {
        loop {
            if taker.remaining() <= Decimal::ZERO {
                break;
            }

            let (maker_key, maker_price) = match taker.side {
                Side::Bid => match self.book.best_ask_front() {
                    Some(k) => {
                        let p = self.book.best_ask().unwrap_or(Decimal::ZERO);
                        (k, p)
                    }
                    None => break,
                },
                Side::Ask => match self.book.best_bid_front() {
                    Some(k) => {
                        let p = self.book.best_bid().unwrap_or(Decimal::ZERO);
                        (k, p)
                    }
                    None => break,
                },
            };

            self.execute_fill(taker, maker_key, maker_price, fills);
        }
    }

    /// Execute one fill between taker and maker at maker_price.
    fn execute_fill(
        &mut self,
        taker: &mut Order,
        maker_key: usize,
        maker_price: Price,
        fills: &mut Vec<Fill>,
    ) {
        let fill_qty = {
            let maker = match self.book.get_order(maker_key) {
                Some(o) => o,
                None => return,
            };
            taker.remaining().min(maker.remaining())
        };

        if fill_qty <= Decimal::ZERO {
            return;
        }

        self.sequence += 1;
        let seq = self.sequence;

        let maker_id = {
            let maker = match self.book.get_order_mut(maker_key) {
                Some(o) => o,
                None => return,
            };
            maker.filled += fill_qty;
            maker.status = if maker.remaining() <= Decimal::ZERO {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            };
            maker.id
        };

        taker.filled += fill_qty;
        taker.status = if taker.remaining() <= Decimal::ZERO {
            OrderStatus::Filled
        } else {
            OrderStatus::PartiallyFilled
        };

        fills.push(Fill {
            maker_order_id: maker_id,
            taker_order_id: taker.id,
            price: maker_price,
            quantity: fill_qty,
            sequence: seq,
        });

        // If maker fully filled, remove it from the book.
        if self
            .book
            .get_order(maker_key)
            .map_or(false, |o| o.remaining() <= Decimal::ZERO)
        {
            let maker_side = match taker.side {
                Side::Bid => Side::Ask,
                Side::Ask => Side::Bid,
            };
            self.book.pop_front(maker_side, maker_price);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderStatus;
    use crate::types::OrderType;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn limit_order(side: Side, price: Price, qty: Quantity, seq: u64) -> Order {
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

    fn market_order(side: Side, qty: Quantity, seq: u64) -> Order {
        Order {
            id: Uuid::new_v4(),
            market_id: "SOL-USDC".to_string(),
            side,
            order_type: OrderType::Market,
            price: None,
            quantity: qty,
            filled: dec!(0),
            status: OrderStatus::Open,
            sequence: seq,
        }
    }

    // ── Basic matching ────────────────────────────────────────────────────────

    #[test]
    fn limit_bid_matches_resting_ask() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        eng.process_order(limit_order(Side::Ask, dec!(100), dec!(5), 1))
            .unwrap();
        let result = eng
            .process_order(limit_order(Side::Bid, dec!(100), dec!(5), 2))
            .unwrap();

        match result {
            ProcessResult::Fills(fills) => {
                assert_eq!(fills.len(), 1);
                assert_eq!(fills[0].price, dec!(100));
                assert_eq!(fills[0].quantity, dec!(5));
            }
            _ => panic!("expected fills"),
        }
    }

    #[test]
    fn no_match_when_spread_exists() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        eng.process_order(limit_order(Side::Ask, dec!(101), dec!(5), 1))
            .unwrap();
        let result = eng
            .process_order(limit_order(Side::Bid, dec!(99), dec!(5), 2))
            .unwrap();

        assert!(matches!(result, ProcessResult::Resting));
        assert_eq!(eng.book.best_bid(), Some(dec!(99)));
        assert_eq!(eng.book.best_ask(), Some(dec!(101)));
    }

    #[test]
    fn partial_fill_leaves_remainder_resting() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        eng.process_order(limit_order(Side::Ask, dec!(100), dec!(3), 1))
            .unwrap();
        let result = eng
            .process_order(limit_order(Side::Bid, dec!(100), dec!(5), 2))
            .unwrap();

        match result {
            ProcessResult::Fills(fills) => {
                assert_eq!(fills.len(), 1);
                assert_eq!(fills[0].quantity, dec!(3));
                // Remaining 2 should be resting on bid side.
                assert_eq!(eng.book.best_bid(), Some(dec!(100)));
                assert_eq!(eng.book.level_depth(Side::Bid, dec!(100)), 1);
            }
            _ => panic!("expected fills"),
        }
    }

    #[test]
    fn market_order_sweeps_multiple_levels() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        eng.process_order(limit_order(Side::Ask, dec!(100), dec!(2), 1))
            .unwrap();
        eng.process_order(limit_order(Side::Ask, dec!(101), dec!(2), 2))
            .unwrap();
        eng.process_order(limit_order(Side::Ask, dec!(102), dec!(2), 3))
            .unwrap();

        let result = eng
            .process_order(market_order(Side::Bid, dec!(6), 4))
            .unwrap();

        match result {
            ProcessResult::Fills(fills) => {
                assert_eq!(fills.len(), 3);
                assert_eq!(fills[0].price, dec!(100));
                assert_eq!(fills[1].price, dec!(101));
                assert_eq!(fills[2].price, dec!(102));
            }
            _ => panic!("expected fills"),
        }
        assert_eq!(eng.book.best_ask(), None);
    }

    #[test]
    fn market_order_partial_sweep_when_insufficient_liquidity() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        eng.process_order(limit_order(Side::Ask, dec!(100), dec!(2), 1))
            .unwrap();

        let result = eng
            .process_order(market_order(Side::Bid, dec!(10), 2))
            .unwrap();

        match result {
            ProcessResult::Fills(fills) => {
                assert_eq!(fills.len(), 1);
                assert_eq!(fills[0].quantity, dec!(2));
            }
            _ => panic!("expected fills"),
        }
    }

    #[test]
    fn price_time_priority_respected() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        // Three asks at same price — first inserted should match first.
        let ask1 = limit_order(Side::Ask, dec!(100), dec!(1), 1);
        let ask2 = limit_order(Side::Ask, dec!(100), dec!(1), 2);
        let ask3 = limit_order(Side::Ask, dec!(100), dec!(1), 3);
        let id1 = ask1.id;
        eng.process_order(ask1).unwrap();
        eng.process_order(ask2).unwrap();
        eng.process_order(ask3).unwrap();

        let result = eng
            .process_order(limit_order(Side::Bid, dec!(100), dec!(1), 4))
            .unwrap();

        match result {
            ProcessResult::Fills(fills) => {
                assert_eq!(fills[0].maker_order_id, id1);
            }
            _ => panic!("expected fills"),
        }
    }

    #[test]
    fn fill_price_is_maker_price() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        // Maker ask at 100, aggressive bid at 105 — fill must be at 100 (maker price).
        eng.process_order(limit_order(Side::Ask, dec!(100), dec!(5), 1))
            .unwrap();
        let result = eng
            .process_order(limit_order(Side::Bid, dec!(105), dec!(5), 2))
            .unwrap();

        match result {
            ProcessResult::Fills(fills) => {
                assert_eq!(fills[0].price, dec!(100));
            }
            _ => panic!("expected fills"),
        }
    }

    #[test]
    fn cancel_resting_order() {
        let mut eng = MatchingEngine::new("SOL-USDC");
        let order = limit_order(Side::Ask, dec!(100), dec!(5), 1);
        let id = order.id;
        eng.process_order(order).unwrap();

        let result = eng.cancel(id).unwrap();
        assert!(matches!(result, ProcessResult::Cancelled));
        assert_eq!(eng.book.best_ask(), None);
    }

    // ── Invariant / determinism tests ────────────────────────────────────────

    #[test]
    fn determinism_same_inputs_same_fills() {
        let orders = vec![
            limit_order(Side::Ask, dec!(100), dec!(3), 1),
            limit_order(Side::Ask, dec!(101), dec!(2), 2),
            limit_order(Side::Bid, dec!(101), dec!(4), 3),
        ];

        let fills_a = run_sequence(orders.clone());
        let fills_b = run_sequence(orders.clone());

        assert_eq!(fills_a.len(), fills_b.len());
        for (a, b) in fills_a.iter().zip(fills_b.iter()) {
            assert_eq!(a.price, b.price);
            assert_eq!(a.quantity, b.quantity);
        }
    }

    fn run_sequence(orders: Vec<Order>) -> Vec<Fill> {
        let mut eng = MatchingEngine::new("SOL-USDC");
        let mut all_fills = Vec::new();
        for order in orders {
            if let Ok(ProcessResult::Fills(f)) = eng.process_order(order) {
                all_fills.extend(f);
            }
        }
        all_fills
    }

    // ── Proptest invariants ──────────────────────────────────────────────────

    // ── Invariant tests ──────────────────────────────────────────────────────

    #[test]
    fn invariant_no_order_filled_beyond_quantity() {
        let cases: &[(u64, u64)] = &[(1, 1), (5, 3), (3, 5), (10, 10), (1, 100), (100, 1), (7, 7)];
        for &(ask, bid) in cases {
            let ask_qty = Decimal::from(ask);
            let bid_qty = Decimal::from(bid);

            let mut eng = MatchingEngine::new("SOL-USDC");
            eng.process_order(limit_order(Side::Ask, dec!(100), ask_qty, 1))
                .unwrap();
            let result = eng
                .process_order(limit_order(Side::Bid, dec!(100), bid_qty, 2))
                .unwrap();

            if let ProcessResult::Fills(fills) = result {
                let total: Decimal = fills.iter().map(|f| f.quantity).sum();
                assert!(
                    total <= ask_qty,
                    "ask={ask} bid={bid}: filled {total} > ask {ask_qty}"
                );
                assert!(
                    total <= bid_qty,
                    "ask={ask} bid={bid}: filled {total} > bid {bid_qty}"
                );
            }
        }
    }

    #[test]
    fn invariant_book_bid_ask_never_cross() {
        let cases: &[(u64, u64)] = &[(99, 101), (50, 50), (1, 200), (100, 100), (75, 76)];
        for &(bid_p, ask_p) in cases {
            let bp = Decimal::from(bid_p);
            let ap = Decimal::from(ask_p);

            let mut eng = MatchingEngine::new("SOL-USDC");
            let _ = eng.process_order(limit_order(Side::Bid, bp, dec!(1), 1));
            let _ = eng.process_order(limit_order(Side::Ask, ap, dec!(1), 2));

            if let (Some(best_bid), Some(best_ask)) = (eng.book.best_bid(), eng.book.best_ask()) {
                assert!(
                    best_bid < best_ask,
                    "book crossed: bid={best_bid} ask={best_ask} (input bid={bid_p} ask={ask_p})"
                );
            }
        }
    }

    #[test]
    fn invariant_fill_quantity_always_positive() {
        let cases: &[(usize, u64)] = &[(1, 1), (1, 10), (3, 5), (5, 50), (2, 1)];
        for &(n_asks, bid_qty) in cases {
            let mut eng = MatchingEngine::new("SOL-USDC");
            for i in 0..n_asks {
                eng.process_order(limit_order(Side::Ask, dec!(100), dec!(2), i as u64 + 1))
                    .unwrap();
            }
            let result = eng
                .process_order(market_order(
                    Side::Bid,
                    Decimal::from(bid_qty),
                    n_asks as u64 + 1,
                ))
                .unwrap();

            if let ProcessResult::Fills(fills) = result {
                for fill in &fills {
                    assert!(
                        fill.quantity > Decimal::ZERO,
                        "zero fill: n_asks={n_asks} bid={bid_qty}"
                    );
                }
            }
        }
    }
}
