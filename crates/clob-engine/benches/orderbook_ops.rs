use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use clob_engine::{
    order::{Order, OrderStatus},
    orderbook::OrderBook,
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

/// Insert N orders at distinct price levels.
fn bench_insert(c: &mut Criterion) {
    let depths = [10usize, 100, 500, 1000];
    let mut group = c.benchmark_group("insert_orders");

    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| {
                let mut book = OrderBook::new();
                for i in 0..depth {
                    let price = Decimal::from(100u64 + i as u64);
                    book.insert_order(make_limit(Side::Ask, price, dec!(1), i as u64))
                        .expect("insert");
                }
                black_box(&book);
            });
        });
    }
    group.finish();
}

/// Cancel N orders after inserting them.
fn bench_cancel(c: &mut Criterion) {
    let depths = [10usize, 100, 500];
    let mut group = c.benchmark_group("cancel_orders");

    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter_batched(
                || {
                    let mut book = OrderBook::new();
                    let mut ids = Vec::with_capacity(depth);
                    for i in 0..depth {
                        let price = Decimal::from(100u64 + i as u64);
                        let order = make_limit(Side::Ask, price, dec!(1), i as u64);
                        ids.push(order.id);
                        book.insert_order(order).expect("insert");
                    }
                    (book, ids)
                },
                |(mut book, ids)| {
                    for id in ids {
                        book.cancel_order(id).expect("cancel");
                    }
                    black_box(book);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// best_bid / best_ask at various book depths.
fn bench_best_bid_ask(c: &mut Criterion) {
    let depths = [10usize, 100, 500, 1000];
    let mut group = c.benchmark_group("best_bid_ask");

    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            let mut book = OrderBook::new();
            for i in 0..depth {
                let bp = Decimal::from(100u64 + i as u64);
                let ap = Decimal::from(200u64 + i as u64);
                book.insert_order(make_limit(Side::Bid, bp, dec!(1), i as u64 * 2))
                    .expect("insert bid");
                book.insert_order(make_limit(Side::Ask, ap, dec!(1), i as u64 * 2 + 1))
                    .expect("insert ask");
            }
            b.iter(|| {
                black_box(book.best_bid());
                black_box(book.best_ask());
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_cancel, bench_best_bid_ask);
criterion_main!(benches);
