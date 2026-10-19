# Benchmark reproducibility contract

This directory contains three deliberately different evaluation tracks:

| Track | Provider calls | What it proves | Current status |
|---|---:|---|---|
| **Replay** | 0 | Frozen reports, figure data, audit files and hashes are internally consistent | Available now |
| **Live** | Yes | A configured provider/model profile produces fresh outcomes | Requires data, credentials and budget |
| **E2E** | Yes | Real workflow execution, artifacts, tools, wall-clock latency and recovery | Not implemented in this snapshot |

## Reproduce the published snapshot

From the repository root:

```bash
make -f benchmarks/Makefile doctor
make -f benchmarks/Makefile replay
```

Both commands are stdlib-only and must work without `/Volumes/Files`. The
replay command must not require an API key or make a network request. A missing
external raw-data symlink is a warning in replay mode because published
showcase numbers come from frozen files in the repository.

To inspect whether a machine is suitable for a new collection run:

```bash
python3 benchmarks/tools/doctor.py --mode live --strict
```

Live collection is intentionally not part of the default verification path.
Before collecting, record the provider, model version, endpoint, prompt,
temperature, thinking/max-token settings, judge version, budget limit and
stopping condition in the run manifest. Do not use a live run to silently
replace a frozen snapshot.

## Interpretation rules

The machine-readable rules are in `benchmark_contract.json` and the detailed
claims are in `showcase/claims_and_limits.md`. In particular:

1. **Best Single is a required comparator for P2.** It currently dominates P2
   on the reported point estimates; routing must not be described as globally
   optimal.
2. SystemBench v0.1 replays frozen outcomes and uses simulated duration. It is
   not an end-to-end agent success or production wall-clock benchmark.
3. S12 semantic data is exploratory, mixed-protocol and partly incomplete.
   It is not eligible for a pooled headline leaderboard.
4. The current showcase is not a formal SystemBench v0.2 release.

## Data tiers

- **Minimal replay package:** tracked reports, matrices, CSVs, figures and
  manifests; enough for the offline showcase audit.
- **Raw collection package:** external raw data and `bench.db`; needed only to
  rebuild normalized data or run new collection.
- **Live environment:** provider credentials and an explicitly versioned run
  configuration; never commit credentials or generated secrets.
