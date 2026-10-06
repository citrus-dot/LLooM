#!/usr/bin/env python3
"""freeze_v02.py — Day 11 冻结：R7_stability + P2 → benchmark_id `lloom-routerbench-v0.2`。

冻结内容（16-v3 §十四，全部 calibration 侧产物，test 只读未触碰）：
  Best Pool   = R7_stability 冠军池（deephermes-3-8b / deepseek-r1-distill-qwen-7b / intern-s1-mini）
  Best Policy = P2 Soft Eligibility（weak nominal tier 2、penalty=0、生产 default 权重、
                质量先验=calibration 全局均分、hit_rate 全零、est 固定 profile）
  Test 名单   = llmrb test split 中 R7 池三模型全覆盖的实例（确定性：覆盖是矩阵属性非选择）

产出（manifests/）：
  model_pool_v02.yaml        pool_id + 三模型 + 选择依据 + calibration 指标快照
  test_selection_v02.jsonl   冻结 test 名单（dataset_id + sample_id）
  frozen_manifest_v02.json   benchmark_id + 七 hash（六 hash + policy_hash）+ 快照

任一 hash 输入变化 = 新 benchmark_id（10 号 §6.2 / 13-v2 §十八）；冻结后 test 只读。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/freeze_v02.py
"""

from __future__ import annotations

import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path("benchmarks/routerbench")
NORM = ROOT / "normalized" / "llmrouterbench"
MANIFESTS = ROOT / "manifests"
REPORTS = ROOT / "reports"

