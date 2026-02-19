#!/usr/bin/env bash
diff "$1" <(cargo run --bin org2jsonl -- "$1" | cargo run --bin jsonl2org)
