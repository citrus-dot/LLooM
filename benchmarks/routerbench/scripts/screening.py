#!/usr/bin/env python3
"""screening.py — RouterBench 七信号 screening（10 号文档 §4.2/§17；Day 1 交付，Day 2 全量跑）。

对冻结矩阵（normalized/instances.jsonl + outcomes.jsonl，或直读 raw pkl）在显式临时模型池上
逐 dataset 计算 S1-S7 七信号，输出 reports/screening.json + screening.csv。

信号定义（10 号 §4.2，pool = weak/mid/strong 三元临时池）：
  S1 model_gap            = BestSingle - min_m mean(score)         基础模型差异
  S2 oracle_gain          = mean(max_m score) - BestSingle         routing headroom（最重要）
  S3 upgrade_opportunity  = P(score_weak < score_strong)           确实需要升级的题占比
  S4 cheap_safe           = P(score_weak >= score_strong)          不必花强模型钱的题占比
  S5 disagreement         = 二元: P(∃i≠j score_i≠score_j)；连续: mean(max-min)
  S6 winner_entropy       = -Σ p_m ln p_m（argmax 并列时按 1/k 平分 winner 记账）
  S7 cost_ratio           = mean(cost_strong) / mean(cost_weak)    成本梯度（工程门槛 ≥3×）

防泄漏：
- 默认只算 calibration split（10 号 §7.3：benchmark 选择过程不得污染 test），--split 可改；
- 池由 --models 显式传入（临时池，Day 3 才冻结），输出固定 provisional_pool: true；
- screening_score = oracle_gain + upgrade_opportunity + cheap_safe + winner_entropy
  （§5.1 rank 求和口径，仅用于排序，不是研究结论阈值）。

用法（在仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/screening.py \
      --models <weak> <mid> <strong> \
      --tasks gsm8k mmlu hellaswag arc winogrande mbpp mt_bench \
      --output benchmarks/routerbench/reports/screening.json
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from import_frozen import (  # noqa: E402  复用导入器的列探测/family 归组/切分，公式单源
    FAMILY_META,
    FALLBACK_META,
    detect_model_columns,
    family_of,
    load_dataframe,
    slugify,
    split_of,
)

EPS = 1e-12
DEFAULT_TASKS = ("gsm8k", "mmlu", "hellaswag", "arc", "winogrande", "mbpp", "mt_bench")
SCREENING_SCORE_FORMULA = "oracle_gain + upgrade_opportunity + cheap_safe + winner_entropy (10 号 §5.1 等权 rank)"


def load_inputs(input_path: Path, seed: int, calibration_ratio: float):
    """返回 (instances, outcomes)。

    - 目录：canonical 冻结矩阵（import_frozen.py 产物，instances.jsonl + outcomes.jsonl）；
    - .pkl：直读 raw 矩阵（10 号 §17 用法），sample_id/split 公式与 import_frozen 保持同源。
      注意 pkl 直读不含 prompt 落盘，screening 只需要 score/cost，够用。
    """
    if input_path.is_dir():
        inst_path = input_path / "instances.jsonl"
        out_path = input_path / "outcomes.jsonl"
        if not inst_path.exists() or not out_path.exists():
            raise SystemExit(
                f"✗ {input_path} 缺 instances.jsonl / outcomes.jsonl——请先跑 import_frozen.py 生成冻结矩阵"
            )
        # 用 split("\n") 而非 splitlines()：JSON 字符串里合法的 U+2028/U+2029 会被 splitlines 误切
        instances = [
            json.loads(line)
            for line in inst_path.read_text(encoding="utf-8").split("\n")
            if line.strip()
        ]
        outcomes = [
            json.loads(line)
            for line in out_path.read_text(encoding="utf-8").split("\n")
            if line.strip()
        ]
        return instances, outcomes

    if input_path.is_file() and input_path.suffix == ".pkl":
        df = load_dataframe(input_path, None)
        model_cols = detect_model_columns(df)
        instances: list[dict] = []
        outcomes: list[dict] = []
        for row in df.to_dict(orient="records"):
            family = family_of(row["eval_name"])
            sample_id = f"rb0_{family}_{str(row['sample_id']).strip()}"
            dataset_id = f"rb0shot_{family}_v1"
            task_family, task_type = FAMILY_META.get(family, FALLBACK_META)
            instances.append({
                "dataset_id": dataset_id,
                "sample_id": sample_id,
                "task_family": task_family,
                "task_type": task_type,
                "split": split_of(sample_id, seed, calibration_ratio),
            })
            for model_raw, cols in model_cols.items():
                try:
                    score = float(row[cols["score"]])
                    cost = float(row[cols["cost"]])
                except (TypeError, ValueError):
                    continue
                outcomes.append({
                    "dataset_id": dataset_id,
                    "sample_id": sample_id,
                    "model_id": slugify(model_raw),
                    "score": score,
                    "cost": cost,
                })
        return instances, outcomes

    raise SystemExit(f"✗ --input 需为 normalized 目录或 .pkl 文件，收到: {input_path}")


def mean(values: list[float]) -> float:
    return sum(values) / len(values)


def screen_dataset(dataset_id: str, insts: list[dict], by_sample: dict, pool: dict) -> dict | None:
    """对一个 dataset（已按 split 过滤的实例）在三元池上算 S1-S7。

    pool: {"weak": model_id, "mid": model_id, "strong": model_id}
    by_sample: sample_id -> {model_id -> (score, cost)}
    只统计池内模型 outcome 全覆盖的实例（10 号 §5.1 Gate 1：coverage<100% 的 dataset 不进 headline）。
    """
    model_ids = [pool["weak"], pool["mid"], pool["strong"]]
    n_total = len(insts)
    covered: list[dict] = []
    for inst in insts:
        per = by_sample.get(inst["sample_id"])
        if per and all(m in per for m in model_ids):
            covered.append(per)
    n_cov = len(covered)
    if n_cov == 0:
        return None

    weak_m, mid_m, strong_m = model_ids
    means = {
        "weak": mean([per[weak_m][0] for per in covered]),
        "mid": mean([per[mid_m][0] for per in covered]),
        "strong": mean([per[strong_m][0] for per in covered]),
    }
    best_single = max(means.values())
    worst_mean = min(means.values())

    oracle_mean = mean([max(per[m][0] for m in model_ids) for per in covered])
    upgrade_opportunity = mean([1.0 if per[weak_m][0] < per[strong_m][0] - EPS else 0.0 for per in covered])
    cheap_safe = mean([1.0 if per[weak_m][0] >= per[strong_m][0] - EPS else 0.0 for per in covered])

    # S5 disagreement：分数全为 0/1 视为二元（频次判据），否则连续（mean(max-min)）
    all_scores = [s for per in covered for s, _ in per.values()]
    is_binary = all(abs(s - round(s)) < 1e-9 and round(s) in (0, 1) for s in all_scores)
    if is_binary:
        disagreement = mean([
            1.0 if len({round(s, 9) for s, _ in per.values()}) > 1 else 0.0 for per in covered
        ])
        disagreement_mode = "binary"
    else:
        disagreement = mean([max(s for s, _ in per.values()) - min(s for s, _ in per.values()) for per in covered])
        disagreement_mode = "continuous"

    # S6 winner 熵：argmax 并列时每个并列 winner 记 1/k，避免并列题把熵算爆
    winner_credit: Counter = Counter()
    for per in covered:
        top = max(score for score, _ in per.values())
        winners = [m for m in model_ids if per[m][0] >= top - EPS]
        for m in winners:
            winner_credit[m] += 1.0 / len(winners)
    total_credit = sum(winner_credit.values())
    if total_credit > 0:
        probs = [c / total_credit for c in winner_credit.values() if c > 0]
        winner_entropy = -sum(p * math.log(p) for p in probs)
    else:
        winner_entropy = 0.0

    weak_cost = mean([per[weak_m][1] for per in covered])
    strong_cost = mean([per[strong_m][1] for per in covered])
    cost_ratio = strong_cost / weak_cost if weak_cost > 0 else None

    oracle_gain = oracle_mean - best_single
    screening_score = oracle_gain + upgrade_opportunity + cheap_safe + winner_entropy

    return {
        "dataset": dataset_id,
        "n": n_total,
        "covered": n_cov,
        "coverage": round(n_cov / n_total, 4) if n_total else 0.0,
        "weak": round(means["weak"], 4),
        "mid_mean": round(means["mid"], 4),
        "strong_mean": round(means["strong"], 4),
        "best_single": round(best_single, 4),
        "model_gap": round(best_single - worst_mean, 4),
        "oracle": round(oracle_mean, 4),
        "oracle_gain": round(oracle_gain, 4),
        "upgrade_opportunity": round(upgrade_opportunity, 4),
        "cheap_safe": round(cheap_safe, 4),
        "disagreement": round(disagreement, 4),
        "disagreement_mode": disagreement_mode,
        "winner_entropy": round(winner_entropy, 4),
        "winner_shares": {m: round(winner_credit.get(m, 0.0) / total_credit, 4) if total_credit else 0.0
                          for m in model_ids},
        "cost_ratio": round(cost_ratio, 4) if cost_ratio is not None else None,
        "screening_score": round(screening_score, 4),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--input", default="benchmarks/routerbench/normalized",
                    help="normalized 目录（默认）或 raw pkl 文件")
    ap.add_argument("--models", nargs=3, required=True, metavar=("WEAK", "MID", "STRONG"),
                    help="临时池三模型（model_id slug 或源名，按 weak/mid/strong 顺序）")
    ap.add_argument("--tasks", nargs="*", default=None,
                    help="family 过滤（如 gsm8k mmlu …），缺省跑矩阵内全部 dataset")
    ap.add_argument("--split", choices=("calibration", "test", "all"), default="calibration",
                    help="screening 用的切分（默认 calibration，10 号 §7.3 防选择污染 test）")
    ap.add_argument("--seed", type=int, default=3407, help="pkl 直读时的切分 seed")
    ap.add_argument("--calibration-ratio", type=float, default=0.2, help="pkl 直读时的切分比例")
    ap.add_argument("--output", default="benchmarks/routerbench/reports/screening.json")
    args = ap.parse_args()

    pool = {role: slugify(m) for role, m in zip(("weak", "mid", "strong"), args.models)}
    if len(set(pool.values())) != 3:
        raise SystemExit(f"✗ --models 三个模型去重后不足 3 个: {args.models}")

    instances, outcomes = load_inputs(Path(args.input), args.seed, args.calibration_ratio)

    # 模型存在性校验：先收集矩阵内真实 model_id，找不到即报错并列出可用项
    known_models = {o["model_id"] for o in outcomes}
    unknown = [m for m in pool.values() if m not in known_models]
    if unknown:
        raise SystemExit(
            f"✗ 模型不在矩阵内: {unknown}\n  可用 model_id: {sorted(known_models)}"
        )

    # split 过滤（泄漏防护：screening 默认不见 test）+ family 过滤（dataset_id 子串或 task_family 匹配）
    if args.split != "all":
        instances = [i for i in instances if i.get("split") == args.split]
    if args.tasks:
        wanted = {slugify(t) for t in args.tasks}
        instances = [
            i for i in instances
            if any(t in i["dataset_id"] or t == slugify(i.get("task_family", "")) for t in wanted)
        ]

    by_sample: dict[str, dict[str, tuple[float, float]]] = {}
    for o in outcomes:
        by_sample.setdefault(o["sample_id"], {})[o["model_id"]] = (float(o["score"]), float(o["cost"]))

    by_dataset: dict[str, list[dict]] = {}
    for inst in instances:
        by_dataset.setdefault(inst["dataset_id"], []).append(inst)

    results = []
    for ds in sorted(by_dataset):
        r = screen_dataset(ds, by_dataset[ds], by_sample, pool)
        if r is not None:
            results.append(r)
    results.sort(key=lambda r: -r["screening_score"])

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    report = {
        "provisional_pool": True,  # 池 Day 3 才冻结（10 号 §17 / 11 号 §三 Day2 增量）
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "input": str(args.input),
        "split": args.split,
        "split_note": "screening 默认只看 calibration split（§7.3：benchmark 选择不得污染 test）",
        "pool": pool,
        "tasks_filter": list(args.tasks) if args.tasks else None,
        "screening_score_formula": SCREENING_SCORE_FORMULA,
        "datasets": results,
    }
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    csv_path = output.with_suffix(".csv")
    if results:
        with csv_path.open("w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(results[0].keys()))
            writer.writeheader()
            writer.writerows(results)

    print(f"✓ screening: {output} (+ {csv_path.name}) | split={args.split} | "
          f"pool(weak/mid/strong)={pool['weak']} / {pool['mid']} / {pool['strong']}")
    hdr = (f"{'dataset':<34}{'n':>7}{'cov':>6}{'weak':>7}{'mid':>7}{'best':>7}"
           f"{'oracle':>8}{'o_gain':>8}{'upgr':>7}{'safe':>7}{'disa':>7}{'H':>6}{'cost×':>8}{'score':>8}")
    print(hdr)
    for r in results:
        cr = f"{r['cost_ratio']:.1f}" if r["cost_ratio"] is not None else "n/a"
        print(f"{r['dataset']:<34}{r['n']:>7}{r['coverage']:>6.2f}{r['weak']:>7.3f}{r['mid_mean']:>7.3f}"
              f"{r['best_single']:>7.3f}{r['oracle']:>8.3f}{r['oracle_gain']:>8.3f}"
              f"{r['upgrade_opportunity']:>7.3f}{r['cheap_safe']:>7.3f}{r['disagreement']:>7.3f}"
              f"{r['winner_entropy']:>6.2f}{cr:>8}{r['screening_score']:>8.3f}")


if __name__ == "__main__":
    main()