POOL = {
    "weak": "deephermes_3_llama_3_8b_preview",
    "mid": "deepseek_r1_distill_qwen_7b",
    "strong": "intern_s1_mini",
}
POLICY = {
    "family": "P2_softgate0",
    "soft_gate_penalty": 0.0,
    "weak_nominal_tier": 2,
    "weights": {"cost": 0.5, "quality": 0.4, "latency": 0.1},
    "quality_prior": "calibration global mean（注册冷启动先验）",
    "hit_rate": "all-zero",
    "est_profile": {"est_in": "chars*0.6", "est_out_cold_start": 750},
    "production_code": "router::plan() 未改动；eligibility 走内存 nominal tier + quality_override 通道",
}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def canon(obj) -> bytes:
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def main() -> int:
    src_manifest = json.loads((NORM / "source_manifest.json").read_text(encoding="utf-8"))
    tournament = json.loads((REPORTS / "pool_tournament_calibration.json").read_text(encoding="utf-8"))

    # ── 冻结 test 名单：test split × R7 池全覆盖（确定性）──
    selection: list[dict] = []
    per_dataset: dict[str, int] = {}
    with (NORM / "instances.jsonl").open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            inst = json.loads(line)
            if inst.get("split") != "test":
                continue
            # 覆盖判定需要 outcomes——与 instances 同序读取代价高，改由 outcomes 索引判断
            selection.append(inst)
    outcomes_coverage: dict[str, set[str]] = {}
    with (NORM / "outcomes.jsonl").open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            o = json.loads(line)
            outcomes_coverage.setdefault(o["sample_id"], set()).add(o["model_id"])
    frozen = []
    for inst in selection:
        sid = inst["sample_id"]
        models = outcomes_coverage.get(sid, set())
        if all(m in models for m in POOL.values()):
            frozen.append({"dataset_id": inst["dataset_id"], "sample_id": sid, "split": "test"})
            per_dataset[inst["dataset_id"]] = per_dataset.get(inst["dataset_id"], 0) + 1
    frozen.sort(key=lambda r: (r["dataset_id"], r["sample_id"]))
    if not frozen:
        raise SystemExit("✗ 冻结 test 名单为空")

    sel_path = MANIFESTS / "test_selection_v02.jsonl"
    with sel_path.open("w", encoding="utf-8") as f:
        for row in frozen:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")

    # ── pool manifest ──
    t_r7 = next(e for e in tournament["results"]
                if e["entrant"] == "R7_stability" and e["policy"] == "P2_softgate0")
    pool_yaml = f"""# model_pool_v02.yaml — Day 11 冻结（2026-10-06）
# 来源：Day 9 Calibration Tournament R7_stability 冠军（16-v3 §5.1，calibration-only 选出）
# 冻结规则：任何模型 id/tier 变更 = 新 pool_id = 新 benchmark_id；不写 production models 表
pool_id: llmrb_tournament_r7_v1
version: 1
track: public_screening
cost_basis: llmrouterbench_source
frozen_at: "2026-10-06"
selection_reason: >-
  Tournament 中唯一 P2 对 BASE Pareto 改配的池：weak share 0→9.7%、weak recall 0.115、
  weak precision 0.809、retention 100.4%、saving 12.6%（calibration，13 datasets 覆盖）。
calibration_snapshot:
  n_calibration_covered: {t_r7['n']}
  quality_mean_p2: {t_r7['quality_mean']}
  cost_mean_p2: {t_r7['cost_mean']}
  weak_recall: {t_r7['weak_recall']}
  weak_precision: {t_r7['weak_precision']}
models:
  - role: weak
    lloom_model_id: {POOL['weak']}
    capability_tier: 1
    nominal_tier_in_policy: 2   # P2 Soft Eligibility：medium 放行，hard 仍拒
  - role: mid
    lloom_model_id: {POOL['mid']}
    capability_tier: 2
  - role: strong
    lloom_model_id: {POOL['strong']}
    capability_tier: 3
"""
    pool_path = MANIFESTS / "model_pool_v02.yaml"
    pool_path.write_text(pool_yaml, encoding="utf-8")

    # ── 七 hash ──
    benchmark_hash = hashlib.sha256(
        (NORM / "instances.jsonl").read_bytes()
        + (NORM / "outcomes.jsonl").read_bytes()
        + sel_path.read_bytes()
    ).hexdigest()
    pool_hash = sha256_file(pool_path)
    selection_rule = ("llmrb test split × R7 池三模型全覆盖（覆盖是矩阵属性，非抽样）；"
                      "名单落盘 test_selection_v02.jsonl")
    policy_hash = hashlib.sha256(canon(POLICY)).hexdigest()
    split_hash = hashlib.sha256(canon(
        src_manifest["split"] | {"v02_test_selection": selection_rule})).hexdigest()
    scorer_hash = hashlib.sha256(canon(src_manifest["scoring"])).hexdigest()
    cost_basis_hash = hashlib.sha256(canon({"cost_basis": "llmrouterbench_source"})).hexdigest()
    hashes = {
        "benchmark_hash": benchmark_hash,
        "pool_hash": pool_hash,
        "policy_hash": policy_hash,
        "split_hash": split_hash,
        "scorer_hash": scorer_hash,
        "cost_basis_hash": cost_basis_hash,
    }
    manifest_hash = hashlib.sha256(canon(hashes)).hexdigest()

    frozen_manifest = {
        "benchmark_id": "lloom-routerbench-v0.2",
        "track_scope": "public_screening",
        "source_matrix": "llmrouterbench（v0.1 的 routerbench 0-shot 矩阵模型集与本池不相交，"
                         "故 v0.2 换源；成本口径分轨见 cost_basis_hash）",
        "frozen_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "schema_version": 2,
        "hashes": hashes,
        "manifest_hash": manifest_hash,
        "pool_id": "llmrb_tournament_r7_v1",
        "policy": POLICY,
        "test_selection": {
            "file": str(sel_path),
            "total": len(frozen),
            "per_dataset": dict(sorted(per_dataset.items())),
            "rule": selection_rule,
        },
        "inputs": {
            "instances_sha256": sha256_file(NORM / "instances.jsonl"),
            "outcomes_sha256": sha256_file(NORM / "outcomes.jsonl"),
            "source_tar_sha256": src_manifest["source"]["files"][0]["sha256"],
            "tournament_report_sha256": sha256_file(REPORTS / "pool_tournament_calibration.json"),
            "entrants_sha256": sha256_file(REPORTS / "tournament_entrants.json"),
        },
        "leakage_statement": (
            "池选择/policy 选择全部发生在 calibration split（Day 9 tournament）；"
            "test split 在本 manifest 冻结前未被任何策略读取（Day 6-9 全程 calibration-only）"
        ),
        "next": "Day 12：冻结配置 test 一次性 replay + matched random + budget curve（test 只读）",
    }
    frozen_path = MANIFESTS / "frozen_manifest_v02.json"
    frozen_path.write_text(json.dumps(frozen_manifest, ensure_ascii=False, indent=2) + "\n",
                           encoding="utf-8")

    print(f"✓ test_selection_v02.jsonl : {len(frozen)} 实例（{len(per_dataset)} datasets）")
    print(f"✓ model_pool_v02.yaml      : pool_id=llmrb_tournament_r7_v1")
    print(f"✓ frozen_manifest_v02.json : benchmark_id=lloom-routerbench-v0.2")
    print(f"  manifest_hash = {manifest_hash}")
    for k, v in hashes.items():
        print(f"  {k:<18}= {v[:16]}…")
    return 0


if __name__ == "__main__":
    sys.exit(main())
