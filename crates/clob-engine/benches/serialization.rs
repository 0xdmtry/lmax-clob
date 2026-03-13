use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use clob_engine::{
    order::{Order, OrderStatus},
    orderbook::OrderBook,
    snapshot::{deserialize_book, serialize_book},
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

pub fn build_book(n_bids: usize, n_asks: usize) -> OrderBook {
    let mut book = OrderBook::new();
    for i in 0..n_bids {
        let price = Decimal::from(100u64 - i as u64 % 50);
        book.insert_order(make_limit(Side::Bid, price, dec!(1), i as u64))
            .expect("insert bid");
    }
    for i in 0..n_asks {
        let price = Decimal::from(101u64 + i as u64 % 50);
        book.insert_order(make_limit(Side::Ask, price, dec!(1), (n_bids + i) as u64))
            .expect("insert ask");
    }
    book
}

fn bench_serialize(c: &mut Criterion) {
    let sizes = [10usize, 100, 500, 1000];
    let mut group = c.benchmark_group("snapshot_serialize");

    for &size in &sizes {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let book = build_book(size / 2, size / 2);
            b.iter(|| {
                let bytes = serialize_book(&book, "SOL-USDC", 0).expect("serialize");
                black_box(bytes);
            });
        });
    }
    group.finish();
}

fn bench_deserialize(c: &mut Criterion) {
    let sizes = [10usize, 100, 500, 1000];
    let mut group = c.benchmark_group("snapshot_deserialize");

    for &size in &sizes {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let book = build_book(size / 2, size / 2);
            let bytes = serialize_book(&book, "SOL-USDC", 0).expect("serialize");
            b.iter(|| {
                let result = deserialize_book(black_box(&bytes)).expect("deserialize");
                black_box(result);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_serialize, bench_deserialize);
criterion_main!(benches);
