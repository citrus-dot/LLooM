// Session route — conversation messages, sidebar, prompt. OpenCode-style.

import { createSignal, onMount, onCleanup } from "solid-js"
import { theme } from "../theme"
import {
  listConversations,
  loadConversation,
  saveConversation,
  deleteConversation,
  chatStream,
  orchestrateStream,
  type Conversation,
  type ChatMessage,
} from "../api"
import { activeSessionId, setActiveSessionId, setRoute, setNavHandler, navHandler } from "../app"

let initialQuery: string | null = null
export function setInitialQuery(q: string) {
  initialQuery = q
}

interface DisplayMsg extends ChatMessage {
  detail?: string
}

export function Session(props: { setStatus: (s: string) => void }) {
  const [convs, setConvs] = createSignal<Conversation[]>([])
  const [msgs, setMsgs] = createSignal<DisplayMsg[]>([])
  const [input, setInput] = createSignal("")
  const [loading, setLoading] = createSignal(false)
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)
  const [focus, setFocus] = createSignal<"conv" | "input">("input")

  const refreshConvs = async () => {
    try {
      setConvs((await listConversations()).conversations)
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
  }

  onMount(async () => {
    await refreshConvs()
    const id = activeSessionId()
    if (id) {
      await openConv(id)
    } else if (initialQuery) {
      const q = initialQuery
      initialQuery = null
      await send(q)
    }
    // Register keyboard nav: up/down cycle conversations (Enter handled by textarea)
    setNavHandler((key) => {
      if (key === "up" || key === "down") {
        const n = convs().length
        if (n === 0) return
        const dir = key === "down" ? 1 : -1
        const next = (selIdx() + dir + n) % n
        setSelIdx(next)
        setFocus("conv")
      } else if (key === "n") {
        newConv()
      } else if (key === "d") {
        if (activeSessionId()) delConv(activeSessionId()!)
      } else if (key === "escape" || key === "esc") {
        setFocus("input")
      }
    })
  })

  onCleanup(() => {
    if (navHandler()) setNavHandler(null)
  })

  const openConv = async (id: string) => {
    try {
      const c = await loadConversation(id)
      setMsgs(c.messages.map((m) => ({ ...m })))
      setActiveSessionId(id)
      setRoute("session")
    } catch (e) {
      props.setStatus(`加载失败: ${e}`)
    }
  }

  const persist = async (final: DisplayMsg[]) => {
    try {
      const messages = final.map((m) => ({ role: m.role, content: m.content }))
      const title = final.find((m) => m.role === "user")?.content.slice(0, 20) ?? "新对话"
      const res = await saveConversation({ id: activeSessionId() ?? undefined, title, messages })
      setActiveSessionId(res.id)
      refreshConvs()
    } catch {}
  }

  const send = async (text?: string) => {
    const q = (text ?? input()).trim()
    if (!q || loading()) return
    setInput("")
    const userMsg: DisplayMsg = { role: "user", content: q }
    const next = [...msgs(), userMsg]
    setMsgs(next)
    setLoading(true)
    props.setStatus("思考中...")

    try {
      // Try orchestration first; fall back to chat on error
      let response = ""
      let detail = ""
      try {
        const events = await orchestrateStream(q)
        let models: string[] = []
        for (const ev of events) {
          if (ev.event === "task_start" && ev.data?.model) models.push(ev.data.model)
          else if (ev.event === "result" && ev.data?.response) response = ev.data.response
        }
        if (models.length) detail = `调用模型: ${[...new Set(models)].join(" | ")}`
      } catch {
        response = await chatStream([...next.filter((m) => m.role === "user" || m.role === "assistant")])
      }
      const final = [...next, { role: "assistant", content: response || "(无响应)", detail }]
      setMsgs(final)
      await persist(final)
      props.setStatus("")
    } catch (e) {
      const final = [...next, { role: "assistant", content: `请求失败: ${e}` }]
      setMsgs(final)
      await persist(final)
      props.setStatus(`失败: ${e}`)
    } finally {
      setLoading(false)
    }
  }

  const newConv = async () => {
    setActiveSessionId(null)
    setMsgs([])
    setInput("")
  }

  const delConv = async (id: string) => {
    try {
      await deleteConversation(id)
      if (activeSessionId() === id) {
        setActiveSessionId(null)
        setMsgs([])
      }
      refreshConvs()
    } catch {}
  }

  return (
    <box flexDirection="row" flexGrow={1} minHeight={0}>
      {/* Sidebar: conversation list */}
      <box
        flexShrink={0}
        width={40}
        flexDirection="column"
        backgroundColor={theme.backgroundPanel}
        border={["right"]}
        borderColor={theme.border}
        paddingTop={1}
        paddingBottom={1}
        paddingLeft={1}
        paddingRight={1}
      >
        <text fg={theme.textMuted} attributes={1}> 会话</text>
        <box height={1} />
        <box flexDirection="column" flexGrow={1} gap={0}>
          {convs().length === 0 && <text fg={theme.textDim}>  暂无对话</text>}
          {convs().map((c, i) => {
            const isSel = i === selIdx() && focus() === "conv"
            const isHover = i === hoverIdx()
            const isActive = c.id === activeSessionId()
            return (
              <box
                backgroundColor={isSel ? theme.primary : isHover ? theme.backgroundElement : theme.backgroundPanel}
                paddingLeft={2}
                paddingRight={2}
                paddingTop={0}
                paddingBottom={0}
                onMouseOver={() => setHoverIdx(i)}
                onMouseOut={() => setHoverIdx(null)}
                onMouseDown={() => { setFocus("conv"); setSelIdx(i) }}
                onMouseUp={() => { setFocus("conv"); setSelIdx(i); openConv(c.id) }}
              >
                <text fg={isSel ? theme.background : isActive ? theme.primary : theme.text}>
                  {isActive ? "● " : ""}{c.title.slice(0, 20)}{isSel ? "" : ` (${c.message_count})`}
                </text>
              </box>
            )
          })}
        </box>
        <box height={1} />
        <box flexDirection="row" gap={1}>
          <text fg={theme.textMuted} onMouseUp={() => newConv()}>[新建]</text>
          <text fg={theme.textMuted} onMouseUp={() => { if (activeSessionId()) delConv(activeSessionId()!) }}>[删除]</text>
          <text fg={theme.textMuted} onMouseUp={() => setRoute("home")}>[主页]</text>
        </box>
      </box>

      {/* Main: messages + input */}
      <box flexGrow={1} flexDirection="column" minWidth={0}>
        {/* Messages */}
        <box flexGrow={1} flexDirection="column" paddingLeft={2} paddingRight={2} gap={1}>
          {msgs().length === 0 && (
            <text fg={theme.textDim}>  输入消息开始，或点击左侧会话</text>
          )}
          {msgs().map((m, i) => (
            <box
              backgroundColor={theme.backgroundPanel}
              border={["left"]}
              borderColor={m.role === "user" ? theme.primary : theme.secondary}
              paddingLeft={2}
              paddingRight={2}
              paddingTop={1}
              paddingBottom={1}
            >
              <text fg={m.role === "user" ? theme.text : theme.text} attributes={1}>
                {m.role === "user" ? " 你" : " LLooM"}
              </text>
              <text fg={theme.textMuted}>{m.role === "user" ? " · " : " · 第" + (i + 1) + "条"}</text>
              <text></text>
              <text fg={theme.text}>{m.content}</text>
              {m.detail ? <text fg={theme.textDim}>  {m.detail}</text> : null}
            </box>
          ))}
          {loading() && <text fg={theme.warning}>  ⏳ 思考中...</text>}
        </box>

        {/* Input */}
        <box flexShrink={0} paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1}>
          <box
            flexDirection="column"
            backgroundColor={theme.backgroundElement}
            border={["left"]}
            borderColor={focus() === "input" ? theme.primary : theme.border}
            paddingLeft={2}
            paddingRight={2}
            paddingTop={1}
            paddingBottom={1}
            onMouseDown={() => setFocus("input")}
          >
            <textarea
              value={input()}
              onContentChange={(v: string) => setInput(v)}
              onSubmit={() => send()}
              placeholder={loading() ? "处理中..." : "输入消息... Enter 发送"}
              width="100%"
            />
            <text fg={theme.textDim}>  [Enter] 发送 · [Tab] 页面 · ←/→ 切会话 · ↑/↓ 输入历史</text>
          </box>
        </box>
      </box>
    </box>
  )
}
