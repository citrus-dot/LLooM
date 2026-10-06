#!/usr/bin/env python3
"""validate.py — LLooM OrchestrationBench canonical workflow validator（O-Day 9）。

角色：方案 16-OrchestrationBench §七 的 canonical object 入口闸门——
  外部 benchmark adapter → canonical workflow → (validator) → evaluator/simulator。
零成本红线：纯 stdlib、无网络、无 LLM。

校验分层（错误码是 O-Day 14 报告归因的稳定键，不得改名）：
  结构层   SCHEMA_*           必填/类型/枚举（对应 workflow.schema.json 声明的子集，手写不引 jsonschema）
  图语义层 DUP_NODE / SELF_EDGE / DUP_EDGE / UNKNOWN_NODE_REF / DEPENDS_EDGE_MISMATCH
                             节点 id 唯一、自环/重边拒收、edge 端点存在、
                             depends_on 必须与 data 边互洽（双通道冗余互验，防 adapter 写飘）
  DAG 层   CYCLE              data+control 边构成的有向图必须无环（resource 不参与）
  波次层   WAVE_PARTITION / WAVE_ORDER
                             waves 必须是节点精确分区；每节点全部 data/control 依赖落在更早波次
                             （同波依赖即非法——生产 dependency_waves 的 Kahn 分层语义）
  语义层   EMPTY_NODES / UNSOLVABLE_WITH_NODES / BAD_MODEL_REF
                             非 unsolvable 必须有节点；unsolvable 必须空节点（正确行为=拒绝计划）；
                             model 非 null 时必须是 slug 形态并在 model_pool（若提供）

用法：
  python validate.py manifests/fixtures/                     # 目录递归
  python validate.py manifest/fixtures/fixture_chain.json    # 单文件
  python -m unittest tests.test_validate                     # 单测

import 侧：
  from validate import validate_workflow, WorkflowIssue
  issues = validate_workflow(obj, model_pool={"weak_m", "mid_m", "strong_m"})
"""

from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

SCHEMA_VERSION = 1

TASK_TYPES = ("simple_qa", "coding", "math_logic", "complex_reasoning", "general")
EDGE_TYPES = ("data", "control", "resource")
AGG_TYPES = ("final_synthesis", "none", "unsolvable")
NODE_ID_RE = re.compile(r"^[A-Za-z0-9_.-]+$")
SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9_]*$")


@dataclass(frozen=True)
class WorkflowIssue:
    """单条校验结果；code ∈ 上列错误码，path 为 JSON 点路径（报告层直接展示）。"""

    code: str
    message: str
    path: str = ""

    def __str__(self) -> str:  # pragma: no cover - 仅 CLI 展示
        loc = f" @ {self.path}" if self.path else ""
        return f"[{self.code}]{loc} {self.message}"


def _type_ok(value: object, spec: str) -> bool:
    if spec == "object":
        return isinstance(value, dict)
    if spec == "array":
        return isinstance(value, list)
    if spec == "string":
        return isinstance(value, str)
    if spec == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if spec == "null":
        return value is None
    raise ValueError(f"未支持的类型校验 {spec}")  # 内部编程错误


def _err(issues: list[WorkflowIssue], code: str, path: str, msg: str) -> None:
    issues.append(WorkflowIssue(code, msg, path))


# ── 结构层 ──────────────────────────────────────────────────────────────

_WF_REQUIRED = ("workflow_id", "schema_version", "root_goal", "nodes", "edges", "aggregation")
_NODE_REQUIRED = ("id", "description", "task_type", "depends_on")
_EDGE_REQUIRED = ("from", "to", "type")


