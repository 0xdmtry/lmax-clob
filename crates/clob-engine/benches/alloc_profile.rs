// Heap allocation profile for the matching engine hot path.
// Run with:
//   cargo bench -p clob-engine --bench alloc_profile --features dhat-heap -- --profile-time=5
//
// dhat writes dhat-heap.json to the working directory.
// Visualise at https://nnethercote.github.io/dh_view/dh_view.html

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
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

fn bench_alloc_single_match(c: &mut Criterion) {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    c.bench_function("alloc_single_match", |b| {
        b.iter(|| {
            let mut eng = MatchingEngine::new("SOL-USDC");
            eng.process_order(make_limit(Side::Ask, dec!(100), dec!(10), 1))
                .ok();
            let result = eng
                .process_order(make_limit(Side::Bid, dec!(100), dec!(10), 2))
                .ok();
            black_box(result);
        });
    });
}

fn bench_alloc_market_sweep_50(c: &mut Criterion) {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    c.bench_function("alloc_market_sweep_50", |b| {
        b.iter(|| {
            let mut eng = MatchingEngine::new("SOL-USDC");
            for i in 0..50u64 {
                let price = Decimal::from(100 + i);
                eng.process_order(make_limit(Side::Ask, price, dec!(1), i + 1))
                    .ok();
            }
            let result = eng
                .process_order(Order {
                    id: Uuid::new_v4(),
                    market_id: "SOL-USDC".to_string(),
                    side: Side::Bid,
                    order_type: OrderType::Market,
                    price: None,
                    quantity: dec!(50),
                    filled: dec!(0),
                    status: OrderStatus::Open,
                    sequence: 51,
                })
                .ok();
            black_box(result);
        });
    });
}

criterion_group!(
    benches,
    bench_alloc_single_match,
    bench_alloc_market_sweep_50
);
criterion_main!(benches);
