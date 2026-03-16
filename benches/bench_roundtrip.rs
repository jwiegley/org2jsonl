use criterion::{black_box, criterion_group, criterion_main, Criterion};
use org2jsonl::json_to_org::entries_to_org;
use org2jsonl::org_to_json::org_to_entries;

const SIMPLE: &str = "\
* TODO Buy groceries
SCHEDULED: <2025-01-15>
- [ ] Milk
- [X] Eggs
";

const COMPLEX: &str = include_str!("../tests/fixtures/full_document.org");

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    group.bench_function("simple", |b| {
        b.iter(|| org_to_entries(black_box(SIMPLE)));
    });
    group.bench_function("complex", |b| {
        b.iter(|| org_to_entries(black_box(COMPLEX)));
    });
    group.finish();
}

fn bench_write(c: &mut Criterion) {
    let simple_entries = org_to_entries(SIMPLE);
    let complex_entries = org_to_entries(COMPLEX);

    let mut group = c.benchmark_group("write");
    group.bench_function("simple", |b| {
        b.iter(|| entries_to_org(black_box(&simple_entries)));
    });
    group.bench_function("complex", |b| {
        b.iter(|| entries_to_org(black_box(&complex_entries)));
    });
    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");
    group.bench_function("simple", |b| {
        b.iter(|| {
            let entries = org_to_entries(black_box(SIMPLE));
            entries_to_org(&entries)
        });
    });
    group.bench_function("complex", |b| {
        b.iter(|| {
            let entries = org_to_entries(black_box(COMPLEX));
            entries_to_org(&entries)
        });
    });
    group.finish();
}

fn bench_json_serde(c: &mut Criterion) {
    let entries = org_to_entries(COMPLEX);
    let jsonl: String = entries
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    let mut group = c.benchmark_group("json_serde");
    group.bench_function("serialize", |b| {
        b.iter(|| {
            for entry in black_box(&entries) {
                let _ = serde_json::to_string(entry).unwrap();
            }
        });
    });
    group.bench_function("deserialize", |b| {
        b.iter(|| {
            for line in black_box(&jsonl).lines() {
                let _: org2jsonl::model::OrgEntry = serde_json::from_str(line).unwrap();
            }
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_write,
    bench_roundtrip,
    bench_json_serde
);
criterion_main!(benches);
