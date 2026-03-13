.PHONY: up down build bench load-baseline load-stress load-soak load-oha migrate reset-db db-stats logs console profile-cpu profile-heap

up:
	docker compose up -d

down:
	docker compose down -v

logs:
	docker compose logs -f clob-api

build:
	cargo build --release

bench:
	cargo criterion

bench-save:
	cargo criterion --message-format=json | tee target/bench-$(shell date +%Y%m%d-%H%M%S).json

profile-cpu:
	cargo build --profile profiling --bin clob-api
	samply record ./target/profiling/clob-api

profile-heap:
	cargo bench --bench order_matching --features dhat-heap -- --profile-time=10

load-baseline:
	docker compose --profile load run k6 run /scripts/baseline.js

load-stress:
	docker compose --profile load run k6 run /scripts/stress.js

load-soak:
	docker compose --profile load run k6 run /scripts/soak.js

load-oha:
	oha -n 100000 -c 100 http://localhost:8000/health

migrate:
	cargo sqlx migrate run --database-url "postgres://clob:clob@localhost:5432/clob"

reset-db:
	docker compose stop postgres
	docker compose rm -f postgres
	docker volume rm clob-lab_postgres-data || true
	docker compose up -d postgres

db-stats:
	docker compose exec postgres psql -U clob -c \
		"SELECT query, calls, mean_exec_time, total_exec_time \
		 FROM pg_stat_statements \
		 ORDER BY total_exec_time DESC \
		 LIMIT 20;"

console:
	RUSTFLAGS="--cfg tokio_unstable" cargo build --features console --bin clob-api
	@echo "Run: tokio-console http://localhost:6669"

mock:
	docker compose up --build -d yellowstone-mock

topics:
	docker compose exec redpanda rpk topic create orders-in \
		--partitions 4 --replicas 1 --topic-config retention.ms=3600000 || true
	docker compose exec redpanda rpk topic create fills-out \
		--partitions 4 --replicas 1 --topic-config retention.ms=3600000 || true

bench-alloc-dhat:
	cargo bench -p clob-engine --bench alloc_profile --features dhat-heap -- --profile-time=5

bench-mimalloc:
	cargo bench -p clob-engine --bench order_matching --features mimalloc

bench-jemalloc:
	cargo bench -p clob-engine --bench order_matching --features jemalloc

bench-system:
	cargo bench -p clob-engine --bench order_matching

criterion-mimalloc:
	cargo criterion -p clob-engine --bench order_matching --features mimalloc

criterion-jemalloc:
	cargo criterion -p clob-engine --bench order_matching --features jemalloc

criterion-system:
	cargo criterion -p clob-engine --bench order_matching

bench-ds:
	cargo bench -p clob-engine --bench btreemap_vs_vec

criterion-ds:
	cargo criterion -p clob-engine --bench btreemap_vs_vec

load-channel-256:
	ENGINE_CHANNEL_DEPTH=256 STREAM_RATE=2000 cargo run -p clob-api &
	sleep 5 && oha -n 50000 -c 50 http://localhost:8000/health; kill %1

load-channel-1024:
	ENGINE_CHANNEL_DEPTH=1024 STREAM_RATE=2000 cargo run -p clob-api &
	sleep 5 && oha -n 50000 -c 50 http://localhost:8000/health; kill %1

load-channel-8192:
	ENGINE_CHANNEL_DEPTH=8192 STREAM_RATE=2000 cargo run -p clob-api &
	sleep 5 && oha -n 50000 -c 50 http://localhost:8000/health; kill %1

load-channel-32768:
	ENGINE_CHANNEL_DEPTH=32768 STREAM_RATE=2000 cargo run -p clob-api &
	sleep 5 && oha -n 50000 -c 50 http://localhost:8000/health; kill %1

load-kafka-linger-0:
	KAFKA_LINGER_MS=0 KAFKA_BATCH_NUM_MESSAGES=1 cargo run -p clob-api

load-kafka-linger-5:
	KAFKA_LINGER_MS=5 KAFKA_BATCH_NUM_MESSAGES=1000 cargo run -p clob-api

load-kafka-linger-50:
	KAFKA_LINGER_MS=50 KAFKA_BATCH_NUM_MESSAGES=5000 cargo run -p clob-api

load-kafka-linger-100:
	KAFKA_LINGER_MS=100 KAFKA_BATCH_NUM_MESSAGES=10000 cargo run -p clob-api

load-db-batch-10:
	DB_BATCH_SIZE=10 DB_FLUSH_MS=10 cargo run -p clob-api

load-db-batch-100:
	DB_BATCH_SIZE=100 DB_FLUSH_MS=25 cargo run -p clob-api

load-db-batch-500:
	DB_BATCH_SIZE=500 DB_FLUSH_MS=50 cargo run -p clob-api

load-db-batch-1000:
	DB_BATCH_SIZE=1000 DB_FLUSH_MS=100 cargo run -p clob-api

load-snapshot-100:
	SNAPSHOT_EVERY=100 cargo run -p clob-api

load-snapshot-1000:
	SNAPSHOT_EVERY=1000 cargo run -p clob-api

load-snapshot-10000:
	SNAPSHOT_EVERY=10000 cargo run -p clob-api

load-snapshot-100000:
	SNAPSHOT_EVERY=100000 cargo run -p clob-api