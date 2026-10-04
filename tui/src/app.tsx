// LLooM TUI app — OpenCode-style layout.
// Top tab bar, main content area, bottom status.

import { createSignal, onMount, onCleanup, Show } from "solid-js"
import { theme } from "./theme"
import { Home } from "./routes/home"
import { Session } from "./routes/session"
import { Models } from "./routes/models"
import { Usage } from "./routes/usage"
import { Settings } from "./routes/settings"
import { DialogProvider } from "./ui/dialog"
import { useBindings } from "@opentui/keymap/solid"
import { startHealthPolling, serverConnected } from "./health"

export type Route = "home" | "session" | "models" | "usage" | "settings"

const TABS: { key: Route; label: string; keyHint: string }[] = [
  { key: "home", label: "概览", keyHint: "1" },
  { key: "session", label: "对话", keyHint: "2" },
  { key: "models", label: "模型", keyHint: "3" },
  { key: "usage", label: "用量", keyHint: "4" },
  { key: "settings", label: "设置", keyHint: "5" },
]

export const [route, setRoute] = createSignal<Route>("home")
export const [activeSessionId, setActiveSessionId] = createSignal<string | null>(null)

// Pending query passed from Home to Session (set before switching route).
export const [initialQuery, setInitialQuery] = createSignal<string | null>(null)

// Set while a modal dialog is open; dialog bindings take priority over pages.
export const [dialogOpen, setDialogOpen] = createSignal(false)

// Registered by index.tsx so App's Ctrl+C binding can quit cleanly.
let quitHandler: (() => void) | null = null
export function setQuitHandler(fn: () => void) {
  quitHandler = fn
}
function quit() {
  quitHandler?.()
}

export function App() {
  const [status, setStatus] = createSignal("")

  // Global health polling runs for the whole app regardless of the active page.
  onMount(() => {
    const dispose = startHealthPolling(30000)
    onCleanup(dispose)
  })

  // Global keys via keymap: Tab cycles pages, Ctrl+C quits.
  useBindings(() => ({
    enabled: () => !dialogOpen(),
    bindings: [
      {
        key: "tab",
        cmd: () => {
          const cur = TABS.findIndex((t) => t.key === route())
          setRoute(TABS[(cur + 1) % TABS.length].key)
          setStatus("")
        },
        desc: "Switch page",
      },
      {
        key: "ctrl+c",
        cmd: () => quit(),
        desc: "Quit",
      },
      ...TABS.map((tab) => ({ key: tab.keyHint, cmd: () => { setRoute(tab.key); setStatus("") }, desc: `Go to ${tab.label}` })),
    ],
  }))

  const pageLabel = () => {
    if (route() === "session" && activeSessionId()) return ` Chat ${activeSessionId()!.slice(0, 8)} `
    return ` ${TABS.find((t) => t.key === route())?.label} `
  }

  return (
    <DialogProvider>
      <box flexDirection="column" backgroundColor={theme.background} width="100%" height="100%">
      {/* Top tab bar */}
      <box
        flexDirection="row"
        paddingLeft={2}
        gap={1}
        border={["bottom"]}
        borderColor={theme.border}
      >
        <text fg={theme.primary} attributes={1}>LLooM</text>
        <text fg={theme.border}>│</text>
        {TABS.map((t) => (
          <text
            fg={route() === t.key ? theme.primary : theme.textMuted}
            attributes={route() === t.key ? 1 : 0}
            onMouseUp={() => {
              setRoute(t.key)
              setStatus("")
            }}
          >
            {t.key === route() ? `[${t.keyHint}] ${t.label}` : ` ${t.keyHint}  ${t.label}`}
          </text>
        ))}
      </box>

      {/* Main content */}
      <box flexGrow={1} minHeight={0}>
        <Show when={route() === "home"}><Home setStatus={setStatus} /></Show>
        <Show when={route() === "session"}><Session setStatus={setStatus} /></Show>
        <Show when={route() === "models"}><Models setStatus={setStatus} /></Show>
        <Show when={route() === "usage"}><Usage setStatus={setStatus} /></Show>
        <Show when={route() === "settings"}><Settings setStatus={setStatus} /></Show>
      </box>

      {/* Bottom status line */}
      <box
        flexShrink={0}
        paddingLeft={2}
        paddingRight={2}
        border={["top"]}
        borderColor={theme.border}
        backgroundColor={theme.backgroundPanel}
      >
        <text fg={serverConnected() ? theme.textMuted : theme.error}>
          {serverConnected()
            ? status() || `${pageLabel()}  Tab/1-5 切换 · ↑↓ 选择 · Enter 操作 · Ctrl+C 退出`
            : "● 服务器连接断开 — 请确认 lloom-server (:7861) 已启动"}
        </text>
      </box>
      </box>
    </DialogProvider>
  )
}
