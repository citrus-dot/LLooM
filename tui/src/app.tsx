// LLooM TUI app — OpenCode-style layout.
// Top tab bar, main content area, bottom status. Routes global nav to pages.

import { createSignal, Show } from "solid-js"
import { theme } from "./theme"
import { Home } from "./routes/home"
import { Session } from "./routes/session"
import { Models } from "./routes/models"
import { Usage } from "./routes/usage"
import { Settings } from "./routes/settings"

export type Route = "home" | "session" | "models" | "usage" | "settings"

const TABS: { key: Route; label: string }[] = [
  { key: "home", label: "Home" },
  { key: "session", label: "Chat" },
  { key: "models", label: "Models" },
  { key: "usage", label: "Usage" },
  { key: "settings", label: "Settings" },
]

export const [route, setRoute] = createSignal<Route>("home")
export const [activeSessionId, setActiveSessionId] = createSignal<string | null>(null)

// Global nav: each page registers a handler for arrow/enter keys.
type NavFn = (key: string, shift: boolean, ctrl: boolean) => void
export const [navHandler, setNavHandler] = createSignal<NavFn | null>(null)

export function App() {
  const [status, setStatus] = createSignal("")

  const pageLabel = () => {
    if (route() === "session" && activeSessionId()) return ` Chat ${activeSessionId()!.slice(0, 8)} `
    return ` ${TABS.find((t) => t.key === route())?.label} `
  }

  return (
    <box flexDirection="column" backgroundColor={theme.background} width="100%" height="100%">
      {/* Top tab bar */}
      <box
        flexDirection="row"
        paddingLeft={2}
        paddingTop={1}
        paddingBottom={1}
        gap={1}
        border={["bottom"]}
        borderColor={theme.border}
      >
        {TABS.map((t) => (
          <text
            fg={route() === t.key ? theme.primary : theme.textMuted}
            attributes={route() === t.key ? 1 : 0}
            onMouseUp={() => {
              setRoute(t.key)
              setStatus("")
            }}
          >
            {t.key === route() ? `▶ ${t.label}` : `  ${t.label}`}
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
        paddingTop={1}
        paddingBottom={1}
        border={["top"]}
        borderColor={theme.border}
        backgroundColor={theme.backgroundPanel}
      >
        <text fg={theme.textMuted}>
          {status() || "● LLooM · REST · 鼠标点击 · Tab 切换 · ↑↓/Enter 导航"}
        </text>
      </box>
    </box>
  )
}
