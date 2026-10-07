#!/usr/bin/env python3
"""builder_v02.py — SystemBench v0.2 Semantic Workflow case Builder（24 号 D1-c' / 25 号 §五、§十一）。

v0.2 身份边界（25 号 §五）：
  Primary   = Semantic Set（真实 LLM decomposition，headline 候选）
  Secondary = Control Set（llmrb exact query 拼装，role=control/diagnostic，不与 semantic 混合平均）

Semantic node frozen prompt（S12.2 执行决策，报告存档）：
  prompt = root_goal + "\n\n---\n## 子任务\n" + description
  - benchmark 适配：生产 ai_client.rs:526 现状只发 description（root_goal 数据不随发），
    已记为 production observation；benchmark 层嵌入 root_goal 使节点可独立执行。
  - artifact_dependent 节点的上游产物注入（生产「前置任务结果：」格式）留 S12.6 stage 0
    以 qwen-plus 参考执行冻结后追加，届时 prompt_hash 重算（本 builder 标 artifact_injection=pending）。

红线：
  - semantic node binding mode 只准 `semantic_matrix_exact`（23 号 §五 1.1 封闭枚举；禁止 fuzzy/nearest）；
  - control node binding mode = `llmrouterbench_exact`（与 v0.1 同法，逐字 prompt_hash 验证）；
  - v0.1 Frozen 资产（system_cases.jsonl / bindings.jsonl）只读不覆写；
  - split 同法新算（sha256 lloom-sbsplit:3407:{case_id}，20/80）——v0.2 = 新 benchmark identity。

产物：
  cases/system_cases_v02.jsonl   每 case 一行（workflow DAG + bindings + 标注占位 + split）
  cases/bindings_v02.jsonl       每 node 一行 binding 台账
  cases/builder_manifest_v02.json

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/adapters/builder_v02.py
"""

from __future__ import annotations

import hashlib
import json
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))  # 复用 v0.1 builder 的 llmrb 装载与 POOL 常量（不复制）
from builder import POOL, load_llmrb, slugify, split_of  # noqa: E402

S11_REPORT = Path("benchmarks/systembench/reports/s11_decomposition_pilot.json")
S12_REPORT = Path("benchmarks/systembench/reports/s12_decomposition_batch.json")
V01_BINDINGS = Path("benchmarks/systembench/cases/bindings.jsonl")
OUT_CASES = Path("benchmarks/systembench/cases/system_cases_v02.jsonl")
OUT_BIND = Path("benchmarks/systembench/cases/bindings_v02.jsonl")
OUT_MANIFEST = Path("benchmarks/systembench/cases/builder_manifest_v02.json")

N_CONTROL = 10
CONTROL_TEMPLATES = [  # 小形态轮转（chain2/fanout3/diamond），control 只承担回归/对照职责
    ("chain2", 2, [(0, 1)]),
    ("fanout3", 4, [(0, 1), (0, 2), (0, 3)]),
    ("diamond", 4, [(0, 1), (0, 2), (1, 3), (2, 3)]),
]
SEMANTIC_PROMPT_TEMPLATE = "{root_goal}\n\n---\n## 子任务\n{description}"

DOMAIN_KEYWORDS = [  # case_level.semantic_domain 初判（S12.3 标注可修正；低风险元数据）
    ("planning", ["规划", "调度", "排", "方案", "计划", "安排"]),
    ("data", ["数据", "统计", "分析", "报表", "汇总"]),
    ("coding", ["代码", "编程", "函数", "脚本", "接口", "bug"]),
    ("content", ["文案", "撰写", "文章", "内容", "翻译"]),
    ("research", ["调研", "研究", "文献", "综述"]),
    ("tool", ["工具", "系统", "平台", "自动化"]),
]


def domain_of(goal: str) -> list[str]:
    hits = [name for name, kws in DOMAIN_KEYWORDS if any(k in goal for k in kws)]
    return hits or ["planning"]  # PlanningBench 语料缺省 planning


