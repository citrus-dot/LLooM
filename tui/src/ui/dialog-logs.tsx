// DialogLogs — OpenCode-style modal for viewing service logs.
// Scrollable log tail, r refresh / Esc close.

import { createSignal, onMount, Show } from "solid-js"
import { theme } from "../theme"
import { useDialog } from "./dialog"

export function DialogLogs(props: {
  title: string
  logs?: string
  onRefresh?: () => string | Promise<string>
}) {
  const dialog = useDialog()
  const [content, setContent] = createSignal(props.logs ?? "")
  const [refreshing, setRefreshing] = createSignal(false)

  const refresh = async () => {
    if (!props.onRefresh || refreshing()) return
    setRefreshing(true)
    try {
      setContent((await props.onRefresh()) ?? "")
    } finally {
      setRefreshing(false)
    }
  }

  onMount(() => {
    void refresh()
  })

  return (
    <box paddingLeft={2} paddingRight={2} flexDirection="column">
      <box flexDirection="row" justifyContent="space-between">
        <text attributes={1} fg={theme.text}>{props.title}</text>
        <text fg={theme.textMuted} onMouseUp={() => dialog.close()}>esc</text>
      </box>
      <box height={1} />
      <scrollbox
        maxHeight={20}
        paddingLeft={1}
        paddingRight={1}
        backgroundColor={theme.backgroundElement}
      >
        <Show when={content() || !refreshing()} fallback={<text fg={theme.textDim}>加载中...</text>}>
          <text fg={theme.textMuted} wrapMode="word">
            {content() || "(暂无日志)"}
          </text>
        </Show>
      </scrollbox>
      <box paddingBottom={1} paddingTop={1} flexDirection="row" gap={1}>
        <text fg={theme.text} onMouseUp={() => refresh()}>r</text>
        <text fg={theme.textMuted}>刷新</text>
        <text fg={theme.textMuted}>·</text>
        <text fg={theme.textMuted}>esc 关闭</text>
      </box>
    </box>
  )
}
