# Solver baseline

Generate and run a deterministic, synthetic benchmark:

```sh
python3 benchmarks/generate_synthetic.py > /tmp/alina-school.json
cargo build --release -p alina-cli
/usr/bin/time -p target/release/alina-cli solve-chronexa /tmp/alina-school.json /tmp/alina-result.json
target/release/alina-cli verify /tmp/alina-result.json
```

The default fixture has 30 independent classes and teachers, four one-period cards each, and no rooms or advanced constraints. On the developer's Mac on 2026-11-19, commit `089cbf0` placed 120/120 cards in 4.72 seconds (121 search nodes); independent verification found zero supported hard violations. These are single-run observations, not a performance guarantee. This easy fixture does not measure realistic contention, the full constraint set, or parity with Chronexa. Do not use private school data as a committed benchmark.
