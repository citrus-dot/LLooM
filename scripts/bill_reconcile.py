#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
账单对账（NEXT-PLAN §N3.c）：阿里云百炼账单导出 CSV × usage_records.actual_cost 对账。

数据源：
  - 账单侧：费用与成本 > 账单详情，产品名称筛「大模型服务平台百炼」，逐月导出 CSV。
    关键列「实例 ID（出账粒度）」以分号分隔：
      ApiKeyID;业务空间ID;模型名称;输入/输出类型;调用渠道;免费额度用完即停标识
    例：12xxx;llm-xxx;qwen-max;output_token;app;0（控制台调用首段为空）。
  - 本地侧：Rust host SQLite usage_records——act_cost/act_input_cost（USD 真源）与
    input/output tokens。语义缓存命中（cost=0）未真正调用供应商，不计入 token 对账。

口径与换算：
  - LLooM 内部计价为 USD；百炼账单为 CNY。默认 --cny-rate 7.2（或 env LLOOM_CNY_RATE），
    汇率仅影响换算偏差项，不影响同币种的输入侧分项对账（若有 CNY 价格快照可后续接）。
  - created_at 为 UTC（SQLite CURRENT_TIMESTAMP），与账单的北京时间月界有 ±8h 边缘差；
    月初/月末 8h 内的记录可能落在相邻账期，属已知口径差，报告中不做静默修正。

用法：
  python3 scripts/bill_reconcile.py --bill PATH [--db PATH] [--period YYYY-MM]
         [--cny-rate R] [--model-map FILE] [--tolerance PCT] [--save] [--json]
  --period      账期（YYYY-MM），两侧同筛；省略则账单全量 vs 本地全量（多账期 CSV 会告警）
  --model-map   JSON 文件 {"账单模型名": "本地模型名"}，手工映射优先于自动归一
  --tolerance   单模型偏差率阈值（%），默认 5；超过记 ⚠，否则记 ✓ 已对账
  --save        报告写入 <data>/reconcile_last.json（UsagePage「已对账」徽标数据源，N3.c 收尾）
