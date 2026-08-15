// DialogMenu — option-picker dialog (right-click menus), OpenCode DialogSelect-style.
// ▸ indicator, ↑↓ select, Enter confirm, Esc close.

import { createSignal, For, Show } from "solid-js"
import { theme } from "../theme"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "./dialog"

export type MenuItem = {
  title: string
  desc?: string
  danger?: boolean
  onSelect?: () => void
}

export function DialogMenu(props: { title: string; items: MenuItem[]; onClose?: () => void }) {
  const dialog = useDialog()
  const [index, setIndex] = createSignal(0)

  const choose = (i: number) => {
    const item = props.items[i]
    if (!item) return
    const fn = item.onSelect
    dialog.close()
    fn?.()
  }

  useBindings(() => ({
    enabled: () => dialog.isOpen(),
    bindings: [
      {
        key: "up",
        cmd: () => {
          const n = props.items.length
          if (n > 0) setIndex((index() - 1 + n) % n)
        },
        desc: "Previous",
      },
      {
        key: "down",
        cmd: () => {
          const n = props.items.length
          if (n > 0) setIndex((index() + 1) % n)
        },
        desc: "Next",
      },
      {
        key: "return",
        cmd: () => choose(index()),
        desc: "Confirm",
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
      <For each={props.items}>
        {(item, i) => (
          <box
            flexDirection="row"
            backgroundColor={i() === index() ? theme.primary : theme.backgroundPanel}
            paddingLeft={2}
            paddingRight={2}
            onMouseOver={() => setIndex(i())}
            onMouseUp={() => choose(i())}
          >
            <text fg={i() === index() ? theme.background : theme.textMuted} width={2} flexShrink={0}>
              {i() === index() ? "▸" : " "}
            </text>
            <text fg={i() === index() ? theme.background : item.danger ? theme.error : theme.text}>
              {item.title}
              <Show when={item.desc}>
                <span style={{ fg: i() === index() ? theme.background : theme.textMuted }}> {item.desc}</span>
              </Show>
            </text>
          </box>
        )}
      </For>
      <box height={1} />
      <box flexDirection="row" gap={2}>
        <text fg={theme.textMuted}>↑↓ 选择</text>
        <text fg={theme.textMuted}>⏎ 确认</text>
        <text fg={theme.textMuted}>esc 取消</text>
      </box>
    </box>
  )
}