def _validate_structure(wf: object, issues: list[WorkflowIssue]) -> None:
    if not isinstance(wf, dict):
        _err(issues, "SCHEMA_TYPE", "", f"顶层必须是 object，实际 {type(wf).__name__}")
        return
    for key in _WF_REQUIRED:
        if key not in wf:
            _err(issues, "SCHEMA_MISSING_FIELD", key, f"必填字段缺失: {key}")
    if "schema_version" in wf and wf["schema_version"] != SCHEMA_VERSION:
        _err(issues, "SCHEMA_TYPE", "schema_version", f"只认 {SCHEMA_VERSION}，实际 {wf['schema_version']!r}")
    for key in ("workflow_id", "root_goal"):
        v = wf.get(key)
        if v is not None and (not isinstance(v, str) or not v.strip()):
            _err(issues, "SCHEMA_TYPE", key, "必须是非空 string")
    if isinstance(wf.get("workflow_id"), str) and wf["workflow_id"] and " " in wf["workflow_id"]:
        _err(issues, "SCHEMA_TYPE", "workflow_id", "workflow_id 不应含空格（slug 形态）")

    nodes = wf.get("nodes")
    if nodes is not None:
        if not isinstance(nodes, list):
            _err(issues, "SCHEMA_TYPE", "nodes", "必须是 array")
        else:
            for i, node in enumerate(nodes):
                _validate_node(node, f"nodes[{i}]", issues)
    edges = wf.get("edges")
    if edges is not None and not isinstance(edges, list):
        _err(issues, "SCHEMA_TYPE", "edges", "必须是 array")
        return
    if isinstance(edges, list):
        for i, edge in enumerate(edges):
            _validate_edge(edge, f"edges[{i}]", issues)
    agg = wf.get("aggregation")
    if agg is not None:
        if not isinstance(agg, dict):
            _err(issues, "SCHEMA_TYPE", "aggregation", "必须是 object")
        elif "type" not in agg:
            _err(issues, "SCHEMA_MISSING_FIELD", "aggregation.type", "必填字段缺失: type")
        elif agg["type"] not in AGG_TYPES:
            _err(issues, "SCHEMA_TYPE", "aggregation.type", f"必须是 {AGG_TYPES} 之一，实际 {agg['type']!r}")
    waves = wf.get("waves")
    if waves is not None and not isinstance(waves, list):
        _err(issues, "SCHEMA_TYPE", "waves", "必须是 array 或 null")


def _validate_node(node: object, path: str, issues: list[WorkflowIssue]) -> None:
    if not isinstance(node, dict):
        _err(issues, "SCHEMA_TYPE", path, "node 必须是 object")
        return
    for key in _NODE_REQUIRED:
        if key not in node:
            _err(issues, "SCHEMA_MISSING_FIELD", f"{path}.{key}", f"必填字段缺失: {key}")
    nid = node.get("id")
    if nid is not None:
        if not isinstance(nid, str) or not nid:
            _err(issues, "SCHEMA_TYPE", f"{path}.id", "必须是非空 string")
        elif not NODE_ID_RE.match(nid):
            _err(issues, "SCHEMA_TYPE", f"{path}.id", f"id 只允许 [A-Za-z0-9_.-]，实际 {nid!r}")
    for key in ("description", "task_type"):
        v = node.get(key)
        if v is not None and not isinstance(v, str):
            _err(issues, "SCHEMA_TYPE", f"{path}.{key}", "必须是 string")
    if node.get("task_type") is not None and node.get("task_type") not in TASK_TYPES:
        _err(
            issues,
            "SCHEMA_TYPE",
            f"{path}.task_type",
            f"必须是 {TASK_TYPES} 之一，实际 {node.get('task_type')!r}（生产 router 词表）",
        )
    deps = node.get("depends_on")
    if deps is not None and (not isinstance(deps, list) or not all(isinstance(d, str) for d in deps)):
        _err(issues, "SCHEMA_TYPE", f"{path}.depends_on", "必须是 string 数组")
    for key in ("inputs", "outputs", "constraints"):
        v = node.get(key)
        if v is not None and (not isinstance(v, list) or not all(isinstance(x, str) for x in v)):
            _err(issues, "SCHEMA_TYPE", f"{path}.{key}", "必须是 string 数组")
    model = node.get("model", None)
    if model is not None and not isinstance(model, str):
        _err(issues, "SCHEMA_TYPE", f"{path}.model", "必须是 string(slug) 或 null")
    for key in ("estimated_output_tokens", "duration_ms"):
        v = node.get(key)
        if v is not None and (not _type_ok(v, "integer") or v < 1):
            _err(issues, "SCHEMA_TYPE", f"{path}.{key}", "必须是 >=1 的 integer")


