use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use clob_engine::{
    matching::MatchingEngine,
    order::{Order, OrderStatus},
    types::{OrderType, Side},
};

pub fn make_limit(side: Side, price: Decimal, qty: Decimal, seq: u64) -> Order {
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

pub fn make_market(side: Side, qty: Decimal, seq: u64) -> Order {
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

/// Single limit order match: one resting ask, one incoming bid.
fn bench_single_limit_match(c: &mut Criterion) {
    c.bench_function("single_limit_match", |b| {
        b.iter(|| {
            let mut eng = MatchingEngine::new("SOL-USDC");
            eng.process_order(make_limit(Side::Ask, dec!(100), dec!(10), 1))
                .expect("insert ask");
            let result = eng
                .process_order(make_limit(Side::Bid, dec!(100), dec!(10), 2))
                .expect("match bid");
            black_box(result);
        });
    });
}

/// Market order sweeping N price levels.
fn bench_market_sweep(c: &mut Criterion) {
    let depths = [1usize, 10, 50, 100];
    let mut group = c.benchmark_group("market_sweep");

    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| {
                let mut eng = MatchingEngine::new("SOL-USDC");
                for i in 0..depth {
                    let price = Decimal::from(100u64 + i as u64);
                    eng.process_order(make_limit(Side::Ask, price, dec!(1), i as u64 + 1))
                        .expect("insert");
                }
                let sweep_qty = Decimal::from(depth as u64);
                let result = eng
                    .process_order(make_market(Side::Bid, sweep_qty, depth as u64 + 1))
                    .expect("sweep");
                black_box(result);
            });
        });
    }
    group.finish();
}

/// Partial fill: taker quantity less than resting maker quantity.
fn bench_partial_fill(c: &mut Criterion) {
    c.bench_function("partial_fill", |b| {
        b.iter(|| {
            let mut eng = MatchingEngine::new("SOL-USDC");
            eng.process_order(make_limit(Side::Ask, dec!(100), dec!(100), 1))
                .expect("insert ask");
            let result = eng
                .process_order(make_limit(Side::Bid, dec!(100), dec!(1), 2))
                .expect("partial fill");
            black_box(result);
        });
    });
}

criterion_group!(
    benches,
    bench_single_limit_match,
    bench_market_sweep,
    bench_partial_fill,
);
criterion_main!(benches);
