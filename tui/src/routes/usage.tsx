// Usage route — spend stats + usage table, OpenCode-style.

import { createSignal, onMount, onCleanup } from "solid-js"
import { theme } from "../theme"
import { getStats, getUsage, getBudgets, type UsageRow } from "../api"
import { setNavHandler, navHandler } from "../app"

export function Usage(props: { setStatus: (s: string) => void }) {
  const [rows, setRows] = createSignal<UsageRow[]>([])
  const [spend, setSpend] = createSignal(0)
  const [modelCount, setModelCount] = createSignal(0)
  const [budgets, setBudgets] = createSignal<{ scope: string; scope_id: string; max_budget: number }[]>([])
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)

  onMount(async () => {
    try {
      const [s, u, b] = await Promise.all([getStats(), getUsage(), getBudgets()])
      setRows(u.usage)
      setSpend(u.total_spend ?? s.total_spend ?? 0)
      setModelCount(s.model_count ?? 0)
      setBudgets(b.budgets)
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
    setNavHandler((key) => {
      const n = rows().length
      if (n === 0) return
      if (key === "up" || key === "down") {
        const dir = key === "down" ? 1 : -1
        setSelIdx((selIdx() + dir + n) % n)
      }
    })
  })

  onCleanup(() => {
    if (navHandler()) setNavHandler(null)
  })

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
              <text fg={isSel ? theme.background : theme.warning}>${r.total_cost.toFixed(4)}</text>
            </box>
          )
        })}
      </box>

      {/* Budgets */}
      <box flexDirection="column" paddingTop={1}>
        <text fg={theme.textMuted} attributes={1}>预算</text>
        {budgets().length === 0 && <text fg={theme.textDim}>  未设置预算（用 lloom-cli budgets set）</text>}
        {budgets().map((b) => (
          <text fg={theme.text}>  {b.scope}/{b.scope_id} · ${b.max_budget.toFixed(2)}</text>
        ))}
      </box>
    </box>
  )
}