def _validate_edge(edge: object, path: str, issues: list[WorkflowIssue]) -> None:
    if not isinstance(edge, dict):
        _err(issues, "SCHEMA_TYPE", path, "edge 必须是 object")
        return
    for key in _EDGE_REQUIRED:
        if key not in edge:
            _err(issues, "SCHEMA_MISSING_FIELD", f"{path}.{key}", f"必填字段缺失: {key}")
    for key in ("from", "to", "type"):
        v = edge.get(key)
        if v is not None and not isinstance(v, str):
            _err(issues, "SCHEMA_TYPE", f"{path}.{key}", "必须是 string")
    if edge.get("type") is not None and edge.get("type") not in EDGE_TYPES:
        _err(issues, "SCHEMA_TYPE", f"{path}.type", f"必须是 {EDGE_TYPES} 之一，实际 {edge.get('type')!r}")


# ── 图语义层 ────────────────────────────────────────────────────────────


def _validate_graph(wf: dict, issues: list[WorkflowIssue]) -> dict[str, dict]:
    """返回 node id → node 的映射（供上层复用）；图结构错误时返回已解析部分。"""
    nodes: dict[str, dict] = {}
    if not isinstance(wf.get("nodes"), list):
        return nodes
    for i, node in enumerate(wf["nodes"]):
        if not isinstance(node, dict) or not isinstance(node.get("id"), str):
            continue
        nid = node["id"]
        if nid in nodes:
            _err(issues, "DUP_NODE", f"nodes[{i}].id", f"节点 id 重复: {nid!r}")
            continue
        nodes[nid] = node

    edges = wf.get("edges") if isinstance(wf.get("edges"), list) else []
    seen_pairs: set[tuple[str, str, str]] = set()
    for i, edge in enumerate(edges):
        if not isinstance(edge, dict):
            continue
        src, dst, etype = edge.get("from"), edge.get("to"), edge.get("type")
        if not isinstance(src, str) or not isinstance(dst, str):
            continue
        if src not in nodes:
            _err(issues, "UNKNOWN_NODE_REF", f"edges[{i}].from", f"边起点 {src!r} 不在 nodes 中")
        if dst not in nodes:
            _err(issues, "UNKNOWN_NODE_REF", f"edges[{i}].to", f"边终点 {dst!r} 不在 nodes 中")
        if src == dst and src in nodes:
            _err(issues, "SELF_EDGE", f"edges[{i}]", f"自环边 {src!r}→{src!r}")
        key = (src, dst, str(etype))
        if key in seen_pairs:
            _err(issues, "DUP_EDGE", f"edges[{i}]", f"重复边 {src!r}→{dst!r} (type={etype})")
        seen_pairs.add(key)

    # depends_on 与 data 边互洽（双通道冗余，方案 §七 node.depends_on + edges）
    for nid, node in nodes.items():
        deps = node.get("depends_on") if isinstance(node.get("depends_on"), list) else []
        data_edges = {
            e.get("from")
            for e in edges
            if isinstance(e, dict) and e.get("to") == nid and e.get("type") == "data" and isinstance(e.get("from"), str)
        }
        dep_set = set(deps)
        if dep_set != data_edges:
            _err(
                issues,
                "DEPENDS_EDGE_MISMATCH",
                f"nodes[{nid}].depends_on",
                f"depends_on {sorted(dep_set)} 与 data 入边 {sorted(data_edges)} 不一致（两通道必须互洽）",
            )
        for d in dep_set:
            if d not in nodes:
                _err(issues, "UNKNOWN_NODE_REF", f"nodes[{nid}].depends_on", f"依赖 {d!r} 不在 nodes 中")
    return nodes


