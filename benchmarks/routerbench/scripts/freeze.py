#!/usr/bin/env python3
"""freeze.py — Day 3 冻结：Core 名单抽样 + 六 hash + frozen_manifest.json。

输入（全部已存在，只读）：
  normalized/instances.jsonl + outcomes.jsonl          冻结矩阵（Day 1 导入）
  normalized/source_manifest.json                       切分/评分/prompt 口径真源
  manifests/model_pool_public.yaml                      决策 3a
  manifests/model_pool_native.yaml                      决策 2a（结构冻结，价格 provisional）
  manifests/datasets.yaml                               决策 4a（Core 5 × 200）
  manifests/task_mapping.yaml                           映射 adapter
  reports/screening.json + screening_poolB.json         a/b 双池敏感性证据

输出：
  manifests/core_selection.jsonl    Core 抽样名单（dataset_id + sample_id，均 test split）
  manifests/frozen_manifest.json    benchmark_id + 六 hash + manifest_hash + 敏感性结论

六 hash 口径（11 号 §三 Day3：benchmark/pool/prompt_profile/scorer/split/cost_basis）：
  benchmark_hash      = sha256(instances ‖ outcomes ‖ core_selection 三文件字节串联)
  pool_hash           = sha256(model_pool_public.yaml)
  prompt_profile_hash = sha256(canon(source_manifest.prompt_profile ‖ {"public_core_selection": policy}))
  scorer_hash         = sha256(canon(source_manifest.scoring))
  split_hash          = sha256(canon(source_manifest.split ‖ core 抽样策略))
  cost_basis_hash     = sha256(canon({"public": "source", "native": pool_native.cost_basis}))
  manifest_hash       = sha256(六 hash 规范化串联)——总纲，任一输入变化即变。

抽样规则（datasets.yaml selection_policy）：每 dataset 的 test 实例按
sha256("lloom-core-select:{seed}:{sample_id}") 升序取前 200——确定性、无随机模块、可复现；
任何名单/比例/policy 变更 = 新 benchmark_id（10 号 §6.2）。

用法（仓库根目录）：benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/freeze.py
"""

from __future__ import annotations

import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path("benchmarks/routerbench")
NORM = ROOT / "normalized"
MANIFESTS = ROOT / "manifests"
REPORTS = ROOT / "reports"
SEED = 3407
PER_DATASET = 200
CORE_DATASET_IDS = [
    "rb0shot_winogrande_v1",
    "rb0shot_mmlu_v1",
    "rb0shot_hellaswag_v1",
    "rb0shot_gsm8k_v1",
    "rb0shot_mbpp_v1",
]


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canon(obj) -> bytes:
    """规范化 JSON 字节：键排序 + 紧凑分隔符，保证同语义同 hash。"""
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def selection_key(sample_id: str) -> bytes:
    return hashlib.sha256(f"lloom-core-select:{SEED}:{sample_id}".encode("utf-8")).digest()


