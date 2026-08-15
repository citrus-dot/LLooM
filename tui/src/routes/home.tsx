// Home route — centered logo, service status, and a prompt (OpenCode-style).

import { createSignal, onMount, Show } from "solid-js"
import { theme } from "../theme"
import { getServicesStatus, type ServicesStatus } from "../api"
import { setRoute, setActiveSessionId } from "../app"

const LOGO = [
  " ██╗     ██╗      ██████╗  ██████╗ ███╗   ███╗",
  " ██║     ██║     ██╔═══██╗██╔═══██╗████╗ ████║",
  " ██║     ██║     ██║   ██║██║   ██║██╔████╔██║",
  " ███████╗███████╗╚██████╔╝╚██████╔╝██║╚██╔╝██║",
  " ╚══════╝╚══════╝ ╚═════╝  ╚═════╝ ╚═╝ ╚═╝ ╚═╝",
]

export function Home(props: { setStatus: (s: string) => void }) {
  const [services, setServices] = createSignal<ServicesStatus | null>(null)
  const [prompt, setPrompt] = createSignal("")

  onMount(async () => {
    try {
      setServices(await getServicesStatus())
    } catch (e) {
      props.setStatus(`无法连接服务器: ${e} — 请先启动 lloom-server`)
    }
  })

  const healthy = () => services()?.healthy ?? 0
  const total = () => services()?.total ?? 0

  const submit = () => {
    const q = prompt().trim()
    if (!q) return
    setActiveSessionId(null)
    setRoute("session")
    // pass the query to session via a module signal
    import("../app").then((m) => m.setInitialQuery(q))
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
          value={prompt()}
          onContentChange={(v: string) => setPrompt(v)}
          onSubmit={() => submit()}
          placeholder="Ask anything... 按 Enter 发送，Tab 切换页面"
          width="100%"
        />
      </box>

      <box height={1} />

      {/* Service status */}
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

      <box height={1} />

      <text fg={theme.textDim}>通过 REST 连接 lloom-server (:7861) · 模型路由 · 成本追踪 · 安全过滤</text>
    </box>
  )
}