def _order_deps(nodes: dict[str, dict], edges: list) -> dict[str, set[str]]:
    """data+control 边的先序依赖（resource 不参与波次/环检查——纯标注）。"""
    order: dict[str, set[str]] = {nid: set() for nid in nodes}
    for e in edges:
        if not isinstance(e, dict):
            continue
        if e.get("type") in ("data", "control") and isinstance(e.get("from"), str) and isinstance(e.get("to"), str):
            if e["from"] in order and e["to"] in order:
                order[e["to"]].add(e["from"])
    for nid, node in nodes.items():
        deps = node.get("depends_on") if isinstance(node.get("depends_on"), list) else []
        for d in deps:
            if isinstance(d, str) and d in order:
                order[nid].add(d)
    return order


def _validate_dag(order: dict[str, set[str]], issues: list[WorkflowIssue]) -> None:
    """Kahn 消去判环；剩余节点即处于环上。"""
    indeg = {nid: len(deps) for nid, deps in order.items()}
    dependents: dict[str, list[str]] = {nid: [] for nid in order}
    for nid, deps in order.items():
        for d in deps:
            dependents[d].append(nid)
    queue = [nid for nid, d in indeg.items() if d == 0]
    removed = 0
    while queue:
        cur = queue.pop()
        removed += 1
        for nxt in dependents[cur]:
            indeg[nxt] -= 1
            if indeg[nxt] == 0:
                queue.append(nxt)
    if removed != len(order):
        cyclic = sorted(nid for nid, d in indeg.items() if d > 0)
        _err(issues, "CYCLE", "", f"存在依赖环，环上节点: {cyclic[:8]}")


# ── 波次层 ──────────────────────────────────────────────────────────────


def _validate_waves(wf: dict, nodes: dict[str, dict], order: dict[str, set[str]], issues: list[WorkflowIssue]) -> None:
    waves = wf.get("waves")
    if waves is None:
        return
    if not isinstance(waves, list):
        return  # 结构层已报
    seen: set[str] = set()
    wave_index: dict[str, int] = {}
    for wi, wave in enumerate(waves):
        if not isinstance(wave, list) or not all(isinstance(n, str) for n in wave):
            _err(issues, "SCHEMA_TYPE", f"waves[{wi}]", "波次必须是 string 数组")
            return
        if not wave:
            _err(issues, "WAVE_PARTITION", f"waves[{wi}]", "空波次（分区不允许空块）")
        for nid in wave:
            if nid not in nodes:
                _err(issues, "UNKNOWN_NODE_REF", f"waves[{wi}]", f"波次引用 {nid!r} 不在 nodes 中")
                continue
            if nid in seen:
                _err(issues, "WAVE_PARTITION", f"waves[{wi}]", f"节点 {nid!r} 重复出现在多个波次")
            seen.add(nid)
            wave_index[nid] = wi
    missing = sorted(set(nodes) - seen)
    if missing:
        _err(issues, "WAVE_PARTITION", "waves", f"节点未被任何波次覆盖: {missing[:8]}")
    for wi, wave in enumerate(waves):
        if not isinstance(wave, list):
            continue
        for nid in wave:
            for dep in sorted(order.get(nid, ())):
                dep_wave = wave_index.get(dep)
                if dep_wave is not None and dep_wave >= wi:
                    _err(
                        issues,
                        "WAVE_ORDER",
                        f"waves[{wi}]",
                        f"节点 {nid!r} 的依赖 {dep!r} 在波次 {dep_wave}（>= 当前 {wi}）——"
                        "生产 dependency_waves 要求依赖全部落在更早波次",
                    )


# ── 语义层 ──────────────────────────────────────────────────────────────