def main() -> None:
    src_manifest = json.loads((NORM / "source_manifest.json").read_text(encoding="utf-8"))

    # ── Core 抽样：每 dataset 的 test 实例按确定性哈希升序取前 200 ──
    by_dataset: dict[str, list[str]] = {ds: [] for ds in CORE_DATASET_IDS}
    with (NORM / "instances.jsonl").open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            inst = json.loads(line)
            ds = inst["dataset_id"]
            if ds in by_dataset and inst.get("split") == "test":
                by_dataset[ds].append(inst["sample_id"])

    selection: list[dict] = []
    core_stats: dict[str, dict] = {}
    for ds in CORE_DATASET_IDS:
        ids = sorted(by_dataset[ds], key=selection_key)
        if len(ids) < PER_DATASET:
            raise SystemExit(f"✗ {ds} test 实例仅 {len(ids)} < {PER_DATASET}，Core 抽样不足")
        chosen = sorted(ids[:PER_DATASET])  # 名单落盘按 sample_id 排序，便于 diff
        selection.extend({"dataset_id": ds, "sample_id": sid, "split": "test"} for sid in chosen)
        core_stats[ds] = {"available": len(ids), "selected": len(chosen)}
    selection.sort(key=lambda r: (r["dataset_id"], r["sample_id"]))

    sel_path = MANIFESTS / "core_selection.jsonl"
    with sel_path.open("w", encoding="utf-8") as f:
        for row in selection:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")

    # ── 六 hash ──
    benchmark_hash = sha256_bytes(
        (NORM / "instances.jsonl").read_bytes()
        + (NORM / "outcomes.jsonl").read_bytes()
        + sel_path.read_bytes()
    )
    pool_hash = sha256_file(MANIFESTS / "model_pool_public.yaml")
    selection_policy = (
        f"per-dataset test 抽 {PER_DATASET}：sha256('lloom-core-select:{SEED}:{{sample_id}}') 升序前 N"
    )
    prompt_profile_hash = sha256_bytes(
        canon(src_manifest["prompt_profile"] | {"public_core_selection": selection_policy})
    )
    scorer_hash = sha256_bytes(canon(src_manifest["scoring"]))
    split_hash = sha256_bytes(
        canon(src_manifest["split"] | {"core_selection": selection_policy})
    )
    cost_basis_hash = sha256_bytes(
        canon({"public": "source", "native_cost_basis": "provider_official_usd_per_1m_tokens"})
    )
    hashes = {
        "benchmark_hash": benchmark_hash,
        "pool_hash": pool_hash,
        "prompt_profile_hash": prompt_profile_hash,
        "scorer_hash": scorer_hash,
        "split_hash": split_hash,
        "cost_basis_hash": cost_basis_hash,
    }
    manifest_hash = sha256_bytes(canon(hashes))

    # ── 敏感性结论（a/b 双池 rank 对比，来自 reports/）──
    pool_a = json.loads((REPORTS / "screening.json").read_text(encoding="utf-8"))
    pool_b = json.loads((REPORTS / "screening_poolB.json").read_text(encoding="utf-8"))
    rank_a = {r["dataset"]: i + 1 for i, r in enumerate(pool_a["datasets"])}
    rank_b = {r["dataset"]: i + 1 for i, r in enumerate(pool_b["datasets"])}
    sensitivity = {
        "pool_a": {"models": pool_a["pool"], "rank": rank_a},
        "pool_b": {"models": pool_b["pool"], "rank": rank_b},
        "finding": (
            "rank 对 mid 强度敏感（screening_score 的 oracle_gain 依赖池），两池 top4 仅两席重合；"
            "但 Core 5 名单在规格约束下唯一：mt_bench 被 10 号 §1.3 排除出 Core 排序、arc 两池垫底、"
            "gsm8k/mbpp 有 task_type 覆盖与 upgrade_opportunity 的独立入选理由。决策 4a 不被推翻。"
        ),
        "conclusion": "按 pool_a（决策 3a）冻结；rank 敏感性记录在案，解读 headline 时不得跨池引用 screening rank",
    }

    frozen = {
        "benchmark_id": "lloom-routerbench-v0.1",
        "track_scope": "public_screening",  # Native Track Day 6 另出 native frozen manifest
        "frozen_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "schema_version": 1,
        "hashes": hashes,
        "manifest_hash": manifest_hash,
        "hash_notes": "六 hash 任一变化 = 新 benchmark_id（10 号 §6.2）；benchmark_hash 覆盖矩阵本体+Core 名单",
        "inputs": {
            "raw_sources": src_manifest["source"]["files"],
            "normalized": {
                "instances_sha256": sha256_file(NORM / "instances.jsonl"),
                "outcomes_sha256": sha256_file(NORM / "outcomes.jsonl"),
            },
            "manifests": {
                name: sha256_file(MANIFESTS / name)
                for name in ("model_pool_public.yaml", "model_pool_native.yaml",
                             "datasets.yaml", "task_mapping.yaml")
            },
            "screening_reports": {
                name: sha256_file(REPORTS / name)
                for name in ("screening.json", "screening_poolB.json")
            },
        },
        "core": {
            "per_dataset": PER_DATASET,
            "split": "test",
            "seed": SEED,
            "stats": core_stats,
            "total": len(selection),
            "selection_file": str(sel_path),
        },
        "pool_sensitivity": sensitivity,
        "decisions": {
            "P1_bench_db": "独立 benchmarks/routerbench/bench.db（生产库零污染；.gitignore *.db 兜底）",
            "P2_price": "native 价格维持转述值冻结，Day 6 自采前从官方页重录 diff，变化即新 benchmark_version",
            "P3_pool": "public 池 = mistral-7b / Yi-34B / GPT-4-1106（决策 3a）",
            "P4_core": "Core 5 = winogrande/mmlu/hellaswag/gsm8k/mbpp × 200（决策 4a）",
        },
        "next": "Day 4 bench.rs（复用 production plan()/score_all()）+ Plan parity test",
    }
    frozen_path = MANIFESTS / "frozen_manifest.json"
    frozen_path.write_text(json.dumps(frozen, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ core_selection : {sel_path}  ({len(selection)} 行 = {len(CORE_DATASET_IDS)} × {PER_DATASET})")
    print(f"✓ frozen_manifest: {frozen_path}")
    print(f"  benchmark_id    = {frozen['benchmark_id']}（track: {frozen['track_scope']}）")
    print(f"  manifest_hash   = {manifest_hash}")
    for k, v in hashes.items():
        print(f"  {k:<20}= {v[:16]}…")
    for ds, st in core_stats.items():
        print(f"  {ds:<32} test 可用 {st['available']:>5} → 抽 {st['selected']}")


if __name__ == "__main__":
    main()
