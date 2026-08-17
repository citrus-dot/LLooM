// Session route — OpenCode-style chat: scrollbox + user/assistant message blocks.

import { createSignal, createMemo, createEffect, onMount, For, Show } from "solid-js"
import { theme } from "../theme"
import type { TextareaRenderable } from "@opentui/core"
import { useBindings } from "@opentui/keymap/solid"
import {
  listConversations,
  loadConversation,
  saveConversation,
  deleteConversation,
  renameConversation,
  chatStream,
  orchestrateStream,
  type Conversation,
  type ChatMessage,
} from "../api"
import { activeSessionId, setActiveSessionId, setRoute, initialQuery, setInitialQuery, dialogOpen } from "../app"
import { useDialog } from "../ui/dialog"
import { UserBubble, AssistantBubble } from "../ui"

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
  const [convHover, setConvHover] = createSignal<number | null>(null)
  const [focus, setFocus] = createSignal<"conv" | "input">("input")
  const [inputHover, setInputHover] = createSignal(false)
  const dialog = useDialog()
  let inputRef: TextareaRenderable | undefined

  createEffect(() => {
    if (inputRef && !inputRef.focused) inputRef.focus()
  })

  onMount(() => {
    if (inputRef && !inputRef.focused) inputRef.focus()
  })

  const refreshConvs = async () => {
    try {
      setConvs((await listConversations()).conversations)
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
  }

  const openConv = async (id: string) => {
    try {
      const c = await loadConversation(id)
      setMsgs(c.messages.map((m) => ({ ...m })))
      setActiveSessionId(id)
      setRoute("session")
      setSelIdx(Math.max(1, convs().findIndex((x) => x.id === id) + 1))
      setFocus("input")
    } catch (e) {
      props.setStatus(`加载失败: ${e}`)
    }
  }

  onMount(async () => {
    await refreshConvs()
    const id = activeSessionId()
    if (id) {
      await openConv(id)
    } else if (initialQuery()) {
      const q = initialQuery()!
      setInitialQuery(null)
      await send(q)
    }
  })

  useBindings(() => ({
    // List navigation only applies while focus is on the conversation list;
    // when the input is focused, Enter/arrows go to the textarea.
    enabled: () => !dialogOpen() && focus() === "conv",
    bindings: [
      {
        key: "up",
        cmd: () => {
          const total = convs().length + 1
          if (total <= 1) return
          setSelIdx((selIdx() - 1 + total) % total)
          setFocus("conv")
        },
        desc: "Previous conversation",
      },
      {
        key: "down",
        cmd: () => {
          const total = convs().length + 1
          if (total <= 1) return
          setSelIdx((selIdx() + 1) % total)
          setFocus("conv")
        },
        desc: "Next conversation",
      },
      {
        key: "return",
        cmd: () => {
          setFocus("conv")
          if (selIdx() === 0) {
            newConv()
            return
          }
          const c = convs()[selIdx() - 1]
          if (c) openConv(c.id)
        },
        desc: "Open conversation",
      },
      {
        key: "ctrl+n",
        cmd: () => newConv(),
        desc: "New conversation",
      },
      {
        key: "ctrl+d",
        cmd: () => {
          if (activeSessionId()) delConv(activeSessionId()!)
        },
        desc: "Delete conversation",
      },
    ],
  }))

  const persist = async (final: DisplayMsg[]) => {
    try {
      const messages = final.map((m) => ({ role: m.role, content: m.content }))
      // Only a brand-new conversation gets an auto-title; existing ones pass an
      // empty title so the backend preserves the user's (possibly edited) title.
      const id = activeSessionId()
      const title = id ? "" : (final.find((m) => m.role === "user")?.content.slice(0, 20) ?? "新对话")
      const res = await saveConversation({ id: id ?? undefined, title, messages })
      setActiveSessionId(res.id)
      refreshConvs()
    } catch {}
  }

  const send = async (text?: string) => {
    const q = (text ?? inputRef?.plainText ?? "").trim()
    if (!q || loading()) return
    setInput("")
    inputRef?.clear()
    const base = msgs()
    const userMsg: DisplayMsg = { role: "user", content: q }
    const assistantIdx = base.length + 1
    setMsgs([...base, userMsg, { role: "assistant", content: "", detail: "" }])
    setLoading(true)
    props.setStatus("思考中...")

    // Streaming accumulators, updated from the onEvent callback.
    let streaming = ""
    const models = new Set<string>()
    let aggregator = ""
    let subTasks = 0
    let doneCount = 0
    let totalDur = 0
    let cached = false
    let blocked = ""
    let failed = false

    const patch = (content: string, detail: string) => {
      setMsgs((prev) => {
        const copy = prev.slice()
        if (assistantIdx < copy.length) copy[assistantIdx] = { role: "assistant", content, detail }
        return copy
      })
    }

    try {
      const history = base.filter((m) => m.role === "user" || m.role === "assistant")
      try {
        await orchestrateStream(q, history, (ev) => {
          switch (ev.event) {
            case "task_start":
              if (ev.data?.model) models.add(ev.data.model)
              break
            case "decompose":
              if (ev.data?.sub_tasks) subTasks = ev.data.sub_tasks.length
              break
            case "task_done":
              doneCount++
              totalDur += ev.data?.duration ?? 0
              if (ev.data?.cache_hit) cached = true
              if (ev.data?.error) failed = true
              break
            case "token":
              if (ev.data?.delta) {
                streaming += ev.data.delta
                patch(streaming, "")
              }
              break
            case "result":
              if (ev.data?.response) streaming = ev.data.response
              if (ev.data?.models_used) (ev.data.models_used as string[]).forEach((m) => models.add(m))
              if (ev.data?.aggregator) aggregator = ev.data.aggregator
              if (ev.data?.cache_hit) cached = true
              break
            default:
              if (ev.data?.error && ev.data?.detail) blocked = String(ev.data.detail)
          }
        })
      } catch {
        streaming = await chatStream(history)
      }

      const parts: string[] = []
      if (models.size) parts.push(`调用模型: ${[...models].join(" | ")}`)
      if (subTasks > 1) parts.push(`${doneCount}/${subTasks} 子任务 · ${totalDur.toFixed(1)}s`)
      if (aggregator) parts.push(`汇总: ${aggregator}`)
      if (cached) parts.push("来自缓存")
      if (failed) parts.push("部分子任务失败")
      const detail = parts.join(" · ")

      const content = blocked ? `请求被拦截: ${blocked}` : streaming || "(无响应)"
      const final = [...base, userMsg, { role: "assistant", content, detail }]
      setMsgs(final)
      await persist(final)
      props.setStatus("")
    } catch (e) {
      const final = [...base, userMsg, { role: "assistant", content: `请求失败: ${e}`, detail: "" }]
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
    inputRef?.clear()
    setFocus("input")
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

  const convMenu = (c: Conversation) => {
    dialog.menu(c.title.slice(0, 18), {
      items: [
        { title: "打开", desc: "进入对话", onSelect: () => openConv(c.id) },
        {
          title: "重命名",
          desc: "修改对话标题",
          onSelect: () => {
            dialog.prompt("重命名对话", {
              value: c.title,
              onConfirm: async (title) => {
                const t = title.trim()
                if (!t) return
                try {
                  await renameConversation(c.id, t)
                  refreshConvs()
                } catch {}
              },
            })
          },
        },
        {
          title: "删除",
          desc: "移除该对话",
          danger: true,
          onSelect: () => {
            delConv(c.id)
            props.setStatus("")
          },
        },
      ],
    })
  }

  const headerMenu = () => {
    dialog.menu("会话", {
      items: [
        { title: "新建对话", onSelect: () => newConv() },
        { title: "返回主页", onSelect: () => setRoute("home") },
      ],
    })
  }

  return (
    <box flexDirection="row" flexGrow={1} minHeight={0}>
      {/* Sidebar: conversation list (OpenCode style) */}
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
        <text fg={theme.textMuted} attributes={1} onMouseUp={(evt: { button?: number }) => { if (evt?.button === 2) headerMenu() }}> 会话</text>
        <box height={1} />
        <box flexDirection="column" flexGrow={1} gap={0}>
          {/* "New conversation" is always the first (index 0) item. */}
          <box
            backgroundColor={focus() === "conv" && selIdx() === 0 ? theme.primary : convHover() === 0 ? theme.backgroundElement : theme.backgroundPanel}
            paddingLeft={2}
            paddingRight={2}
            onMouseOver={() => setConvHover(0)}
            onMouseOut={() => setConvHover(null)}
            onMouseDown={() => { setFocus("conv"); setSelIdx(0) }}
            onMouseUp={() => newConv()}
          >
            <text fg={focus() === "conv" && selIdx() === 0 ? theme.background : theme.primary}>
              {activeSessionId() === null ? "▸ " : "  "}[+] 新建对话
            </text>
          </box>
          {convs().length === 0 && <text fg={theme.textDim} paddingLeft={2}>  暂无对话</text>}
          {convs().map((c, i) => {
            const listIdx = i + 1
            const isSel = focus() === "conv" && listIdx === selIdx()
            const isHover = listIdx === convHover()
            const isActive = c.id === activeSessionId()
            return (
              <box
                backgroundColor={isSel ? theme.primary : isHover ? theme.backgroundElement : theme.backgroundPanel}
                paddingLeft={2}
                paddingRight={2}
                onMouseOver={() => setConvHover(listIdx)}
                onMouseOut={() => setConvHover(null)}
                onMouseDown={() => { setFocus("conv"); setSelIdx(listIdx) }}
                onMouseUp={(evt: { button?: number }) => {
                  if (evt?.button === 2) convMenu(c)
                  else openConv(c.id)
                }}
              >
                <text fg={isSel ? theme.background : isActive ? theme.primary : theme.text}>
                  {isActive ? "▸ " : "  "}{c.title.slice(0, 18)}{isSel ? "" : ` (${c.message_count})`}
                </text>
              </box>
            )
          })}
        </box>
        <box height={1} />
        <text fg={theme.textDim}>  右键会话项弹出操作菜单</text>
      </box>

      {/* Main: scrollbox messages + input */}
      <box flexGrow={1} flexDirection="column" minWidth={0}>
        {/* Scrollable messages (OpenCode scrollbox, sticky bottom) */}
        <scrollbox
          flexGrow={1}
          minHeight={0}
          stickyScroll={true}
          stickyStart="bottom"
          paddingLeft={2}
          paddingRight={2}
          paddingBottom={1}
        >
          <box height={1} />
          <For each={msgs()}>
            {(m, i) => (
              <Show when={m.role === "user" || m.role === "assistant"}>
                <box
                  marginTop={i() === 0 ? 0 : 1}
                  flexShrink={0}
                >
                  {m.role === "user" ? (
                    <UserBubble content={m.content} />
                  ) : (
                    <AssistantBubble content={m.content} detail={m.detail} />
                  )}
                </box>
              </Show>
            )}
          </For>
          {loading() && (
            <box paddingLeft={3} marginTop={1}>
              <text fg={theme.warning}>⏳ 思考中...</text>
            </box>
          )}
          <box height={1} />
        </scrollbox>

        {/* Input (OpenCode prompt style) */}
        <box flexShrink={0} paddingLeft={2} paddingRight={2} paddingTop={1} paddingBottom={1}>
          <box
            flexDirection="column"
            backgroundColor={inputHover() || focus() === "input" ? theme.backgroundElement : theme.backgroundPanel}
            border={["left"]}
            borderColor={focus() === "input" ? theme.primary : theme.border}
            paddingLeft={2}
            paddingRight={2}
            paddingTop={1}
            paddingBottom={1}
            onMouseOver={() => setInputHover(true)}
            onMouseOut={() => setInputHover(false)}
            onMouseDown={() => setFocus("input")}
          >
            <textarea
              ref={(r: TextareaRenderable) => {
                inputRef = r
              }}
              onContentChange={() => setInput(inputRef?.plainText ?? "")}
              onSubmit={() => send()}
              placeholder={loading() ? "处理中..." : "输入消息... Enter 发送"}
              minHeight={1}
              width="100%"
              textColor={theme.text}
              focusedTextColor={theme.text}
              keyBindings={[
                { name: "return", action: "submit" },
                { name: "return", shift: true, action: "newline" },
              ]}
            />
            <text fg={theme.textDim}>  Enter 发送 · Tab 切页 · ↑↓ 切会话 · Ctrl+N 新建 · Ctrl+D 删除</text>
          </box>
        </box>
      </box>
    </box>
  )
}