def _validate_semantics(wf: dict, nodes: dict[str, dict], issues: list[WorkflowIssue]) -> None:
    agg_type = wf.get("aggregation", {}).get("type") if isinstance(wf.get("aggregation"), dict) else None
    if agg_type == "unsolvable":
        if nodes:
            _err(issues, "UNSOLVABLE_WITH_NODES", "aggregation.type", "unsolvable 任务不允许携带 nodes（正确行为=拒绝计划）")
        return
    # final_synthesis 必须有节点；none 允许空（planning-only 任务登记：PlanningBench 类
    # 数据只有 goal+checklist，无可执行工作流，adapter 以 obligation-node 形态或空节点接入）
    if agg_type == "final_synthesis" and not nodes:
        _err(issues, "EMPTY_NODES", "nodes", "aggregation.type=final_synthesis 时必须有至少 1 个节点")
    pool = wf.get("meta", {}).get("model_pool") if isinstance(wf.get("meta"), dict) else None
    for nid, node in nodes.items():
        model = node.get("model")
        if model is None:
            continue
        if not SLUG_RE.match(str(model)):
            _err(issues, "BAD_MODEL_REF", f"nodes[{nid}].model", f"{model!r} 不是 RouterBench slug 形态")
        elif isinstance(pool, list) and model not in pool:
            _err(issues, "BAD_MODEL_REF", f"nodes[{nid}].model", f"{model!r} 不在 meta.model_pool 内")


# ── 入口 ────────────────────────────────────────────────────────────────


def validate_workflow(wf: object, *, strict_meta: bool = False) -> list[WorkflowIssue]:
    """校验一个 canonical workflow 对象，返回全部问题（空列表=合法）。

    strict_meta：要求 meta 带 source provenance（adapter 正式导入时开；fixture 可关）。
    """
    issues: list[WorkflowIssue] = []
    _validate_structure(wf, issues)
    if any(i.code.startswith("SCHEMA_") for i in issues):
        return issues  # 结构已坏，图/波次层无法可靠执行
    assert isinstance(wf, dict)
    nodes = _validate_graph(wf, issues)
    order = _order_deps(nodes, wf.get("edges") if isinstance(wf.get("edges"), list) else [])
    _validate_dag(order, issues)
    _validate_waves(wf, nodes, order, issues)
    _validate_semantics(wf, nodes, issues)
    if strict_meta:
        meta = wf.get("meta")
        if not isinstance(meta, dict) or not meta.get("source_benchmark") or not meta.get("source_id"):
            _err(issues, "SCHEMA_MISSING_FIELD", "meta", "strict 模式要求 meta.source_benchmark 与 meta.source_id（provenance 红线）")
    return issues


def load_workflow_file(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv:
        print(__doc__)
        return 2
    strict = "--strict" in argv
    paths = [a for a in argv if not a.startswith("--")]
    # 路径收口（Mimosa 约束）：CLI 路径 resolve 后必须落在本工具目录内，仓外一律拒收，
    # 消除任意路径读取面；fixture/adapter 产物校验均在本目录内，收口不影响用途
    root = Path(__file__).resolve().parent
    all_ok = True
    for raw in paths:
        p = Path(raw).resolve()
        try:
            p.relative_to(root)
        except ValueError:
            print(f"✗ 拒收路径 {p}（validator CLI 只接受 benchmarks/orchestration/ 内的路径）")
            all_ok = False
            continue
        # 不用 glob 模式展开（命令行参数进 glob 有模式注入面）；iterdir + 后缀过滤等价且污点面为零
        files = sorted([p] if p.is_file() else (x for x in p.iterdir() if x.suffix == ".json" and x.is_file()))
        if not files:
            print(f"✗ 未找到待校验文件: {p}")
            all_ok = False
            continue
        for f in files:
            try:
                wf = load_workflow_file(f)
            except json.JSONDecodeError as e:
                print(f"FAIL {f.name}: JSON 解析失败: {e}")
                all_ok = False
                continue
            issues = validate_workflow(wf, strict_meta=strict)
            wid = wf.get("workflow_id", "?") if isinstance(wf, dict) else "?"
            if issues:
                all_ok = False
                print(f"FAIL {f.name} ({wid})")
                for i in issues:
                    print(f"    {i}")
            else:
                print(f"PASS {f.name} ({wid})")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
