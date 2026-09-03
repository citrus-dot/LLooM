#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
AIQ 重放：从 LiteLLM 路由库离线计算「成本 × 成效」曲线与 AIQ 指标（ROUTING-PLAN §P1.d）。

RouterBench 式 AIQ = (当前策略质量 − 全弱质量) / (全强质量 − 全弱质量) 的预算积分。
本脚本用**影子评测真实样本**（routing_calibration）对比三条线的成本—质量：
  - 全弱基线（Weak）  ：最便宜可用模型（本地零成本/最低价）
  - 当前策略（Current）：路由实际选择（routing_calibration.routed_*）
  - 全强基线（Strong） ：旗舰模型（routing_calibration.baseline_*）
无依赖，仅用 stdlib sqlite3。quality 缺失时从 models / model_task_score 回填并写库。

用法：
  python3 scripts/aiq_replay.py [--db PATH] [--ratio SAVE_RATIO] [--json]
输出：
  - 三条线的样本数/总成本/平均质量
  - AIQ（成本区间内质量填充比）与当前策略相对全强的成本节省
  - --json：机器可读报告（N2.a 周期 job 消费，数字与文本报告同源同值）
"""

import argparse
import json
import os
import sqlite3
import sys


def resolve_db(explicit: str | None) -> str:
    if explicit:
        return explicit
    if os.environ.get("LLOOM_DATA_DIR"):
        return os.path.join(os.environ["LLOOM_DATA_DIR"], "lloom.db")
    # 脚本在 scripts/ 下 → 库里在上级 data/
    return os.path.join(os.path.dirname(__file__), "..", "data", "lloom.db")


def model_quality(conn, scores, name: str, task_type: str) -> float:
    """成效分优先（sample>=5 的 ewma），否则回落冷启动 quality_score。"""
    key = (name, task_type)
    if key in scores:
        ewma, n = scores[key]
        if n >= 5:
            return ewma
    row = conn.execute(
        "SELECT quality_score FROM models WHERE name = ?", (name,)
    ).fetchone()
    return row[0] if row else 0.0


def cost_rate(conn, name: str) -> float:
    """混合单价比率（输入 + 2×输出），用于排序弱/强。无价格 → 0（本地）按最便宜排。"""
    row = conn.execute(
        "SELECT input_cost_per_token, output_cost_per_token FROM models WHERE name = ?",
        (name,),
    ).fetchone()
    if not row:
        return float("inf")
    i, o = row
    if i is None or o is None:
        return float("inf")
    return float(i) + 2.0 * float(o)


def main() -> int:
    ap = argparse.ArgumentParser(description="AIQ 离线重放：路由成本×成效曲线")
    ap.add_argument("--db", help="SQLite 库路径（默认 data/lloom.db）")
    ap.add_argument("--ratio", type=float, default=0.15, help="EWMA 样本权重，仅用于说明")
    ap.add_argument("--json", action="store_true", help="输出机器可读 JSON（与文本报告数字一致）")
    args = ap.parse_args()

    report = compute(args)
    if report is None:
        return 2 if not os.path.exists(resolve_db(args.db)) else 1
    if args.json:
        print(json.dumps(report, ensure_ascii=False))
    else:
        print_report(report, resolve_db(args.db))
    return 0


def compute(args) -> dict | None:
    """计算三线成本/质量 + AIQ，返回 report dict；失败（无库/无样本）返回 None
    （错误信息已打 stderr，exit code 由 main 按「库是否存在」区分）。"""
    db_path = resolve_db(args.db)
    if not os.path.exists(db_path):
        print(f"[aiq] 库不存在：{db_path}", file=sys.stderr)
        return None

    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row

    cal = conn.execute(
        "SELECT id, task_type, query_hash, routed_model, baseline_model,"
        " routed_cost, baseline_cost, routed_quality, baseline_quality"
        " FROM routing_calibration"
    ).fetchall()
    if not cal:
        print("[aiq] routing_calibration 无样本——先用 POST /api/routing/shadow 采集。", file=sys.stderr)
        return None

    # 归并按模型名取对应的质量/成本（先建映射，避免重复查询）
    models = conn.execute(
        "SELECT name, quality_score, capability_tier, input_cost_per_token, output_cost_per_token"
        " FROM models WHERE is_active = 1"
    ).fetchall()
    score_rows = conn.execute(
        "SELECT model_name, task_type, ewma_quality, sample_count FROM model_task_score"
    ).fetchall()
    scores: dict = {}
    for r in score_rows:
        scores[(r["model_name"], r["task_type"])] = (r["ewma_quality"], r["sample_count"])
    name_scores: dict = {}  # task_type -> {model: quality}
    for r in cal:
        name_scores.setdefault(r["task_type"], {})
        for m in (r["routed_model"], r["baseline_model"]):
            name_scores[r["task_type"]].setdefault(m, model_quality(conn, scores, m, r["task_type"]))

    model_names = {r["name"] for r in models}

    # 逐样本算质量（回填 NULL 并写库）
    routed_q_all, base_q_all = [], []
    routed_c, base_c = 0.0, 0.0
    for r in cal:
        rq = r["routed_quality"]
        if rq is None:
            rq = name_scores[r["task_type"]].get(r["routed_model"], 0.0)
        bq = r["baseline_quality"]
        if bq is None:
            bq = name_scores[r["task_type"]].get(r["baseline_model"], 0.0)
        # 回填质量，供下次重放直接使用
        if r["routed_quality"] is None or r["baseline_quality"] is None:
            conn.execute(
                "UPDATE routing_calibration SET routed_quality = ?1, baseline_quality = ?2"
                " WHERE id = ?3",
                (rq, bq, r["id"]),
            )
        routed_q_all.append(rq)
        base_q_all.append(bq)
        routed_c += r["routed_cost"] or 0.0
        base_c += r["baseline_cost"] or 0.0
    conn.commit()

    n = len(cal)

    # 全弱基线：样本域里各任务最便宜模型的成本等价重放。实际弱成本无法从影子样本直接得出，
    # 用「当前策略成本 × 弱/当前 价格比」估算（本地/最便宜 → 低成本）。
    weak_q_all, weak_c = [], 0.0
    for r in cal:
        cheap = min(
            (m for m in name_scores[r["task_type"]] if m in model_names),
            key=lambda m: cost_rate(conn, m),
            default=r["routed_model"],
        )
        weak_q_all.append(name_scores[r["task_type"]].get(cheap, 0.0))
        ratio = cost_rate(conn, cheap) / max(cost_rate(conn, r["routed_model"]), 1e-12)
        weak_c += (r["routed_cost"] or 0.0) * ratio

    cur_q = sum(routed_q_all) / n
    strong_q = sum(base_q_all) / n
    weak_q = sum(weak_q_all) / n

    # AIQ：质量在 [weak, strong] 内的填充比例（RouterBench 预算积分），clamp 到 [0,1]
    denom = strong_q - weak_q
    aiq = (cur_q - weak_q) / denom if denom > 1e-9 else (1.0 if cur_q >= strong_q else 0.0)
    aiq = max(0.0, min(1.0, aiq))
    saved_pct = (base_c - routed_c) / base_c * 100.0 if base_c > 0 else 0.0

    # 决策建议：AIQ 高说明路由聪明；成本节省高说明经济。二者失衡则提示调权重。
    if aiq >= 0.8 and saved_pct >= 40:
        conclusion = "路由既省又快还保质量，无需调整。"
    elif aiq >= 0.8 and saved_pct < 40:
        conclusion = "质量已接近全强但成本节省不足——可提高 cost_weight/下调 quality_weight。"
    elif aiq < 0.8 and saved_pct >= 40:
        conclusion = "省得多但质量掉损大——应提高 quality_weight 或提高 min_capability_tier。"
    else:
        conclusion = "成本与质量大致平衡；建议增加影子样本后再判定。"

    return {
        "samples": n,
        "weak": {"cost": weak_c, "quality": weak_q},
        "current": {"cost": routed_c, "quality": cur_q},
        "strong": {"cost": base_c, "quality": strong_q},
        "aiq": round(aiq, 4),
        "saved_pct": round(saved_pct, 2),
        "conclusion": conclusion,
    }


def print_report(rep: dict, db_path: str) -> None:
    n = rep["samples"]
    print(f"\n[aiq] 影子样本：{n} 条  (db={db_path})")
    print(f"[aiq] 全弱基线 Weak    : 成本 ${rep['weak']['cost']:.6f}  平均质量 {rep['weak']['quality']:.3f}")
    print(f"[aiq] 当前策略 Current : 成本 ${rep['current']['cost']:.6f}  平均质量 {rep['current']['quality']:.3f}")
    print(f"[aiq] 全强基线 Strong  : 成本 ${rep['strong']['cost']:.6f}  平均质量 {rep['strong']['quality']:.3f}")
    print(f"\n[aiq] AIQ = (当前−弱)/(强−弱) = {rep['aiq']:.3f}   （0~1，越高越接近全强质量）")
    print(f"[aiq] 相对全强成本节省 = {rep['saved_pct']:.1f}%（付出质量代价换取的成本）")
    print(f"[aiq] 结论：{rep['conclusion']}")


if __name__ == "__main__":
    sys.exit(main())
