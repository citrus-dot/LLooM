// DialogPrompt — OpenCode dialog-prompt.tsx style: title row, textarea,
// Enter submits / Shift+Enter newline, Esc cancels.

import { createEffect, onMount, Show } from "solid-js"
import { theme } from "../theme"
import type { TextareaRenderable } from "@opentui/core"
import { useDialog, type DialogPromptOptions } from "./dialog"

export function DialogPrompt(props: {
  title: string
  value?: string
  placeholder?: string
  onConfirm?: (value: string) => void
  onCancel?: () => void
}) {
  const dialog = useDialog()
  let inputRef: TextareaRenderable | undefined

  const confirm = () => {
    props.onConfirm?.(inputRef?.plainText ?? "")
    dialog.close()
  }

  createEffect(() => {
    if (inputRef && !inputRef.focused) inputRef.focus()
  })

  onMount(() => {
    if (inputRef && !inputRef.focused) inputRef.focus()
    inputRef?.gotoBufferEnd?.()
  })

  return (
    <box paddingLeft={2} paddingRight={2} flexDirection="column" gap={1}>
      <box flexDirection="row" justifyContent="space-between">
        <text attributes={1} fg={theme.text}>{props.title}</text>
        <text fg={theme.textMuted} onMouseUp={() => dialog.close()}>esc</text>
      </box>
      <textarea
        ref={(r: TextareaRenderable) => { inputRef = r }}
        initialValue={props.value}
        placeholder={props.placeholder ?? "Enter text"}
        placeholderColor={theme.textMuted}
        textColor={theme.text}
        focusedTextColor={theme.text}
        width="100%"
        keyBindings={[
          { name: "return", action: "submit" },
          { name: "return", shift: true, action: "newline" },
        ]}
        onSubmit={() => confirm()}
      />
      <box paddingBottom={1} flexDirection="row" gap={1}>
        <text fg={theme.text}>⏎</text>
        <text fg={theme.textMuted}>提交</text>
        <text fg={theme.textMuted}>·</text>
        <text fg={theme.textMuted}>⇧⏎ 换行</text>
        <text fg={theme.textMuted}>·</text>
        <text fg={theme.textMuted}>esc 取消</text>
      </box>
    </box>
  )
}
