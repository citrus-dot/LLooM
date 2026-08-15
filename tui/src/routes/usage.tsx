// Usage route — spend stats + usage table, OpenCode-style.

import { createSignal, onMount } from "solid-js"
import { theme } from "../theme"
import type { ScrollBoxRenderable } from "@opentui/core"
import { getStats, getUsage, getBudgets, setBudget, checkBudget, deleteBudget, getModels, type UsageRow, type Model } from "../api"
import { dialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "../ui/dialog"
import { Button, StatCard, Table } from "../ui"

export function Usage(props: { setStatus: (s: string) => void }) {
  const [rows, setRows] = createSignal<UsageRow[]>([])
  const [spend, setSpend] = createSignal(0)
  const [modelCount, setModelCount] = createSignal(0)
  const [models, setModels] = createSignal<Model[]>([])
  const [budgets, setBudgets] = createSignal<{ scope: string; scope_id: string; max_budget: number; duration?: string }[]>([])
  const [checked, setChecked] = createSignal<Record<string, { within: boolean; spent: number; max: number }>>({})
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)
  const dialog = useDialog()
  let scrollRef: ScrollBoxRenderable | undefined

  const loadBudgets = async () => {
    try {
      const b = await getBudgets()
      setBudgets(b.budgets)
      // Check each budget for overspend status.
      const map: Record<string, { within: boolean; spent: number; max: number }> = {}
      for (const budget of b.budgets) {
        try {
          const r = await checkBudget(budget.scope, budget.scope_id)
          map[`${budget.scope}/${budget.scope_id}`] = {
            within: r.within_budget,
            spent: r.spent,
            max: r.budget?.max_budget ?? budget.max_budget,
          }
        } catch {}
      }
      setChecked(map)
    } catch (e) {
      props.setStatus(`预算加载失败: ${e}`)
    }
  }

  const addBudget = () => {
    dialog.form("设置预算", {
      fields: [
        { key: "scope", label: "范围", placeholder: "user/model", required: true },
        { key: "scope_id", label: "范围 ID", placeholder: "如 default 或模型名", required: true },
        { key: "max_budget", label: "上限 ($)", placeholder: "如 10", required: true },
        { key: "duration", label: "周期", placeholder: "30d/7d/1d" },
      ],
      onConfirm: async (vals) => {
        await submitBudget({
          scope: vals.scope.trim(),
          scope_id: vals.scope_id.trim(),
          max_budget: parseFloat(vals.max_budget) || 0,
          duration: vals.duration.trim() || "30d",
        })
      },
    })
  }

  const editBudget = (b: { scope: string; scope_id: string; max_budget: number; duration?: string }) => {
    dialog.form(`编辑预算 ${b.scope}/${b.scope_id}`, {
      fields: [
        { key: "max_budget", label: "上限 ($)", placeholder: String(b.max_budget), default: String(b.max_budget), required: true },
        { key: "duration", label: "周期", placeholder: b.duration ?? "30d", default: b.duration ?? "30d" },
      ],
      onConfirm: async (vals) => {
        await submitBudget({ scope: b.scope, scope_id: b.scope_id, max_budget: parseFloat(vals.max_budget) || 0, duration: vals.duration.trim() || "30d" })
      },
    })
  }

  const submitBudget = async (vals: { scope: string; scope_id: string; max_budget: number; duration?: string }) => {
    try {
      await setBudget(vals.scope, vals.scope_id, vals.max_budget, vals.duration ?? "30d")
      props.setStatus("✓ 预算已保存")
      await loadBudgets()
    } catch (e) {
      props.setStatus(`保存预算失败: ${e}`)
    }
  }

  const budgetMenu = (b: { scope: string; scope_id: string; max_budget: number; duration?: string }) => {
    dialog.menu(`预算 ${b.scope}/${b.scope_id}`, {
      items: [
        { title: "编辑", desc: "修改上限/周期", onSelect: () => editBudget(b) },
        {
          title: "删除",
          desc: "移除该预算",
          danger: true,
          onSelect: async () => {
            try {
              await deleteBudget(b.scope, b.scope_id)
              props.setStatus(`✓ 已删除预算 ${b.scope}/${b.scope_id}`)
              await loadBudgets()
            } catch (e) {
              props.setStatus(`删除预算失败: ${e}`)
            }
          },
        },
      ],
    })
  }

  onMount(async () => {
    try {
      const [s, u, b, m] = await Promise.all([getStats(), getUsage(), getBudgets(), getModels()])
      setRows(u.usage)
      setSpend(u.total_spend ?? s.total_spend ?? 0)
      setModelCount(s.model_count ?? 0)
      setModels(m.models)
      setBudgets(b.budgets)
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
    // Check each budget for overspend (independent of list load).
    try {
      const map: Record<string, { within: boolean; spent: number; max: number }> = {}
      for (const budget of budgets()) {
        try {
          const r = await checkBudget(budget.scope, budget.scope_id)
          map[`${budget.scope}/${budget.scope_id}`] = {
            within: r.within_budget,
            spent: r.spent,
            max: r.budget?.max_budget ?? budget.max_budget,
          }
        } catch {}
      }
      setChecked(map)
    } catch {}
  })

  useBindings(() => ({
    enabled: () => !dialogOpen(),
    bindings: [
      {
        key: "up",
        cmd: () => {
          const n = rows().length
          if (n === 0) return
          setSelIdx((selIdx() - 1 + n) % n)
        },
        desc: "Previous row",
      },
      {
        key: "down",
        cmd: () => {
          const n = rows().length
          if (n === 0) return
          setSelIdx((selIdx() + 1) % n)
        },
        desc: "Next row",
      },
    ],
  }))

  return (
    <box flexDirection="column" flexGrow={1} minHeight={0}>
      <scrollbox
        flexGrow={1}
        minHeight={0}
        paddingLeft={2}
        paddingRight={2}
        paddingTop={1}
        paddingBottom={1}
      >
      {/* Stat cards */}
      <box flexDirection="row" gap={2} paddingBottom={1}>
        <StatCard flexGrow value={`$${spend().toFixed(4)}`} label="累计花费" tone="primary" />
        <StatCard flexGrow value={String(modelCount())} label="可用模型" tone="success" />
        <StatCard flexGrow value={String(budgets().length)} label="预算数" tone="secondary" />
      </box>

      {/* Usage */}
      <text fg={theme.textMuted} attributes={1}>用量明细</text>
      <Table
        columns={[
          { title: "模型", width: "30%", render: (r, { selected }) => <text fg={selected ? theme.background : theme.text} attributes={selected ? 1 : 0}>{r.model_name}</text> },
          { title: "输入", width: "14%", render: (r, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{r.total_input_tokens}</text> },
          { title: "输出", width: "14%", render: (r, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{r.total_output_tokens}</text> },
          { title: "请求", width: "12%", render: (r, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{r.request_count}</text> },
          { title: "缓存", width: "12%", render: (r, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{r.cache_hits ?? 0}</text> },
          { title: "花费", render: (r, { selected }) => <text fg={selected ? theme.background : theme.warning}>${r.total_cost.toFixed(4)}</text> },
        ]}
        rows={rows()}
        selectedIndex={selIdx()}
        hoverIndex={hoverIdx()}
        onHover={setHoverIdx}
        onSelect={setSelIdx}
        emptyText="暂无用量数据"
      />
      <box height={1} />

      {/* Model pricing */}
      <text fg={theme.textMuted} attributes={1}>模型定价 ($/1K tokens)</text>
      <Table
        columns={[
          { title: "模型", width: "30%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.text}>{m.name}</text> },
          { title: "提供商", width: "15%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{m.provider}</text> },
          { title: "输入", width: "25%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>${(m.input_cost_per_token * 1000).toFixed(6)}</text> },
          { title: "输出", render: (m, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>${(m.output_cost_per_token * 1000).toFixed(6)}</text> },
        ]}
        rows={models()}
        emptyText="暂无模型定价"
      />
      <box height={1} />

      {/* Budgets */}
      <box flexDirection="row" gap={1} paddingBottom={1}>
        <text fg={theme.textMuted} attributes={1}>预算</text>
        <Button variant="primary" onClick={() => addBudget()}>设置</Button>
        <Button variant="ghost" onClick={() => loadBudgets()}>刷新</Button>
      </box>
      <Table
        columns={[
          {
            title: "范围",
            width: "30%",
            render: (b) => <text fg={theme.text} attributes={1}>{b.scope}/{b.scope_id}</text>,
          },
          {
            title: "已用 / 上限",
            width: "25%",
            render: (b) => {
              const c = checked()[`${b.scope}/${b.scope_id}`]
              const within = c?.within ?? true
              return <text fg={within ? theme.text : theme.error}>${(c?.spent ?? 0).toFixed(4)} / ${(c?.max ?? b.max_budget).toFixed(2)}</text>
            },
          },
          {
            title: "进度",
            render: (b) => {
              const c = checked()[`${b.scope}/${b.scope_id}`]
              const within = c?.within ?? true
              const spent = c?.spent ?? 0
              const max = c?.max ?? b.max_budget
              const pct = max > 0 ? Math.min((spent / max) * 100, 100) : 0
              const barWidth = 30
              const filled = Math.round((pct / 100) * barWidth)
              return (
                <box flexDirection="row">
                  <text fg={within ? theme.success : theme.error}>{"█".repeat(filled)}</text>
                  <text fg={theme.border}>{"░".repeat(barWidth - filled)}</text>
                  <text fg={within ? theme.success : theme.error} paddingLeft={1}>{pct.toFixed(0)}%</text>
                </box>
              )
            },
          },
        ]}
        rows={budgets()}
        onRowUp={(b, evt) => { if (evt?.button === 2) budgetMenu(b) }}
        emptyText="未设置预算（点「设置」或 lloom-cli budgets set）"
      />
      <box height={1} />
      </scrollbox>
    </box>
  )
}
