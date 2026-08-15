// LLooM TUI — SolidJS + OpenTUI, REST-driven.
// Entry: create renderer, keymap, mount app.
// Key handling goes through @opentui/keymap (OpenCode-style): textarea input is
// managed by registerManagedTextareaLayer, page/dialog navigation via useBindings.

import { render } from "@opentui/solid"
import { createCliRenderer } from "@opentui/core"
import { KeymapProvider } from "@opentui/keymap/solid"
import { createDefaultOpenTuiKeymap } from "@opentui/keymap/opentui"
import { registerManagedTextareaLayer } from "@opentui/keymap/addons/opentui"
import { App, setQuitHandler } from "./app"

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

// destroy() restores the terminal (show cursor / leave alternate screen) via
// async stdout flush. process.exit immediately would truncate that restore,
// leaving the shell with a hidden cursor. Give the restore time to flush.
renderer.once("destroy", () => {
  setTimeout(() => process.exit(0), 100)
})

// App's Ctrl+C binding calls this to quit cleanly.
setQuitHandler(() => destroyRenderer())

const keymap = createDefaultOpenTuiKeymap(renderer)

// Managed textarea input: OpenCode registers the default edit-buffer commands
// and bindings here, so Enter submits / Shift+Enter newlines through the keymap.
registerManagedTextareaLayer(keymap, renderer, {
  enabled: () => {
    const editor = renderer.currentFocusedEditor as unknown as { constructor: { name: string } } | null | undefined
    return !!editor && editor.constructor.name === "TextareaRenderable"
  },
  bindings: [
    { key: "return", cmd: "input.submit" },
    { key: "shift+return", cmd: "input.newline" },
  ],
})

await render(
  () => (
    <KeymapProvider keymap={keymap}>
      <App />
    </KeymapProvider>
  ),
  renderer,
)

process.on("SIGHUP", () => destroyRenderer())
process.on("SIGINT", () => destroyRenderer())