退出码：0 全部对平；1 有偏差/缺数据；2 账单文件或库不存在（与 aiq_replay 约定一致）。
"""

import argparse
import csv
import json
import os
import sqlite3
import sys
from datetime import datetime, timezone


# ── 路径解析（与 aiq_replay 同约定） ──

def resolve_db(explicit: str | None) -> str:
    if explicit:
        return explicit
    if os.environ.get("LLOOM_DATA_DIR"):
        return os.path.join(os.environ["LLOOM_DATA_DIR"], "lloom.db")
    return os.path.join(os.path.dirname(__file__), "..", "data", "lloom.db")


def resolve_data_dir(db_path: str) -> str:
    return os.path.dirname(os.path.abspath(db_path)) or "."


# ── 账单 CSV 解析 ──

def _read_csv_rows(path: str) -> list[dict]:
    """阿里云导出通常 UTF-8(-sig)，个别老账单 GBK——逐个尝试。"""
    for enc in ("utf-8-sig", "utf-8", "gbk"):
        try:
            with open(path, newline="", encoding=enc) as f:
                rows = list(csv.DictReader(f))
            if rows:
                return rows
        except (UnicodeDecodeError, UnicodeError):
            continue
    raise ValueError("无法识别 CSV 编码（试过 utf-8-sig/utf-8/gbk）")


def _col(headers: list[str], *needles: str, exclude: str | None = None) -> str | None:
    """模糊找列名：包含全部 needles（大小写不敏感）；exclude 命中则跳过。"""
    for h in headers:
        hl = h.strip().lower()
        if exclude and exclude.lower() in hl:
            continue
        if all(n.lower() in hl for n in needles):
            return h
    return None


def parse_bill(path: str, period: str | None) -> tuple[dict, dict, list[str]]:
    """解析百炼账单 CSV。
    返回 (per_model, token_by_dir, warnings)：
      per_model: {model: {"bill_cny": float, "rows": int}}
      token_by_dir: {model: {"input_token": n, "output_token": n}}（列/单位可解析时才有）
    """
    rows = _read_csv_rows(path)
    headers = list(rows[0].keys())
    warn: list[str] = []

    inst_col = _col(headers, "实例", "id")
    if not inst_col:
        raise ValueError(f"账单缺「实例 ID（出账粒度）」列，实际列：{headers}")
    amt_col = _col(headers, "应付金额") or _col(headers, "官网价") or _col(headers, "现金支付")
    if not amt_col:
        warn.append("未找到 金额 列（应付金额/官网价），金额对账不可用，仅统计行数")
    usage_col = _col(headers, "用量", exclude="单位")
    unit_col = _col(headers, "计量单位") or _col(headers, "单位")
    prod_col = _col(headers, "产品名称") or _col(headers, "产品")
    month_col = _col(headers, "账单月份") or _col(headers, "账期")
    # 输入/输出方向在实例ID第4段（input_token/output_token），不是独立列

    def to_f(v: str | None) -> float:
        v = (v or "").replace(",", "").replace("¥", "").strip()
        try:
            return float(v)
        except ValueError:
            return 0.0

    def to_n(v: str | None) -> float:
        v = (v or "").replace(",", "").strip()
        try:
            return float(v)
        except ValueError:
            return 0.0

    months_seen: set[str] = set()
    per_model: dict[str, dict] = {}
    tokens: dict[str, dict] = {}
    for r in rows:
        if prod_col and "百炼" not in (r.get(prod_col) or ""):
            continue  # 只对账大模型服务平台百炼产品行
        if month_col and r.get(month_col):
            months_seen.add((r[month_col] or "").strip())
            if period and period not in (r[month_col] or ""):
                continue
        inst = (r.get(inst_col) or "").strip()
        if not inst:
            continue
        parts = inst.split(";")
        if len(parts) < 3:
            warn.append(f"实例ID 段数不足（{len(parts)}）：{inst[:60]}")
            continue
        model = parts[2].strip().lower()
        direction = parts[3].strip().lower() if len(parts) >= 4 else ""
        rec = per_model.setdefault(model, {"bill_cny": 0.0, "rows": 0})
        rec["rows"] += 1
        if amt_col:
            rec["bill_cny"] += to_f(r.get(amt_col))
        # token 用量：仅当单位为 Token（百炼有的账单按 千Token/万Token 计价，先验单位再累加）
        if usage_col and unit_col and direction in ("input_token", "output_token"):
            unit = (r.get(unit_col) or "").strip().lower()
            raw = to_n(r.get(usage_col))
            if "token" in unit:
                mult = 1000.0 if ("千" in unit or "k" in unit.replace("token", "")) else 1.0
                t = tokens.setdefault(model, {})
                t[direction] = t.get(direction, 0.0) + raw * mult

    if not per_model:
        raise ValueError("账单解析结果为空——检查是否为「大模型服务平台百炼」账单明细导出")
    if period is None and len(months_seen) > 1:
        warn.append(f"账单含 {len(months_seen)} 个账期（{sorted(months_seen)[:3]}…），建议 --period 指定单一账期")
    return per_model, tokens, warn


# ── 本地 usage_records 侧 ──

def parse_model_map(path: str | None) -> dict:
    if not path:
        return {}
    with open(path, encoding="utf-8") as f:
        m = json.load(f)
    return {k.strip().lower(): v.strip() for k, v in m.items()}


def load_local(conn: sqlite3.Connection, period: str | None) -> tuple[dict, dict]:
    """本地按模型聚合（真实调用口径 act_cost>0——语义缓存命中未真正调用供应商，
    账单不会出现；注意 act_input_cost 在缓存命中行也非零，须与总额同口径过滤）。
    返回
      per_model: {model: {"usd": act_cost合计, "in_usd": act_input_cost合计,
                          "in": tokens, "out": tokens, "calls": n}}
      extra: {"cache_saved_usd", "est_input_usd", "all_rows"} 全量口径（含缓存命中）
    """
    where, params = "", []
    if period:
        where = "WHERE substr(created_at,1,7) = ?"
        params = [period]
    real_cond = ("AND" if where else "WHERE") + " act_cost > 0"
    per: dict[str, dict] = {}
    for r in conn.execute(
        f"""SELECT lower(model_name) AS m,
                   SUM(act_cost) AS usd, SUM(act_input_cost) AS in_usd,
                   SUM(input_tokens) AS tin, SUM(output_tokens) AS tout, COUNT(*) AS calls
            FROM usage_records {where} {real_cond}
            GROUP BY lower(model_name)""",
        params,
    ):
        per[r["m"]] = {
            "usd": r["usd"] or 0.0,
            "in_usd": r["in_usd"] or 0.0,
            "in": r["tin"] or 0,
            "out": r["tout"] or 0,
            "calls": r["calls"] or 0,
        }
    cs = conn.execute(
        f"""SELECT COALESCE(SUM(cache_saved_cost),0), COALESCE(SUM(est_input_cost),0), COUNT(*)
            FROM usage_records {where}""",
        params,
    ).fetchone()
    extra = {"cache_saved_usd": cs[0], "est_input_usd": cs[1], "all_rows": cs[2]}
    return per, extra


# ── 对账与报告 ──

def reconcile(bill: dict, bill_tokens: dict, local: dict, mapping: dict,
              rate: float, tolerance: float) -> dict:
    """逐模型对账。返回 report dict（文本与 JSON 共用同源数字）。"""
    def norm(name: str) -> str:
        n = name.strip().lower()
        for p in ("dashscope/", "openai/"):
            if n.startswith(p):
                n = n[len(p):]
        return n

    local_by_bill: dict[str, str] = {}  # 账单模型名 → 本地模型名
    local_names = set(local)
    for lm in local_names:
        local_by_bill[norm(lm)] = lm

    rows, matched_bill, matched_local = [], set(), set()
    for bm in sorted(bill):
        lm = mapping.get(bm) or local_by_bill.get(bm)
        b = bill[bm]
        if lm is None or lm not in local:
            rows.append({"model": bm, "status": "local_missing", "bill_cny": b["bill_cny"],
                         "rows": b["rows"]})
            continue
        matched_bill.add(bm)
        matched_local.add(lm)
        l = local[lm]
        l_cny = l["usd"] * rate
        dev = l_cny - b["bill_cny"]
        dev_pct = (dev / b["bill_cny"] * 100.0) if b["bill_cny"] > 1e-9 else (0.0 if abs(l_cny) < 1e-9 else 999.0)
        bt = bill_tokens.get(bm, {})
        rows.append({
            "model": bm, "local_model": lm,
            "bill_cny": round(b["bill_cny"], 6), "local_usd": round(l["usd"], 6),
            "local_cny": round(l_cny, 6), "dev_cny": round(dev, 6),
            "dev_pct": round(dev_pct, 2) if abs(dev_pct) < 999 else None,
            "calls": l["calls"],
            "tokens": {
                "bill_in": bt.get("input_token"), "bill_out": bt.get("output_token"),
                "local_in": l["in"], "local_out": l["out"],
            },
            "status": "ok" if abs(dev_pct) <= tolerance else "deviate",
        })

    for lm in sorted(local_names - matched_local):
        l = local[lm]
        if l["usd"] > 0:
            rows.append({"model": lm, "status": "bill_missing", "local_usd": round(l["usd"], 6),
                         "local_cny": round(l["usd"] * rate, 6), "calls": l["calls"]})

    ok = sum(1 for r in rows if r["status"] == "ok")
    tot_bill = sum(b["bill_cny"] for b in bill.values())
    tot_local_usd = sum(local[m]["usd"] for m in matched_local)
    tot_local_cny = tot_local_usd * rate
    tot_dev = tot_local_cny - tot_bill
    # 「已对账」= 匹配模型全部对平，且账单侧无落空的模型（有金额却查无记录 → 真实差异）
    has_gap = any(r["status"] == "deviate" or
                  (r["status"] == "local_missing" and r.get("bill_cny", 0) > 1e-9)
                  for r in rows)
    verdict = "已对账" if (matched_bill and not has_gap) else ("无重叠数据" if not rows else "有偏差")
    return {
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "cny_rate": rate, "tolerance_pct": tolerance,
        "total": {
            "bill_cny": round(tot_bill, 6), "local_usd": round(tot_local_usd, 6),
            "local_cny": round(tot_local_cny, 6), "dev_cny": round(tot_dev, 6),
            "dev_pct": round(tot_dev / tot_bill * 100.0, 2) if tot_bill > 1e-9 else None,
            "models_matched": len(matched_bill), "models_ok": ok, "verdict": verdict,
        },
        "models": rows,
    }


def print_report(rep: dict, extra: dict, warnings: list[str], bill_path: str, db_path: str) -> None:
    t = rep["total"]
    print(f"\n[bill] 对账报告  账单={bill_path}")
    print(f"[bill] 本地库={db_path}  汇率 1 USD = {rep['cny_rate']} CNY  阈值 ±{rep['tolerance_pct']}%")
    for w in warnings:
        print(f"[bill] ⚠ {w}")
    print(f"\n[bill] {'模型':<28}{'账单(CNY)':>12}{'本地折算(CNY)':>14}{'偏差(CNY)':>11}{'偏差率':>9}  状态")
    for r in rep["models"]:
        if r["status"] in ("ok", "deviate"):
            icon = "✓" if r["status"] == "ok" else "⚠"
            dp = f"{r['dev_pct']:.1f}%" if r.get("dev_pct") is not None else "n/a"
            print(f"[bill] {r['model']:<28}{r['bill_cny']:>12.4f}{r['local_cny']:>14.4f}"
                  f"{r['dev_cny']:>11.4f}{dp:>9}  {icon} {r['status']}")
            tk = r.get("tokens") or {}
            if tk.get("bill_in") is not None or tk.get("bill_out") is not None:
                def _s(v) -> str:
                    return "-" if v is None else str(int(v))
                print(f"[bill]   └ tokens in {_s(tk.get('local_in'))}/{_s(tk.get('bill_in'))}  "
                      f"out {_s(tk.get('local_out'))}/{_s(tk.get('bill_out'))}（本地/账单）")
        elif r["status"] == "local_missing":
            print(f"[bill] {r['model']:<28}{r['bill_cny']:>12.4f}{'-':>14}{'-':>11}{'-':>9}  ⚠ 本地无此模型记录")
        elif r["status"] == "bill_missing":
            print(f"[bill] {r['model']:<28}{'-':>12}{r['local_cny']:>14.4f}{'-':>11}{'-':>9}  ⚠ 账单无此模型（本账期外/他供应商？）")
    dp = f"{t['dev_pct']:.2f}%" if t.get("dev_pct") is not None else "n/a"
    print(f"\n[bill] 合计：账单 ¥{t['bill_cny']:.4f}  本地折算 ¥{t['local_cny']:.4f}  "
          f"偏差 ¥{t['dev_cny']:.4f}（{dp}）  匹配 {t['models_matched']} 模型（对平 {t['models_ok']}）")
    print(f"[bill] 全量口径：语义缓存节省 ${extra['cache_saved_usd']:.6f}  "
          f"输入侧 est/act 分列 est=${extra['est_input_usd']:.6f}")
    print(f"[bill] 结论：{t['verdict']}")


def main() -> int:
    ap = argparse.ArgumentParser(description="百炼账单 × usage_records 对账（N3.c）")
    ap.add_argument("--bill", required=True, help="百炼账单详情导出 CSV 路径")
    ap.add_argument("--db", help="SQLite 库路径（默认 data/lloom.db）")
    ap.add_argument("--period", help="账期 YYYY-MM，两侧同筛")
    ap.add_argument("--cny-rate", type=float,
                    default=float(os.environ.get("LLOOM_CNY_RATE", "7.2")),
                    help="USD→CNY 汇率（默认 7.2 或 LLOOM_CNY_RATE）")
    ap.add_argument("--model-map", help="JSON 映射 {账单模型名: 本地模型名}")
    ap.add_argument("--tolerance", type=float, default=5.0, help="单模型偏差率阈值 %%（默认 5）")
    ap.add_argument("--save", action="store_true", help="报告写 <data>/reconcile_last.json")
    ap.add_argument("--json", action="store_true", help="输出机器可读 JSON")
    args = ap.parse_args()

    bill_path, db_path = args.bill, resolve_db(args.db)
    if not os.path.exists(bill_path):
        print(f"[bill] 账单文件不存在：{bill_path}", file=sys.stderr)
        return 2
    if not os.path.exists(db_path):
        print(f"[bill] 库不存在：{db_path}", file=sys.stderr)
        return 2

    try:
        bill, bill_tokens, warnings = parse_bill(bill_path, args.period)
    except ValueError as e:
        print(f"[bill] 账单解析失败：{e}", file=sys.stderr)
        return 2

    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    local, extra = load_local(conn, args.period)
    rep = reconcile(bill, bill_tokens, local, parse_model_map(args.model_map),
                    args.cny_rate, args.tolerance)
    rep["period"] = args.period
    rep["extra"] = {k: round(v, 6) for k, v in extra.items()}

    if args.json:
        print(json.dumps(rep, ensure_ascii=False))
    else:
        print_report(rep, extra, warnings, bill_path, db_path)

    if args.save:
        # 路径安全：目录先 resolve 定死，文件名是常量；回读校验 parent 恒为数据目录
        from pathlib import Path

        data_dir = Path(resolve_data_dir(db_path)).resolve()
        out = data_dir / "reconcile_last.json"
        if out.parent != data_dir:
            raise SystemExit(f"refusing to write outside data dir: {data_dir}")
        with out.open("w", encoding="utf-8") as f:
            json.dump(rep, f, ensure_ascii=False, indent=2)
        if not args.json:
            print(f"[bill] 已保存：{out}（UsagePage「已对账」徽标数据源）")

    if rep["total"]["verdict"] == "已对账":
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
