#!/usr/bin/env python3
"""import_frozen.py — RouterBench 0-shot pkl → canonical frozen matrix（Day 1）。

实测 pkl 结构（2026-10-05，36,497 行 × 37 列，宽表）：
  sample_id / prompt / eval_name（细粒度，MMLU 拆子题组）
  <model>                 → 该模型 quality 分数（11 个模型列）
  <model>|total_cost      → 该模型推理成本（USD）
  <model>|model_response  → 该模型响应原文
  oracle_model_to_route_to → 源数据逐题 oracle（仅存 provenance，replay 不消费）

输出（10 号文档 §6.1/§16.1 约定）：
  normalized/instances.jsonl      每 query 一行（dataset_id/sample_id/task_family/task_type/
                                  prompt/ground_truth/split/meta）
  normalized/outcomes.jsonl       每 query×model 一行（score/cost/tokens/response_sha256/…）
  normalized/source_manifest.json 源文件 sha256、列映射、行数、切分方法（泄漏防护的可追溯凭证）

设计约定（红线，见 11 号文档 §四）：
- **split 在导入时即生成**（20/80，seed=3407），写入每行——泄漏防护前置，不推迟；
- sample_id = family 前缀 + 源 sample_id，全局唯一 + 去重护栏；
- 本脚本只读 raw/、只写 normalized/，不触碰 production 表（零成本红线的导入侧）；
- task_type 仅是 PlanInput 的 metadata adapter（10 号 §八），不构成第二套路由规则。

反序列化安全（Mimosa 红线对应）：
- pkl 属 pickle 反序列化面，加载前先做 **sha256 完整性校验**（--expect-sha256，不符即拒载）；
- 加载走 **模块白名单受限 Unpickler**（只放行 pandas/numpy/builtins 族），阻断任意类注入。

用法：
  .venv/bin/python scripts/import_frozen.py --inspect
  .venv/bin/python scripts/import_frozen.py \
      --expect-sha256 ba4f77f19517610a707c374e99322d7750c30fc4ae7ff5527888595a1e65d36d
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pickle
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

import pandas as pd

# ── eval_name（细粒度）→ task family 归组 ──
# Day 2 screening 的候选粒度是 family（11 号 §三：mmlu/hellaswag/arc/winogrande/gsm8k/mbpp/mt_bench）。
EVAL_FAMILY_EXACT: dict[str, str] = {
    "grade-school-math": "gsm8k",
    "arc-challenge": "arc",
    "mbpp": "mbpp",
    "hellaswag": "hellaswag",
    "winogrande": "winogrande",
    "mtbench": "mt_bench",
    "mtbench-math": "mt_bench",
    "mtbench-reference": "mt_bench",
}
EVAL_FAMILY_PREFIX: tuple[tuple[str, str], ...] = (("mmlu-", "mmlu"),)

# family → (task_family, task_type)：与 task_mapping.yaml §八对齐；未收录 family 兜底 other/general。
FAMILY_META: dict[str, tuple[str, str]] = {
    "gsm8k": ("mathematics", "math_logic"),
    "mbpp": ("coding", "coding"),
    "mmlu": ("knowledge", "general"),
    "hellaswag": ("commonsense", "general"),
    "winogrande": ("commonsense", "general"),
    "arc": ("knowledge", "general"),
    "mt_bench": ("dialogue", "general"),
}
FALLBACK_META: tuple[str, str] = ("other", "general")

# 受限 Unpickler 的模块白名单：pkl 是 pandas DataFrame，只会引用这些模块族。
# 既放行裸模块名（numpy/pandas/builtins…），也放行其子模块（pandas.core.* 等）。
UNPICKLER_ALLOWED_EXACT = {"numpy", "pandas", "builtins", "copyreg", "_codecs", "collections", "datetime"}
UNPICKLER_MODULE_PREFIXES = (
    "pandas.", "numpy.", "builtins.", "copyreg.", "_codecs.", "collections.", "datetime."
)


def slugify(name: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "_", str(name).strip().lower()).strip("_")
    return s or "unknown"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


class _AllowlistUnpickler(pickle.Unpickler):
    """仅放行白名单模块的受限 Unpickler——pickle 数据被替换注入任意类时在此拒载。"""

    def find_class(self, module: str, name: str):
        if module in UNPICKLER_ALLOWED_EXACT or module.startswith(UNPICKLER_MODULE_PREFIXES):
            return super().find_class(module, name)
        raise pickle.UnpicklingError(
            f"受限 Unpickler 拒载模块 {module}.{name}（不在白名单："
            f"{sorted(UNPICKLER_ALLOWED_EXACT)} 或前缀 {UNPICKLER_MODULE_PREFIXES}）"
        )


def load_dataframe(pkl_path: Path, expect_sha256: str | None) -> pd.DataFrame:
    actual_sha = sha256_file(pkl_path)
    if expect_sha256 and actual_sha != expect_sha256:
        raise SystemExit(
            f"✗ sha256 不符，拒载：\n  期望 {expect_sha256}\n  实际 {actual_sha}\n"
            f"  文件可能损坏或被替换，请重新下载后再试。"
        )
    if not expect_sha256:
        print(f"⚠ 未提供 --expect-sha256，未做完整性校验就反序列化（实测 sha256={actual_sha}）",
              file=sys.stderr)
    with pkl_path.open("rb") as f:
        return _AllowlistUnpickler(f).load()


def family_of(eval_name: str) -> str:
    e = str(eval_name).strip().lower()
    if e in EVAL_FAMILY_EXACT:
        return EVAL_FAMILY_EXACT[e]
    for prefix, family in EVAL_FAMILY_PREFIX:
        if e.startswith(prefix):
            return family
    return slugify(e)


def detect_model_columns(df: pd.DataFrame) -> dict[str, dict[str, str]]:
    """宽表 → 每模型三列：quality（裸模型名列）、cost（|total_cost）、response（|model_response）。
    以「存在对应 |total_cost 列」为模型列判据，不硬编码模型清单。"""
    cols = set(df.columns)
    models: dict[str, dict[str, str]] = {}
    for c in df.columns:
        if f"{c}|total_cost" in cols:
            models[str(c)] = {
                "score": c,
                "cost": f"{c}|total_cost",
                "response": f"{c}|model_response" if f"{c}|model_response" in cols else None,
            }
    if len(models) < 2:
        raise SystemExit(
            f"✗ 模型列探测失败（找到 {len(models)} 个）：宽表应含 <model> 与 <model>|total_cost 列对。\n"
            f"  实际列: {list(df.columns)}"
        )
    return models


def split_of(sample_id: str, seed: int, calibration_ratio: float) -> str:
    """哈希桶确定性切分：无需随机模块，跨机/跨运行可复现（同 sample_id 同 split）。
    逐样本独立映射，calibration 占比在统计意义上收敛到 calibration_ratio；
    重切（改 seed/ratio）= 新 benchmark_id（10 号 §6.2）。"""
    digest = hashlib.sha256(f"lloom-split:{seed}:{sample_id}".encode("utf-8")).digest()
    u = int.from_bytes(digest[:16], "big")
    return "calibration" if u / 2**128 < calibration_ratio else "test"


def run(args: argparse.Namespace) -> None:
    pkl_path = Path(args.input)
    if not pkl_path.exists():
        raise SystemExit(f"✗ 找不到输入文件: {pkl_path}")

    df = load_dataframe(pkl_path, args.expect_sha256)
    if not isinstance(df, pd.DataFrame):
        raise SystemExit(f"✗ pkl 顶层对象是 {type(df).__name__}，期望 DataFrame")
    model_cols = detect_model_columns(df)

    required = ("sample_id", "prompt", "eval_name")
    missing = [c for c in required if c not in df.columns]
    if missing:
        raise SystemExit(f"✗ 必需列缺失: {missing}（实际列: {list(df.columns)}）")

    if args.inspect:
        print(f"shape: {df.shape}")
        print(f"models: {len(model_cols)} → {sorted(model_cols)}")
        print(f"eval_name nunique: {df['eval_name'].nunique()}")
        print(df.head(3).to_string())
        return

    outdir = Path(args.outdir)
    outdir.mkdir(parents=True, exist_ok=True)

    # ── 宽表展开：一 query 一实例；11 模型拉平成长表 outcome ──
    instances: dict[str, dict] = {}
    outcomes: list[dict] = []
    dropped = {"bad_score": 0, "bad_cost": 0, "dup_sample_id": 0, "dup_outcome": 0}
    for row in df.to_dict(orient="records"):
        eval_raw = str(row["eval_name"])
        family = family_of(eval_raw)
        orig_sid = str(row["sample_id"]).strip()
        sample_id = f"rb0_{family}_{orig_sid}"
        if sample_id in instances:
            dropped["dup_sample_id"] += 1
            continue

        prompt = str(row["prompt"])
        task_family, task_type = FAMILY_META.get(family, FALLBACK_META)
        instances[sample_id] = {
            "dataset_id": f"rb0shot_{family}_v1",
            "sample_id": sample_id,
            "task_family": task_family,
            "task_type": task_type,
            "prompt": prompt,
            "ground_truth": None,  # 源数据无逐题 ground truth 列
            "split": split_of(sample_id, args.seed, args.calibration_ratio),
            "meta": {
                "source_eval": eval_raw,
                "source_sample_id": orig_sid,
                "oracle_model": row.get("oracle_model_to_route_to"),
                "prompt_chars": len(prompt),
            },
        }

        for model_raw, colmap in sorted(model_cols.items()):
            try:
                score = float(row[colmap["score"]])
            except (TypeError, ValueError):
                dropped["bad_score"] += 1
                continue
            try:
                cost = float(row[colmap["cost"]])
            except (TypeError, ValueError):
                dropped["bad_cost"] += 1
                continue
            response = row.get(colmap["response"]) if colmap["response"] else None
            outcomes.append({
                "dataset_id": f"rb0shot_{family}_v1",
                "sample_id": sample_id,
                "model_id": slugify(model_raw),
                "score": score,
                "cost": cost,
                "prompt_tokens": None,   # 源 0-shot pkl 无 token 列
                "completion_tokens": None,
                "latency_ms": None,
                "response_sha256": (
                    hashlib.sha256(str(response).encode("utf-8")).hexdigest()
                    if response is not None else None
                ),
                "source": "routerbench",
                "source_model": model_raw,
            })

    if not instances:
        raise SystemExit("✗ 零有效实例，检查输入数据。")

    # ── outcome 去重（同 sample×model 保留首条）+ score/cost 非空校验（Day 1 验收项）──
    seen: set[tuple[str, str]] = set()
    deduped: list[dict] = []
    for o in outcomes:
        key = (o["sample_id"], o["model_id"])
        if key in seen:
            dropped["dup_outcome"] += 1
            continue
        seen.add(key)
        deduped.append(o)

    # ── 落盘 ──
    outdir.mkdir(parents=True, exist_ok=True)
    inst_path = outdir / "instances.jsonl"
    out_path = outdir / "outcomes.jsonl"
    with inst_path.open("w", encoding="utf-8") as f:
        for inst in instances.values():
            f.write(json.dumps(inst, ensure_ascii=False) + "\n")
    with out_path.open("w", encoding="utf-8") as f:
        for o in deduped:
            f.write(json.dumps(o, ensure_ascii=False) + "\n")

    model_counts: dict[str, int] = {}
    for o in deduped:
        model_counts[o["model_id"]] = model_counts.get(o["model_id"], 0) + 1
    ids_by_dataset: dict[str, list[str]] = {}
    for inst in instances.values():
        ids_by_dataset.setdefault(inst["dataset_id"], []).append(inst["sample_id"])
    per_dataset = {
        ds: {
            "instances": len(ids),
            "calibration": sum(1 for i in ids if split_of(i, args.seed, args.calibration_ratio) == "calibration"),
            "test": sum(1 for i in ids if split_of(i, args.seed, args.calibration_ratio) == "test"),
        }
        for ds, ids in sorted(ids_by_dataset.items())
    }

    manifest = {
        "status": "provisional",  # benchmark_id 待 Day 3 冻结（10 号 §6.2）
        "created_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "schema_version": 1,
        "source": {
            "type": "routerbench",
            "revision": "hf-mirror.com/datasets/withmartian/routerbench (0shot pkl, main)",
            "source_url": "https://hf-mirror.com/datasets/withmartian/routerbench/resolve/main/routerbench_0shot.pkl",
            "files": [
                {
                    "path": str(pkl_path),
                    "bytes": pkl_path.stat().st_size,
                    "sha256": sha256_file(pkl_path),
                    "integrity_gate": "load 前 --expect-sha256 校验 + 模块白名单受限 Unpickler",
                }
            ],
        },
        "scoring": {
            "metric": "routerbench_performance",
            "note": "源数据 quality 列；MMLU/ARC/HellaSwag/Winogrande/GSM8K/MBPP 为 0/1 正确性，"
                    "mtbench 为 LLM-judge 分（量纲未归一，screening/replay 时按 family 分别解读）",
        },
        "prompt_profile": {"mode": "zero_shot", "note": "官方 0-shot 矩阵，sampling 参数沿用源数据"},
        "split": {
            "calibration_ratio": args.calibration_ratio,
            "test_ratio": round(1 - args.calibration_ratio, 4),
            "seed": args.seed,
            "method": (
                "哈希桶确定性切分：sha256('lloom-split:{seed}:{sample_id}') 前 128bit 归一后"
                "< calibration_ratio → calibration，否则 test。逐样本独立、与行序无关、"
                "跨机可复现；改 seed/ratio = 新 benchmark_id。"
            ),
        },
        "models": [
            {"id": m, "source_rows": c, "cost_basis": "source"}
            for m, c in sorted(model_counts.items())
        ],
        "row_counts": {
            "instances": len(instances),
            "outcomes": len(deduped),
            "dropped": dropped,
        },
        "per_dataset": per_dataset,
        "column_mapping": {
            "wide_format": True,
            "quality": "<model> 裸列",
            "cost": "<model>|total_cost",
            "response": "<model>|model_response",
            "models_detected": sorted(model_cols),
            "extra": {"sample_id": "sample_id", "eval_name": "eval_name",
                      "oracle": "oracle_model_to_route_to"},
        },
        "notes": [
            "response 原文未落盘（可选 audit artifact，10 号 §6.1）；outcomes 仅存 response_sha256",
            "ground_truth 源数据缺失为 null，不影响 replay（replay 只查 score/cost）",
            "oracle_model_to_route_to 仅存 instances.meta 作 provenance；replay 的 Oracle baseline "
            "从冻结矩阵自算（10 号 §12.4），不消费源 oracle 列",
            "eval_name 细粒度（MMLU 57 子题组）已归组到 family 级 dataset_id，源值保留在 meta.source_eval",
        ],
    }
    manifest_path = outdir / "source_manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    print(f"✓ instances : {inst_path}  ({len(instances)} 行)")
    print(f"✓ outcomes  : {out_path}  ({len(deduped)} 行)")
    print(f"✓ manifest  : {manifest_path}")
    print(f"  模型 {len(model_counts)} 个 | 丢弃 {dropped} | 切分 seed={args.seed} "
          f"calibration {args.calibration_ratio:.0%}")
    for ds, c in per_dataset.items():
        print(f"  {ds:<32} n={c['instances']:<6} cal={c['calibration']:<5} test={c['test']}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--input", default="benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl")
    ap.add_argument("--outdir", default="benchmarks/routerbench/normalized")
    ap.add_argument("--seed", type=int, default=3407)
    ap.add_argument("--calibration-ratio", type=float, default=0.2)
    ap.add_argument("--expect-sha256", default=None, help="下载件 sha256，不符即拒载（推荐始终提供）")
    ap.add_argument("--inspect", action="store_true", help="只打印 pkl 结构，不写文件")
    args = ap.parse_args()
    run(args)


if __name__ == "__main__":
    sys.exit(main())
