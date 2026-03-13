//! Compares the existing BTreeMap-based OrderBook against a Vec<(Price, Level)>
//! sorted variant for: insert, cancel, best_bid/ask, and full match cycle.
//!
//! Run:
//!   cargo criterion -p clob-engine --bench btreemap_vs_vec
//!   cargo bench    -p clob-engine --bench btreemap_vs_vec

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::VecDeque;
use uuid::Uuid;

use clob_engine::{
    matching::MatchingEngine,
    order::{Order, OrderStatus},
    orderbook::OrderBook,
    types::{OrderType, Side},
};

// ── Vec+binary search book (self-contained, no production code changes) ──────

type SlabKey = usize;
type Level = VecDeque<SlabKey>;

struct VecBook {
    orders: slab::Slab<Order>,
    index: std::collections::HashMap<uuid::Uuid, usize>,
    bids: Vec<(Decimal, Level)>, // sorted ascending; best = last
    asks: Vec<(Decimal, Level)>, // sorted ascending; best = first
}

impl VecBook {
    fn new() -> Self {
        Self {
            orders: slab::Slab::new(),
            index: std::collections::HashMap::new(),
            bids: Vec::new(),
            asks: Vec::new(),
        }
    }

    fn insert(&mut self, order: Order) {
        let price = match order.price {
            Some(p) => p,
            None => return,
        };
        let id = order.id;
        let key = self.orders.insert(order);
        self.index.insert(id, key);

        let levels = match self.orders[key].side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        match levels.binary_search_by(|(p, _)| p.cmp(&price)) {
            Ok(i) => levels[i].1.push_back(key),
            Err(i) => {
                let mut q = VecDeque::new();
                q.push_back(key);
                levels.insert(i, (price, q));
            }
        }
    }

    fn cancel(&mut self, id: uuid::Uuid) {
        let key = match self.index.remove(&id) {
            Some(k) => k,
            None => return,
        };
        let order = match self.orders.try_remove(key) {
            Some(o) => o,
            None => return,
        };
        let price = match order.price {
            Some(p) => p,
            None => return,
        };
        let levels = match order.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        if let Ok(i) = levels.binary_search_by(|(p, _)| p.cmp(&price)) {
            levels[i].1.retain(|&k| k != key);
            if levels[i].1.is_empty() {
                levels.remove(i);
            }
        }
    }

    fn best_bid(&self) -> Option<Decimal> {
        self.bids.last().map(|(p, _)| *p)
    }

    fn best_ask(&self) -> Option<Decimal> {
        self.asks.first().map(|(p, _)| *p)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_limit(side: Side, price: Decimal, qty: Decimal, seq: u64) -> Order {
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

fn make_market(side: Side, qty: Decimal, seq: u64) -> Order {
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

// ── Insert ────────────────────────────────────────────────────────────────────

fn bench_insert(c: &mut Criterion) {
    let depths = [50usize, 200, 1000];
    let mut group = c.benchmark_group("insert/btreemap");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| {
                let mut book = OrderBook::new();
                for i in 0..depth {
                    book.insert_order(make_limit(
                        Side::Ask,
                        Decimal::from(100u64 + i as u64),
                        dec!(1),
                        i as u64,
                    ))
                    .ok();
                }
                black_box(&book);
            });
        });
    }
    group.finish();

    let mut group = c.benchmark_group("insert/vec");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| {
                let mut book = VecBook::new();
                for i in 0..depth {
                    book.insert(make_limit(
                        Side::Ask,
                        Decimal::from(100u64 + i as u64),
                        dec!(1),
                        i as u64,
                    ));
                }
                black_box(&book.asks);
            });
        });
    }
    group.finish();
}

// ── Cancel ────────────────────────────────────────────────────────────────────

fn bench_cancel(c: &mut Criterion) {
    let depths = [50usize, 200, 1000];

    let mut group = c.benchmark_group("cancel/btreemap");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter_batched(
                || {
                    let mut book = OrderBook::new();
                    let mut ids = Vec::with_capacity(depth);
                    for i in 0..depth {
                        let o = make_limit(
                            Side::Ask,
                            Decimal::from(100u64 + i as u64),
                            dec!(1),
                            i as u64,
                        );
                        ids.push(o.id);
                        book.insert_order(o).ok();
                    }
                    (book, ids)
                },
                |(mut book, ids)| {
                    for id in ids {
                        book.cancel_order(id).ok();
                    }
                    black_box(book);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();

    let mut group = c.benchmark_group("cancel/vec");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter_batched(
                || {
                    let mut book = VecBook::new();
                    let mut ids = Vec::with_capacity(depth);
                    for i in 0..depth {
                        let o = make_limit(
                            Side::Ask,
                            Decimal::from(100u64 + i as u64),
                            dec!(1),
                            i as u64,
                        );
                        ids.push(o.id);
                        book.insert(o);
                    }
                    (book, ids)
                },
                |(mut book, ids)| {
                    for id in ids {
                        book.cancel(id);
                    }
                    black_box(book.asks);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ── best_bid / best_ask ───────────────────────────────────────────────────────

fn bench_best(c: &mut Criterion) {
    let depths = [50usize, 200, 1000];

    let mut group = c.benchmark_group("best_bid_ask/btreemap");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            let mut book = OrderBook::new();
            for i in 0..depth {
                book.insert_order(make_limit(
                    Side::Bid,
                    Decimal::from(100u64 + i as u64),
                    dec!(1),
                    i as u64 * 2,
                ))
                .ok();
                book.insert_order(make_limit(
                    Side::Ask,
                    Decimal::from(200u64 + i as u64),
                    dec!(1),
                    i as u64 * 2 + 1,
                ))
                .ok();
            }
            b.iter(|| {
                black_box(book.best_bid());
                black_box(book.best_ask());
            });
        });
    }
    group.finish();

    let mut group = c.benchmark_group("best_bid_ask/vec");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            let mut book = VecBook::new();
            for i in 0..depth {
                book.insert(make_limit(
                    Side::Bid,
                    Decimal::from(100u64 + i as u64),
                    dec!(1),
                    i as u64 * 2,
                ));
                book.insert(make_limit(
                    Side::Ask,
                    Decimal::from(200u64 + i as u64),
                    dec!(1),
                    i as u64 * 2 + 1,
                ));
            }
            b.iter(|| {
                black_box(book.best_bid());
                black_box(book.best_ask());
            });
        });
    }
    group.finish();
}

// ── Full match cycle (BTreeMap only — production engine) ─────────────────────
// Vec book does not implement the full matching loop.
// The match cycle benchmark uses the existing MatchingEngine to measure
// end-to-end cost at realistic resting book depths before the taker arrives.

fn bench_match_cycle(c: &mut Criterion) {
    let depths = [50usize, 200, 1000];
    let mut group = c.benchmark_group("match_cycle/btreemap");
    for &depth in &depths {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter_batched(
                || {
                    let mut eng = MatchingEngine::new("SOL-USDC");
                    // Fill resting ask side at depth distinct price levels.
                    for i in 0..depth {
                        eng.process_order(make_limit(
                            Side::Ask,
                            Decimal::from(101u64 + i as u64),
                            dec!(1),
                            i as u64 + 1,
                        ))
                        .ok();
                    }
                    eng
                },
                |mut eng| {
                    // Single aggressive bid that matches only the best ask.
                    let result = eng.process_order(make_limit(
                        Side::Bid,
                        Decimal::from(101u64),
                        dec!(1),
                        999_999,
                    ));
                    black_box(result);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_cancel,
    bench_best,
    bench_match_cycle
);
criterion_main!(benches);