def complexity_bucket(n_nodes: int, depth: int) -> str:
    if n_nodes <= 2:
        return "simple"
    if n_nodes <= 5:
        return "complex" if depth >= 4 else "medium"
    return "complex"


def graph_stats(nodes: list[dict], edges: list[dict]) -> tuple[int, int, float]:
    deps: dict[str, list[str]] = {n["id"]: [] for n in nodes}
    for e in edges:
        deps[e["to"]].append(e["from"])
    depth_memo: dict[str, int] = {}

    def d(nid: str) -> int:
        if nid not in depth_memo:
            depth_memo[nid] = 1 + max((d(x) for x in deps[nid]), default=0)
        return depth_memo[nid]

    for n in nodes:
        d(n["id"])
    widths: dict[int, int] = defaultdict(int)
    for n in nodes:
        widths[depth_memo[n["id"]]] += 1
    return max(depth_memo.values()), max(widths.values()), round(len(edges) / max(len(nodes), 1), 2)


def sem_case(idx: int, seed: dict, provenance: str) -> dict:
    """真实 decomposition seed → v0.2 SystemCase（semantic_matrix_exact binding）。"""
    wf = seed["workflow"]
    case_id = f"sb_v02_sem_{idx:03d}"
    nodes = []
    for n in wf["nodes"]:
        prompt = SEMANTIC_PROMPT_TEMPLATE.format(root_goal=wf["root_goal"], description=n["description"])
        deps = list(n.get("depends_on") or [])
        nodes.append({
            "node_id": n["id"],
            "description": n["description"],
            "task_type": n.get("task_type", "general"),
            "estimated_output_tokens": n.get("estimated_output_tokens"),
            "prompt": prompt,
            "outcome_binding": {
                "mode": "semantic_matrix_exact",
                "source_id": f"{case_id}/{n['id']}",
                "prompt_hash": hashlib.sha256(prompt.encode("utf-8")).hexdigest(),
            },
            "dependency_semantics": "artifact_dependent" if deps else "independent",
            "artifact_injection": "pending_s12_6" if deps else "none",
            # S12.3 标注占位（builder 不做语义判定）
            "semantic_role": "pending_annotation",
            "atomic_ok": "pending_annotation",
            "artifact_contract": "pending_annotation",
        })
    edges = [{"from": e["from"], "to": e["to"], "type": "data"} for e in wf["edges"]]
    depth, width, density = graph_stats(wf["nodes"], edges)
    return {
        "system_case_id": case_id,
        "case_role": "headline_candidate",
        "case_class": complexity_bucket(len(nodes), depth),
        "root_goal": wf["root_goal"],
        "source": {
            "kind": "real_decomposition",
            "provenance": provenance,  # s11_seed | s12_expansion
            "source_sample_id": seed["sample_id"],
            "planner_model": "qwen-plus",
            "planner_prompt": "生产 decompose 原文（ai_client.rs:350）",
        },
        "group_id": seed["sample_id"],  # 每 root_goal 独立组（23 号 §十五 workflow isolation）
        "structure_template": "real_decomposition",
        "morphology": {"n_nodes": len(nodes), "n_edges": len(edges),
                       "depth": depth, "max_width": width, "dependency_density": density},
        "semantic_domain": domain_of(wf["root_goal"]),
        "nodes": nodes,
        "edges": edges,
        "dependency_semantics": "artifact_dependent" if edges else "independent",
        "replay_level": "L1",
        "fidelity": "L1",
        "fidelity_note": ("frozen-prompt replay：node prompt 全冻结（root_goal+description），"
                          "per-(prompt,model) outcome exact binding；artifact_dependent 节点的"
                          "严格 artifact 传播反事实属 L2（S13）范围"),
        "completion_condition": {
            "required_nodes": [n["node_id"] for n in nodes],  # S12.3 标注后按 role 修正
            "optional_nodes": [],
            "success_condition": "workflow_completion",
            "aggregation_condition": "pending_annotation",
        },
        "split": split_of(case_id, 3407, 0.2),
    }


