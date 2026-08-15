// Home route — centered logo, service status, and a prompt (OpenCode-style).

import { createSignal, createEffect, onMount, onCleanup, Show } from "solid-js"
import { theme } from "../theme"
import { getServicesStatus, getStats, type ServicesStatus } from "../api"
import { setRoute, setActiveSessionId, setInitialQuery } from "../app"
import type { TextareaRenderable } from "@opentui/core"
import { StatCard } from "../ui"

const LOGO = [
  " ██╗     ██╗      ██████╗  ██████╗ ███╗   ███╗",
  " ██║     ██║     ██╔═══██╗██╔═══██╗████╗ ████║",
  " ██║     ██║     ██║   ██║██║   ██║██╔████╔██║",
  " ███████╗███████╗╚██████╔╝╚██████╔╝██║╚██╔╝██║",
  " ╚══════╝╚══════╝ ╚═════╝  ╚═════╝ ╚═╝ ╚═╝ ╚═╝",
]

export function Home(props: { setStatus: (s: string) => void }) {
  const [services, setServices] = createSignal<ServicesStatus | null>(null)
  const [stats, setStats] = createSignal<{ total_spend: number; model_count: number; cache_enabled: boolean } | null>(null)
  const [prompt, setPrompt] = createSignal("")
  let inputRef: TextareaRenderable | undefined

  createEffect(() => {
    if (inputRef && !inputRef.focused) inputRef.focus()
  })
  onMount(() => {
    if (inputRef && !inputRef.focused) inputRef.focus()
  })

  const refresh = async () => {
    try {
      const [svc, st] = await Promise.all([getServicesStatus(), getStats()])
      setServices(svc)
      setStats(st)
    } catch (e) {
      props.setStatus(`无法连接服务器: ${e} — 请先启动 lloom-server`)
    }
  }

  onMount(async () => {
    await refresh()
    if (inputRef && !inputRef.focused) inputRef.focus()
    const t = setInterval(refresh, 30000)
    onCleanup(() => clearInterval(t))
  })

  const healthy = () => services()?.healthy ?? 0
  const total = () => services()?.total ?? 0

  const submit = () => {
    const q = (inputRef?.plainText ?? "").trim()
    if (!q) return
    setPrompt("")
    inputRef?.clear()
    setInitialQuery(q)
    setActiveSessionId(null)
    setRoute("session")
  }

  return (
    <box flexGrow={1} flexDirection="column" alignItems="center" justifyContent="center" paddingLeft={2} paddingRight={2}>
      {/* Logo */}
      <box flexDirection="column">
        {LOGO.map((line) => (
          <text fg={theme.primary} attributes={1}>{line}</text>
        ))}
      </box>

      <box height={1} />

      {/* Prompt */}
      <box
        width="70%"
        flexDirection="column"
        backgroundColor={theme.backgroundElement}
        border={["left"]}
        borderColor={theme.primary}
        paddingLeft={2}
        paddingRight={2}
        paddingTop={1}
        paddingBottom={1}
      >
        <textarea
          ref={(r: TextareaRenderable) => {
            inputRef = r
          }}
          onContentChange={() => setPrompt(inputRef?.plainText ?? "")}
          onSubmit={() => submit()}
          placeholder="Ask anything... 按 Enter 发送，Tab 切换页面"
          width="100%"
          keyBindings={[
            { name: "return", action: "submit" },
            { name: "return", shift: true, action: "newline" },
          ]}
        />
      </box>

      <box height={1} />

      {/* Service status + stats */}
      <box flexDirection="row" gap={2} flexWrap="wrap" justifyContent="center">
        <text fg={theme.textMuted}>服务健康</text>
        <text fg={healthy() === total() ? theme.success : theme.error} attributes={1}>
          {healthy()}/{total()}
        </text>
        <text fg={theme.textMuted}>·</text>
        <Show when={services()} fallback={<text fg={theme.textMuted}>加载中...</text>}>
          {services()!.services.map((s) => (
            <>
              <text fg={theme.textMuted}>{s.name}:</text>
              <text fg={s.healthy ? theme.success : theme.error}>{s.healthy ? "●" : "○"}</text>
            </>
          ))}
        </Show>
      </box>
      <box flexDirection="row" gap={2} flexWrap="wrap" justifyContent="center" paddingTop={1}>
        <StatCard value={`$${(stats()?.total_spend ?? 0).toFixed(6)}`} label="累计花费" tone="warning" />
        <StatCard value={String(stats()?.model_count ?? 0)} label="模型" />
        <StatCard value={stats()?.cache_enabled ? "启用" : "未启用"} label="语义缓存" tone={stats()?.cache_enabled ? "success" : "text"} />
      </box>

      <box height={1} />

      <text fg={theme.textDim}>通过 REST 连接 lloom-server (:7861) · 模型路由 · 成本追踪 · 安全过滤</text>
    </box>
  )
}
