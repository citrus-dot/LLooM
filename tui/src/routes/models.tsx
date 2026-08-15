// Models route — model list with hover/click, OpenCode dialog-select style.

import { createSignal, onMount, onCleanup } from "solid-js"
import { theme } from "../theme"
import { getModels, deleteModel, type Model } from "../api"
import { setNavHandler, navHandler } from "../app"

export function Models(props: { setStatus: (s: string) => void }) {
  const [models, setModels] = createSignal<Model[]>([])
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)

  const refresh = async () => {
    try {
      setModels((await getModels()).models)
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
  }

  onMount(() => {
    refresh()
    setNavHandler((key) => {
      const n = models().length
      if (n === 0) return
      if (key === "up" || key === "down") {
        const dir = key === "down" ? 1 : -1
        setSelIdx((selIdx() + dir + n) % n)
      } else if (key === "d") {
        if (models()[selIdx()]) del(models()[selIdx()].name)
      }
    })
  })

  onCleanup(() => {
    if (navHandler()) setNavHandler(null)
  })

  const del = async (name: string) => {
    try {
      await deleteModel(name)
      await refresh()
      props.setStatus(`已删除 ${name}`)
    } catch (e) {
      props.setStatus(`删除失败: ${e}`)
    }
  }

  return (
    <box flexDirection="column" flexGrow={1} minHeight={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
      <box flexDirection="row" gap={1} paddingBottom={1}>
        <text fg={theme.textMuted} attributes={1}>模型管理</text>
        <text fg={theme.textMuted}>·</text>
        <text fg={theme.textMuted}>{models().length} 个</text>
        <text fg={theme.textMuted} onMouseUp={() => refresh()}>[刷新]</text>
      </box>

      <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left", "right"]} borderColor={theme.border} paddingTop={1} paddingBottom={1}>
        <box flexDirection="row" paddingLeft={3} paddingRight={3} paddingBottom={1}>
          <text fg={theme.textMuted} attributes={1} width="30%">名称</text>
          <text fg={theme.textMuted} attributes={1} width="15%">提供商</text>
          <text fg={theme.textMuted} attributes={1} width="40%">LiteLLM 模型</text>
          <text fg={theme.textMuted} attributes={1}>操作</text>
        </box>
        {models().length === 0 && <text fg={theme.textDim} paddingLeft={3}>  暂无模型</text>}
        {models().map((m, i) => {
          const isSel = i === selIdx()
          const isHover = i === hoverIdx()
          return (
            <box
              flexDirection="row"
              backgroundColor={isSel ? theme.primary : isHover ? theme.backgroundElement : theme.backgroundPanel}
              paddingLeft={3}
              paddingRight={3}
              onMouseOver={() => setHoverIdx(i)}
              onMouseOut={() => setHoverIdx(null)}
              onMouseDown={() => setSelIdx(i)}
            >
              <text fg={isSel ? theme.background : theme.text} width="30%" attributes={isSel ? 1 : 0}>{m.name}</text>
              <text fg={isSel ? theme.background : theme.textMuted} width="15%">{m.provider}</text>
              <text fg={isSel ? theme.background : theme.text} width="40%">{m.litellm_model}</text>
              <text fg={isSel ? theme.background : theme.error} onMouseUp={() => del(m.name)}>[删除]</text>
            </box>
          )
        })}
      </box>

      <box paddingTop={1}>
        <text fg={theme.textDim}>  鼠标点击选中 · 点击 [删除] 移除模型 · 用 CLI 添加: lloom-cli models add</text>
      </box>
    </box>
  )
}