def ctl_case(idx: int, template: tuple, chunk_ids: list[str]) -> tuple[dict, list[str]]:
    """llmrb exact query 拼装 control case（mechanism 与 v0.1 同；fresh sample，role=control）。"""
    name, n_nodes, edge_pairs = template
    case_id = f"sb_v02_ctl_{idx:03d}"
    nodes = []
    for k, sid in enumerate(chunk_ids):
        meta = samples_by_id[sid]
        nodes.append({
            "node_id": f"n{k + 1}",
            "prompt": meta["prompt"],
            "task_type": meta["task_type"],
            "source_dataset_id": meta["dataset_id"],
            "outcome_binding": {
                "mode": "llmrouterbench_exact",
                "source_id": sid,
                "prompt_hash": hashlib.sha256(meta["prompt"].encode("utf-8")).hexdigest(),
            },
            "dependency_semantics": "independent",
            "semantic_role": "control_fixture",   # diagnostic 节点：不做 semantic 标注
            "atomic_ok": True,
            "artifact_contract": {"produces": [], "consumes": []},
        })
    edges = [{"from": f"n{a + 1}", "to": f"n{b + 1}", "type": "data"} for a, b in edge_pairs]
    depth, width, density = graph_stats(
        [{"id": n["node_id"]} for n in nodes], edges)
    case = {
        "system_case_id": case_id,
        "case_role": "control",
        "case_class": complexity_bucket(len(nodes), depth),
        "root_goal": f"[llmrb assembly control] {name} over {len(nodes)} exact-bound llmrb nodes",
        "source": {"kind": "llmrb_assembly", "provenance": "v02_control_build"},
        "group_id": case_id,
        "structure_template": name,
        "morphology": {"n_nodes": len(nodes), "n_edges": len(edges),
                       "depth": depth, "max_width": width, "dependency_density": density},
        "semantic_domain": ["control"],
        "nodes": nodes,
        "edges": edges,
        "dependency_semantics": "independent",
        "replay_level": "L1",
        "fidelity": "L1_strict",
        "fidelity_note": "llmrb exact query 逐字绑定 frozen outcome；严格 L1 counterfactual（v0.1 同法）",
        "completion_condition": {
            "required_nodes": [n["node_id"] for n in nodes],
            "optional_nodes": [],
            "success_condition": "workflow_completion",
            "aggregation_condition": "all_required",
        },
        "split": split_of(case_id, 3407, 0.2),
    }
    used_ids = [n["outcome_binding"]["source_id"] for n in nodes]
    return case, used_ids


samples_by_id: dict[str, dict] = {}


