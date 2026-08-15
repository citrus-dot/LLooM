// LLooM TUI — SolidJS + OpenTUI, REST-driven.
// Entry: create renderer, mount app. Global keys: Tab=switch page, Ctrl+C=quit.
// Text input (Enter/arrows/typing) is handled by the focused textarea itself.

import { render } from "@opentui/solid"
import { createCliRenderer } from "@opentui/core"
import { App, route, setRoute, navHandler } from "./app"

const renderer = await createCliRenderer({
  targetFps: 60,
  exitOnCtrlC: false,
  useMouse: true,
})

function destroyRenderer() {
  renderer.setTerminalTitle("")
  if (renderer.isDestroyed) return
  renderer.destroy()
}

renderer.once("destroy", () => process.exit(0))

const PAGES = ["home", "session", "models", "usage", "settings"] as const

// Only global keys: Tab switches page; Ctrl+C quits.
// Everything else goes to the focused component (textarea handles input itself).
renderer.keyInput.on("keypress", (evt) => {
  if (evt.name === "c" && evt.ctrl) {
    destroyRenderer()
    return
  }
  if (evt.name === "tab") {
    const cur = PAGES.indexOf(route() as (typeof PAGES)[number])
    setRoute(PAGES[(cur + 1) % PAGES.length])
    return
  }
  // Arrow keys / escape: dispatch to current page nav handler (for list nav).
  const h = navHandler()
  if (h && ["up", "down", "left", "right", "escape", "esc"].includes(evt.name)) {
    h(evt.name, evt.shift, evt.ctrl)
  }
})

await render(() => <App />, renderer)

process.on("SIGHUP", () => destroyRenderer())
process.on("SIGINT", () => destroyRenderer())
