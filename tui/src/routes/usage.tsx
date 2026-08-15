// Usage route — spend stats + usage table, OpenCode-style.

import { createSignal, onMount } from "solid-js"
import { theme } from "../theme"
import { getStats, getUsage, getBudgets, setBudget, checkBudget, getModels, type UsageRow, type Model } from "../api"
import { dialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "../ui/dialog"

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
        try {
          await setBudget(
            vals.scope.trim(),
            vals.scope_id.trim(),
            parseFloat(vals.max_budget) || 0,
            vals.duration.trim() || "30d",
          )
          props.setStatus("✓ 预算已设置")
          await loadBudgets()
        } catch (e) {
          props.setStatus(`设置预算失败: ${e}`)
        }
      },
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
    <box flexDirection="column" flexGrow={1} minHeight={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
      {/* Stat cards */}
      <box flexDirection="row" gap={2} paddingBottom={1}>
        <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left"]} borderColor={theme.primary} paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1}>
          <text fg={theme.primary} attributes={1}>${spend().toFixed(4)}</text>
          <text fg={theme.textMuted}>累计花费</text>
        </box>
        <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left"]} borderColor={theme.success} paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1}>
          <text fg={theme.success} attributes={1}>{modelCount()}</text>
          <text fg={theme.textMuted}>可用模型</text>
        </box>
        <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left"]} borderColor={theme.secondary} paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1}>
          <text fg={theme.secondary} attributes={1}>{budgets().length}</text>
          <text fg={theme.textMuted}>预算数</text>
        </box>
      </box>

      {/* Usage table */}
      <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left", "right"]} borderColor={theme.border} paddingTop={1} paddingBottom={1}>
        <box flexDirection="row" paddingLeft={3} paddingRight={3} paddingBottom={1}>
          <text fg={theme.textMuted} attributes={1} width="30%">模型</text>
          <text fg={theme.textMuted} attributes={1} width="15%">输入</text>
          <text fg={theme.textMuted} attributes={1} width="15%">输出</text>
          <text fg={theme.textMuted} attributes={1} width="10%">请求</text>
          <text fg={theme.textMuted} attributes={1} width="12%">缓存</text>
          <text fg={theme.textMuted} attributes={1}>花费</text>
        </box>
        {rows().length === 0 && <text fg={theme.textDim} paddingLeft={3}>  暂无用量数据</text>}
        {rows().map((r, i) => {
          const isSel = i === selIdx()
          const isHover = i === hoverIdx()
          return (
            <box
              flexDirection="row"
              backgroundColor={isSel ? theme.primary : isHover ? theme.backgroundElement : theme.backgroundPanel}
              paddingLeft={3}
              paddingRight={3}
              onMouseOver={() => setHoverIdx(i)}
              onMouseOut={() => setHoverIdx(null)}
              onMouseDown={() => setSelIdx(i)}
            >
              <text fg={isSel ? theme.background : theme.text} width="30%" attributes={isSel ? 1 : 0}>{r.model_name}</text>
              <text fg={isSel ? theme.background : theme.textMuted} width="15%">{r.total_input_tokens}</text>
              <text fg={isSel ? theme.background : theme.textMuted} width="15%">{r.total_output_tokens}</text>
              <text fg={isSel ? theme.background : theme.textMuted} width="10%">{r.request_count}</text>
              <text fg={isSel ? theme.background : theme.textMuted} width="12%">{r.cache_hits ?? 0}</text>
              <text fg={isSel ? theme.background : theme.warning}>${r.total_cost.toFixed(4)}</text>
            </box>
          )
        })}
      </box>

      {/* Model pricing */}
      <box flexDirection="column" paddingTop={1}>
        <text fg={theme.textMuted} attributes={1}>模型定价 ($/1K tokens)</text>
        <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left", "right"]} borderColor={theme.border} paddingTop={1} paddingBottom={1}>
          <box flexDirection="row" paddingLeft={3} paddingRight={3} paddingBottom={1}>
            <text fg={theme.textMuted} attributes={1} width="30%">模型</text>
            <text fg={theme.textMuted} attributes={1} width="15%">提供商</text>
            <text fg={theme.textMuted} attributes={1} width="25%">输入</text>
            <text fg={theme.textMuted} attributes={1}>输出</text>
          </box>
          {models().length === 0 && <text fg={theme.textDim} paddingLeft={3}>  暂无模型定价</text>}
          {models().map((m) => (
            <box flexDirection="row" paddingLeft={3} paddingRight={3}>
              <text fg={theme.text} width="30%">{m.name}</text>
              <text fg={theme.textMuted} width="15%">{m.provider}</text>
              <text fg={theme.textMuted} width="25%">${(m.input_cost_per_token * 1000).toFixed(6)}</text>
              <text fg={theme.textMuted}>${(m.output_cost_per_token * 1000).toFixed(6)}</text>
            </box>
          ))}
        </box>
      </box>

      {/* Budgets */}
      <box flexDirection="column" paddingTop={1}>
        <box flexDirection="row" gap={1}>
          <text fg={theme.textMuted} attributes={1}>预算</text>
          <text fg={theme.primary} onMouseUp={() => addBudget()}>[设置]</text>
          <text fg={theme.textMuted} onMouseUp={() => loadBudgets()}>[刷新]</text>
        </box>
        {budgets().length === 0 && <text fg={theme.textDim}>  未设置预算（点 [设置] 或 lloom-cli budgets set）</text>}
        {budgets().map((b) => {
          const c = checked()[`${b.scope}/${b.scope_id}`]
          const within = c?.within ?? true
          return (
            <box flexDirection="column" paddingLeft={1}>
              <text fg={within ? theme.text : theme.error}>
                {within ? "✓" : "✗"} {b.scope}/{b.scope_id} · 上限 ${b.max_budget.toFixed(2)}
                {c ? ` · 已用 $${c.spent.toFixed(4)}${c.spent > 0 ? ` (${((c.spent / c.max) * 100).toFixed(0)}%)` : ""}` : ""}
              </text>
            </box>
          )
        })}
      </box>
    </box>
  )
}