def main() -> int:
    s11 = json.loads(S11_REPORT.read_text(encoding="utf-8"))["semantic_workflow_seeds"]
    s12 = json.loads(S12_REPORT.read_text(encoding="utf-8"))["semantic_workflow_seeds"]
    semantic = ([(s, "s11_seed") for s in sorted(s11, key=lambda x: x["sample_id"])]
                + [(s, "s12_expansion") for s in sorted(s12, key=lambda x: x["sample_id"])])

    # control：v0.1 已用 sample 排除（v0.1 frozen 资产不受扰）
    v01_used: set[str] = set()
    for line in V01_BINDINGS.read_text(encoding="utf-8").split("\n"):
        if line.strip():
            v01_used.add(json.loads(line)["source_sample_id"])
    pool_samples = load_llmrb(Path("benchmarks/routerbench/normalized/llmrouterbench"))
    global samples_by_id
    samples_by_id = pool_samples
    order = sorted((sid for sid in pool_samples if sid not in v01_used),
                   key=lambda s: hashlib.sha256(f"lloom-sbselect:3407:{s}".encode()).digest())
    cursor = 0

    cases = []
    bind_rows = []
    for i, (seed, prov) in enumerate(semantic):
        c = sem_case(i + 1, seed, prov)
        cases.append(c)
    t_idx = 0
    for i in range(N_CONTROL):
        while True:
            template = CONTROL_TEMPLATES[t_idx % len(CONTROL_TEMPLATES)]
            t_idx += 1
            need = template[1]
            if cursor + need <= len(order):
                break
        chunk = order[cursor:cursor + need]
        cursor += need
        c, used = ctl_case(i + 1, template, chunk)
        cases.append(c)
        v01_used.update(used)  # case 间不共用 sample

    # binding 台账 + 校验（prompt_hash 一致性，Gate S-2 同法）
    for c in cases:
        for n in c["nodes"]:
            h = hashlib.sha256(n["prompt"].encode("utf-8")).hexdigest()
            if h != n["outcome_binding"]["prompt_hash"]:
                raise SystemExit(f"✗ binding 破损: {c['system_case_id']}/{n['node_id']}")
            bind_rows.append({
                "case_id": c["system_case_id"],
                "node_id": n["node_id"],
                "source_benchmark": ("llmrouterbench" if n["outcome_binding"]["mode"] == "llmrouterbench_exact"
                                     else "systembench_semantic"),
                "source_id": n["outcome_binding"]["source_id"],
                "prompt_hash": n["outcome_binding"]["prompt_hash"],
                "binding_mode": n["outcome_binding"]["mode"],
                "dependency_semantics": n["dependency_semantics"],
            })

    OUT_CASES.write_text(
        "\n".join(json.dumps(c, ensure_ascii=False) for c in cases) + "\n", encoding="utf-8")
    OUT_BIND.write_text(
        "\n".join(json.dumps(b, ensure_ascii=False) for b in bind_rows) + "\n", encoding="utf-8")

    by_role = defaultdict(int)
    by_class = defaultdict(int)
    by_split = defaultdict(int)
    by_prov = defaultdict(int)
    dep_sem = defaultdict(int)
    for c in cases:
        by_role[c["case_role"]] += 1
        by_class[c["case_class"]] += 1
        by_split[c["split"]] += 1
        if c["case_role"] == "headline_candidate":
            by_prov[c["source"]["provenance"]] += 1
        for n in c["nodes"]:
            dep_sem[n["dependency_semantics"]] += 1
    manifest = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "benchmark_identity": "lloom-systembench-v0.2-candidate（构建层；freeze 于 S12.7）",
        "n_cases": len(cases),
        "n_nodes": sum(len(c["nodes"]) for c in cases),
        "by_case_role": dict(by_role),
        "by_case_class": dict(by_class),
        "by_split": dict(by_split),
        "semantic_by_provenance": dict(by_prov),
        "nodes_by_dependency_semantics": dict(dep_sem),
        "pool": POOL,
        "split_rule": "workflow-group 哈希桶 sha256('lloom-sbsplit:3407:{case_id}')，20/80（v0.1 同法，case_id 已换 → 新 identity）",
        "prompt_composition": {
            "semantic": "root_goal + '\\n\\n---\\n## 子任务\\n' + description（frozen；artifact_dependent 节点 S12.6 stage 0 注入冻结参考产物后重算 prompt_hash）",
            "control": "llmrb 真实 query 逐字（v0.1 同法）",
            "production_observation": "生产 ai_client.rs:526 子任务 prompt 现状只发 description、不嵌 root_goal 数据——benchmark 层嵌入目标数据使节点可执行；是否回修生产为独立决策，不属本 builder",
        },
        "notes": [
            "v0.1 Frozen 资产只读未覆写；control 使用 v0.1 未消费的 llmrb sample（无重叠）",
            "semantic node outcome_binding.mode=semantic_matrix_exact（source_id=case/node），matrix 于 S12.6 采集、S12.7 冻结",
            "headline 候选全部来自真实 decomposition（25 号 §五 D1-c'：Primary=Semantic Set）",
            "case_class 按 node_count+depth（spec complexity_bucket）；所有真实 decomposition 为 3-6 节点",
        ],
    }
    OUT_MANIFEST.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"✓ {OUT_CASES}  ({manifest['n_cases']} cases / {manifest['n_nodes']} nodes)")
    print(f"✓ {OUT_BIND}")
    print(f"  role={dict(by_role)} class={dict(by_class)} split={dict(by_split)}")
    print(f"  semantic provenance={dict(by_prov)} | node dep_sem={dict(dep_sem)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
