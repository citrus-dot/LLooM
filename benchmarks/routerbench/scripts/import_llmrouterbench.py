#!/usr/bin/env python3
"""import_llmrouterbench.py — LLMRouterBench release → canonical frozen matrix（Day 6）。

实测结构（2026-10-06，27 dataset 目录 / 700 JSON / 6.5GB）：
  bench-release/<dataset>[/setting]/<model>/<ds>-<setting>-<model>-<ts>.json
  - 目录深度不齐（arenahard 无 setting 层）→ 以 JSON 内 dataset_name/split/model_name 为权威
  - 顶层聚合：performance/time_taken/tokens/cost/counts/data_fingerprint
  - records[]: index/origin_query/prompt/prompt_tokens/completion_tokens/cost/score/
               prediction/ground_truth/raw_output

输出（10 号 §6.1 + 13-v2 §6 统一 canonical）：
  normalized/llmrouterbench/instances.jsonl   每 query 一行
  normalized/llmrouterbench/outcomes.jsonl    每 query×model 一行（raw/normalized 双轨 + 溯源）
  normalized/llmrouterbench/source_manifest.json

设计约定：
- **raw_score/normalized_score、raw_cost/normalized_cost 双轨**（13-v2 §6.2 严禁覆盖 source）；
  v1 归一化策略 normalized=raw（score 原样保留源量纲，screening 按连续值处理）；
- **raw_output 不复制**：outcomes 记 source_file+record_index 溯源，原文留在 sha256 门控的 raw 归档
  （避免 6.5GB 重复落盘；审计走 raw/llmrouterbench/bench-release/…）；
- split=官方边界（split 字段原样入 dataset_id），内部 calibration/test 用哈希桶 20/80 seed=3407
  （与 routerbench 导入器同方法，改 seed/比例 = 新 benchmark_id）；
- Mimosa 安全：纯 JSON 解析、无 pickle/random/网络。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/import_llmrouterbench.py
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

TAR_SHA256 = "b79f8cde1a6f029c2efa663a3a3b6f7748defb22341fe59f328cebef6648c8f1"
SEED = 3407
CALIBRATION_RATIO = 0.2

# dataset → (task_family, task_type)：与 task_mapping.yaml 对齐，未收录兜底 other/general
FAMILY_META: dict[str, tuple[str, str]] = {
    "aime": ("mathematics", "math_logic"),
    "livemathbench": ("mathematics", "math_logic"),
    "math500": ("mathematics", "math_logic"),
    "mathbench": ("mathematics", "math_logic"),
    "korbench": ("mathematics", "math_logic"),
    "mbpp": ("coding", "coding"),
    "humaneval": ("coding", "coding"),
    "livecodebench": ("coding", "coding"),
    "swe-bench": ("coding", "coding"),
    "mmlupro": ("knowledge", "general"),
    "simpleqa": ("knowledge", "general"),
    "finqa": ("finance_qa", "general"),
    "medqa": ("medical_qa", "general"),
    "gpqa": ("hard_reasoning", "complex_reasoning"),
    "hle": ("hard_reasoning", "complex_reasoning"),
    "bbh": ("logic", "complex_reasoning"),
    "winogrande": ("commonsense", "general"),
    "arcc": ("knowledge", "general"),
    "arc-agi": ("puzzle", "general"),
    "kandk": ("logic", "general"),
    "emorynlp": ("emotion", "general"),
    "meld": ("emotion", "general"),
    "arenahard": ("dialogue", "general"),
    "arenahard_coding": ("dialogue", "general"),
    "arenahard_math": ("dialogue", "general"),
    "arenahard_creative_writing": ("dialogue", "general"),
    "tau2": ("tool_use", "general"),
}
FALLBACK_META: tuple[str, str] = ("other", "general")
SCORER_NOTE = {
    "accuracy": ["aime", "livemathbench", "math500", "mathbench", "korbench", "mbpp", "humaneval",
                 "livecodebench", "swe-bench", "mmlupro", "simpleqa", "finqa", "medqa", "gpqa",
                 "winogrande", "arcc", "emorynlp", "meld"],
    "llm_judge": ["hle", "arenahard", "arenahard_coding", "arenahard_math", "arenahard_creative_writing"],
    "other": ["tau2", "bbh", "kandk", "arc-agi"],
}


def slugify(name: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "_", str(name).strip().lower()).strip("_")
    return s or "unknown"


def split_of(sample_id: str, seed: int, ratio: float) -> str:
    digest = hashlib.sha256(f"lloom-split:{seed}:{sample_id}".encode("utf-8")).digest()
    return "calibration" if int.from_bytes(digest[:16], "big") / 2**128 < ratio else "test"


def iter_result_files(root: Path):
    """遍历 dataset 目录收集 (dataset_dir_name, json_path)；跳过点开头目录与非结果 JSON。"""
    for ds_dir in sorted(root.iterdir()):
        if not ds_dir.is_dir() or ds_dir.name.startswith("."):
            continue
        for f in sorted(ds_dir.rglob("*.json")):
            yield ds_dir.name, f


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--raw-dir", default="benchmarks/routerbench/raw/llmrouterbench/bench-release")
    ap.add_argument("--outdir", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--seed", type=int, default=SEED)
    ap.add_argument("--calibration-ratio", type=float, default=CALIBRATION_RATIO)
    args = ap.parse_args()

    root = Path(args.raw_dir)
    if not root.is_dir():
        raise SystemExit(f"✗ raw 目录不存在: {root}（先解压 bench-release.tar.gz）")

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)

    instances: dict[str, dict] = {}
    outcomes_path = outdir / "outcomes.jsonl"
    seen_outcome: set[tuple[str, str]] = set()
    dropped = {"bad_record": 0, "dup_outcome": 0, "mismatched_dir": 0}
    per_ds_model: dict[tuple[str, str], set[str]] = defaultdict(set)   # (dataset_id) -> models
    per_ds_counts: dict[str, dict] = defaultdict(lambda: {"instances": 0, "outcomes": 0, "models": 0})
    files_used = 0

    with outcomes_path.open("w", encoding="utf-8") as out_f:
        for ds_dir_name, f in iter_result_files(root):
            try:
                doc = json.loads(f.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, UnicodeDecodeError):
                dropped["bad_record"] += 1
                continue
            ds = slugify(str(doc.get("dataset_name") or ds_dir_name))
            setting = slugify(str(doc.get("split") or ""))
            model = slugify(str(doc.get("model_name") or f.parent.name))
            if not setting:
                dropped["mismatched_dir"] += 1
                continue
            records = doc.get("records") or []
            if not records:
                dropped["bad_record"] += 1
                continue
            files_used += 1
            dataset_id = f"llmrb_{ds}_{setting}_v1"
            task_family, task_type = FAMILY_META.get(ds, FALLBACK_META)
            rel_source = str(f.relative_to(root.parent))

            for rec in records:
                try:
                    score = float(rec["score"])
                    cost = float(rec["cost"])
                except (KeyError, TypeError, ValueError):
                    dropped["bad_record"] += 1
                    continue
                origin_query = str(rec.get("origin_query") or rec.get("prompt") or "")
                if not origin_query:
                    dropped["bad_record"] += 1
                    continue
                qhash = hashlib.sha256(f"{ds}\x00{setting}\x00{origin_query}".encode("utf-8")).hexdigest()
                sample_id = f"llmrb_{ds}_{setting}_{qhash[:12]}"
                if sample_id not in instances:
                    instances[sample_id] = {
                        "dataset_id": dataset_id,
                        "sample_id": sample_id,
                        "task_family": task_family,
                        "task_type": task_type,
                        "prompt": str(rec.get("prompt") or origin_query),
                        "ground_truth": (None if rec.get("ground_truth") is None
                                         else str(rec["ground_truth"])),
                        "split": split_of(sample_id, args.seed, args.calibration_ratio),
                        "meta": {
                            "source_dataset": ds,
                            "source_setting": setting,
                            "origin_query": origin_query,
                            "origin_index": rec.get("index"),
                        },
                    }
                key = (sample_id, model)
                if key in seen_outcome:
                    dropped["dup_outcome"] += 1
                    continue
                seen_outcome.add(key)
                p_tok, c_tok = rec.get("prompt_tokens"), rec.get("completion_tokens")
                outcome = {
                    "dataset_id": dataset_id,
                    "sample_id": sample_id,
                    "model_id": model,
                    "raw_score": score,
                    "score": score,               # v1 归一化 = raw（量纲见 manifest.scorer_note）
                    "raw_cost": cost,
                    "cost": cost,
                    "cost_basis": "llmrouterbench_source",
                    "prompt_tokens": int(p_tok) if isinstance(p_tok, (int, float)) else None,
                    "completion_tokens": int(c_tok) if isinstance(c_tok, (int, float)) else None,
                    "latency_ms": None,
                    "response_sha256": None,
                    "source": "llmrouterbench",
                    "source_model": doc.get("model_name") or model,
                    "source_file": rel_source,
                    "source_record_index": rec.get("index"),
                }
                out_f.write(json.dumps(outcome, ensure_ascii=False) + "\n")
                per_ds_model[dataset_id].add(model)

    if not instances:
        raise SystemExit("✗ 零有效实例——检查 raw 目录内容。")

    inst_path = outdir / "instances.jsonl"
    with inst_path.open("w", encoding="utf-8") as f:
        for inst in instances.values():
            f.write(json.dumps(inst, ensure_ascii=False) + "\n")

    for ds_id, models in per_ds_model.items():
        c = per_ds_counts[ds_id]
        c["models"] = len(models)
    for sid, inst in instances.items():
        per_ds_counts[inst["dataset_id"]]["instances"] += 1
    with outcomes_path.open(encoding="utf-8") as f:
        for line in f:
            ds = json.loads(line)["dataset_id"]
            per_ds_counts[ds]["outcomes"] += 1

    manifest = {
        "status": "provisional",  # benchmark_id 待池冻结（13-v2 §7.5）
        "created_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "schema_version": 2,
        "source": {
            "type": "llmrouterbench",
            "revision": "hf-mirror.com/datasets/NPULH/LLMRouterBench bench-release.tar.gz",
            "source_url": "https://hf-mirror.com/datasets/NPULH/LLMRouterBench/resolve/main/bench-release.tar.gz",
            "files": [{
                "path": "benchmarks/routerbench/raw/llmrouterbench/bench-release.tar.gz",
                "sha256": TAR_SHA256,
                "integrity_gate": "解压前 shasum 校验（12 号记录同值）",
            }],
            "result_files_used": files_used,
        },
        "scoring": {
            "metric": "raw_score（v1 normalized=raw）",
            "note": "accuracy 类 0/1；hle/arenahard* 为 LLM-judge 分（源量纲）；详见 doc 13-参考资料 §4.2",
            "scorer_groups": SCORER_NOTE,
        },
        "prompt_profile": {"mode": "source_prompt", "note": "沿用源 benchmark 的 prompt 模板与采样设定"},
        "split": {
            "calibration_ratio": args.calibration_ratio,
            "test_ratio": round(1 - args.calibration_ratio, 4),
            "seed": args.seed,
            "method": "哈希桶（与 import_frozen.py 同法）；官方 split 边界已编入 dataset_id（<ds>_<setting>）",
        },
        "row_counts": {
            "instances": len(instances),
            "outcomes": len(seen_outcome),
            "dropped": dropped,
        },
        "per_dataset": {ds: c for ds, c in sorted(per_ds_counts.items())},
        "notes": [
            "raw_output 不复制进 canonical：outcomes 带 source_file/source_record_index 溯源，"
            "原文在 raw/llmrouterbench/bench-release/（sha256 门控归档）",
            "cost_basis=llmrouterbench_source：与 RouterBench source 成本口径分轨，headline 禁止跨源混算",
            "sample_id 内容哈希派生（ds+setting+origin_query），与行序/record index 无关",
        ],
    }
    (outdir / "source_manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    print(f"✓ instances : {inst_path}  ({len(instances)} 行)")
    print(f"✓ outcomes  : {outcomes_path}  ({len(seen_outcome)} 行)")
    print(f"✓ manifest  : {outdir / 'source_manifest.json'}")
    print(f"  文件 {files_used} 个 | 丢弃 {dropped} | 切分 seed={args.seed} cal={args.calibration_ratio:.0%}")
    for ds, c in sorted(per_ds_counts.items()):
        print(f"  {ds:<40} n={c['instances']:<6} outcomes={c['outcomes']:<7} models={c['models']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
