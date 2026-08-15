// DialogForm — multi-field form dialog (OpenCode-style).
// Tab cycles fields, Enter submits, Esc cancels. Each field is a textarea row.

import { createSignal, createEffect, onMount, For } from "solid-js"
import { theme } from "../theme"
import type { TextareaRenderable } from "@opentui/core"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "./dialog"

export type FormField = {
  key: string
  label: string
  placeholder?: string
  required?: boolean
  default?: string
}

export function DialogForm(props: {
  title: string
  fields: FormField[]
  onConfirm?: (values: Record<string, string>) => void
}) {
  const dialog = useDialog()
  const [index, setIndex] = createSignal(0)
  const [values, setValues] = createSignal<Record<string, string>>({})
  let refs: (TextareaRenderable | undefined)[] = []

  const setValue = (k: string, v: string) => setValues((prev) => ({ ...prev, [k]: v }))

  createEffect(() => {
    refs[index()]?.focus()
  })
  onMount(() => {
    refs[index()]?.focus()
  })

  const submit = () => {
    const vals = values()
    const missing = props.fields.find((f) => f.required && !(vals[f.key] ?? "").trim())
    if (missing) {
      setIndex(props.fields.indexOf(missing))
      return
    }
    props.onConfirm?.(vals)
    dialog.close()
  }

  useBindings(() => ({
    enabled: () => dialog.isOpen(),
    bindings: [
      {
        key: "up",
        cmd: () => {
          if (props.fields.length > 0) setIndex((index() - 1 + props.fields.length) % props.fields.length)
          refs[index()]?.focus()
        },
        desc: "Previous field",
      },
      {
        key: "down",
        cmd: () => {
          if (props.fields.length > 0) setIndex((index() + 1) % props.fields.length)
          refs[index()]?.focus()
        },
        desc: "Next field",
      },
      {
        key: "tab",
        cmd: () => {
          if (props.fields.length > 0) setIndex((index() + 1) % props.fields.length)
          refs[index()]?.focus()
        },
        desc: "Next field",
      },
    ],
  }))

  return (
    <box paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1} flexDirection="column">
      <box flexDirection="row" justifyContent="space-between">
        <text attributes={1} fg={theme.text}>{props.title}</text>
        <text fg={theme.textMuted} onMouseUp={() => dialog.close()}>esc</text>
      </box>
      <box height={1} />
      <For each={props.fields}>
        {(f, i) => (
          <box flexDirection="row" gap={1} paddingBottom={1}>
            <text fg={i() === index() ? theme.primary : theme.textMuted} width={16} flexShrink={0}>
              {i() === index() ? "▸ " : "  "}{f.label}
            </text>
            <textarea
              ref={(r: TextareaRenderable) => {
                refs[i()] = r
              }}
              initialValue={values()[f.key] ?? f.default ?? ""}
              placeholder={f.placeholder ?? f.label}
              placeholderColor={theme.textMuted}
              textColor={theme.text}
              focusedTextColor={theme.text}
              minHeight={1}
              width="100%"
              onContentChange={() => setValue(f.key, refs[i()]?.plainText ?? "")}
              onSubmit={() => submit()}
            />
          </box>
        )}
      </For>
      <box height={1} />
      <box flexDirection="row" gap={2}>
        <text fg={theme.textMuted}>Tab/↑↓ 切换字段</text>
        <text fg={theme.textMuted}>⏎ 提交</text>
        <text fg={theme.textMuted}>esc 取消</text>
      </box>
    </box>
  )
}
