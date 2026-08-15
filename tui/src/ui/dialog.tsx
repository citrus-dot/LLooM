// Dialog layer — OpenCode-style modal overlay + context.
// dialog.prompt() opens a prompt dialog; dialog.close() closes it.
// Esc/Ctrl+C (keymap bindings) or clicking the backdrop closes it.

import { createContext, createSignal, Show, useContext, type JSX, type ParentProps } from "solid-js"
import { useRenderer, useTerminalDimensions } from "@opentui/solid"
import { RGBA } from "@opentui/core"
import { theme } from "../theme"
import { setDialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { DialogPrompt } from "./dialog-prompt"
import { DialogLogs } from "./dialog-logs"
import { DialogMenu } from "./dialog-menu"
import { DialogForm } from "./dialog-form"

export type DialogPromptOptions = {
  value?: string
  placeholder?: string
  onConfirm?: (value: string) => void
  onCancel?: () => void
}

export type DialogLogsOptions = {
  logs?: string
  onRefresh?: () => string | Promise<string>
}

export type DialogMenuOptions = {
  items: import("./dialog-menu").MenuItem[]
  onClose?: () => void
}

export type DialogFormOptions = {
  fields: import("./dialog-form").FormField[]
  onConfirm?: (values: Record<string, string>) => void
}

type DialogSpec =
  | { kind: "prompt"; title: string; options: DialogPromptOptions }
  | { kind: "logs"; title: string; options: DialogLogsOptions }
  | { kind: "menu"; title: string; options: DialogMenuOptions }
  | { kind: "form"; title: string; options: DialogFormOptions }

type DialogContextValue = {
  isOpen: () => boolean
  prompt: (title: string, options?: DialogPromptOptions) => void
  logs: (title: string, options?: DialogLogsOptions) => void
  menu: (title: string, options?: DialogMenuOptions) => void
  form: (title: string, options?: DialogFormOptions) => void
  close: () => void
}

const DialogContext = createContext<DialogContextValue>()

export function useDialog() {
  const ctx = useContext(DialogContext)
  if (!ctx) throw new Error("useDialog must be used inside <DialogProvider>")
  return ctx
}

// Module-level close hook so global key handling (index.tsx) can dismiss the
// dialog without reaching into the context. Registered by DialogProvider.
let closeDialogFn: (() => void) | null = null
export function closeDialog() {
  closeDialogFn?.()
}

export function DialogProvider(props: ParentProps) {
  const [spec, setSpec] = createSignal<DialogSpec | null>(null)

  const close = () => {
    const s = spec()
    setSpec(null)
    setDialogOpen(false)
    if (s && s.kind === "prompt") s.options.onCancel?.()
  }
  closeDialogFn = close

  const value: DialogContextValue = {
    isOpen: () => spec() !== null,
    prompt: (title, options) => {
      setDialogOpen(true)
      setSpec({ kind: "prompt", title, options: options ?? {} })
    },
    logs: (title, options) => {
      setDialogOpen(true)
      const o: DialogLogsOptions = options ?? {}
      setSpec({ kind: "logs", title, options: o })
    },
    menu: (title, options) => {
      setDialogOpen(true)
      const o: DialogMenuOptions = options ?? { items: [] }
      setSpec({ kind: "menu", title, options: o })
    },
    form: (title, options) => {
      setDialogOpen(true)
      const o: DialogFormOptions = options ?? { fields: [] }
      setSpec({ kind: "form", title, options: o })
    },
    close,
  }

  // While a dialog is open, Esc / Ctrl+C dismiss it (page bindings are disabled
  // via dialogOpen()).
  useBindings(() => ({
    enabled: () => spec() !== null,
    bindings: [
      { key: "escape", cmd: () => close(), desc: "Close dialog" },
      { key: "ctrl+c", cmd: () => close(), desc: "Close dialog" },
    ],
  }))

  return (
    <DialogContext.Provider value={value}>
      {props.children}
      <Show when={spec()}>
        {(s: () => DialogSpec) => (
          <DialogOverlay>
            <DialogPanel size={s().kind === "logs" ? "large" : "medium"}>
              <DialogContent spec={s()} onClose={close} />
            </DialogPanel>
          </DialogOverlay>
        )}
      </Show>
    </DialogContext.Provider>
  )
}

function DialogContent(props: { spec: DialogSpec; onClose: () => void }) {
  const s = props.spec
  if (s.kind === "prompt") {
    return (
      <DialogPrompt
        title={s.title}
        value={s.options.value ?? ""}
        placeholder={s.options.placeholder ?? "Enter text"}
        onConfirm={s.options.onConfirm}
        onCancel={props.onClose}
      />
    )
  }
  if (s.kind === "logs") {
    return <DialogLogs title={s.title} logs={s.options.logs ?? ""} onRefresh={s.options.onRefresh} />
  }
  if (s.kind === "menu") {
    return <DialogMenu title={s.title} items={s.options.items} onClose={props.onClose} />
  }
  return <DialogForm title={s.title} fields={s.options.fields} onConfirm={s.options.onConfirm} />
}

// ── Overlay + panel (OpenCode dialog.tsx style) ──

function DialogOverlay(props: ParentProps) {
  const renderer = useRenderer()
  const dimensions = useTerminalDimensions()
  const dialog = useDialog()
  return (
    <box
      onMouseDown={() => {}}
      onMouseUp={() => {
        // Clicking the backdrop closes the dialog.
        dialog.close()
      }}
      width={dimensions().width}
      height={dimensions().height}
      alignItems="center"
      position="absolute"
      zIndex={3000}
      paddingTop={Math.floor(dimensions().height / 4)}
      left={0}
      top={0}
      backgroundColor={RGBA.fromInts(0, 0, 0, 150)}
    >
      {props.children}
    </box>
  )
}

function DialogPanel(props: ParentProps & { size?: "medium" | "large" }) {
  const dimensions = useTerminalDimensions()
  return (
    <box
      onMouseUp={(e: { stopPropagation(): void }) => {
        e.stopPropagation()
      }}
      width={props.size === "large" ? 88 : 60}
      maxWidth={dimensions().width - 2}
      backgroundColor={theme.backgroundPanel}
      paddingTop={1}
    >
      {props.children}
    </box>
  )
}
