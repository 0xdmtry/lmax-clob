#!/usr/bin/env bash
set -euo pipefail

BROKERS="${KAFKA_BROKERS:-localhost:19092}"

rpk topic create orders-in \
  --brokers "$BROKERS" \
  --partitions 4 \
  --replicas 1 \
  --topic-config retention.ms=3600000 \
  || true