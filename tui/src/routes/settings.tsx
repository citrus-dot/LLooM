// Settings route — env keys (API keys) editing, OpenCode-style list.

import { createSignal, onMount, onCleanup } from "solid-js"
import { theme } from "../theme"
import { readEnv, writeEnv, getServicesStatus } from "../api"
import { setNavHandler, navHandler } from "../app"

const ENV_SCHEMA: { title: string; items: { key: string; label: string }[] }[] = [
  { title: "DashScope", items: [{ key: "DASHSCOPE_API_KEY", label: "API Key" }, { key: "DASHSCOPE_API_BASE", label: "API Base" }] },
  { title: "OpenAI", items: [{ key: "OPENAI_API_KEY", label: "API Key" }, { key: "OPENAI_BASE_URL", label: "Base URL" }] },
  { title: "Anthropic", items: [{ key: "ANTHROPIC_API_KEY", label: "API Key" }] },
  { title: "核心配置", items: [{ key: "OLLAMA_API_BASE", label: "Ollama 地址" }, { key: "LLOOM_WEB_PORT", label: "Web 端口" }, { key: "LLOOM_DATA_DIR", label: "数据目录" }] },
]

export function Settings(props: { setStatus: (s: string) => void }) {
  const [env, setEnv] = createSignal<Record<string, string>>({})
  const [editing, setEditing] = createSignal<string | null>(null)
  const [editVal, setEditVal] = createSignal("")
  const [services, setServices] = createSignal<{ name: string; status: string; healthy: boolean }[]>([])
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)

  const flatKeys = () => ENV_SCHEMA.flatMap((s) => s.items.map((i) => i.key))

  onMount(async () => {
    try {
      setEnv(await readEnv())
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
    try {
      setServices((await getServicesStatus()).services)
    } catch {}
    setNavHandler((key) => {
      const n = flatKeys().length
      if (n === 0) return
      if (key === "up" || key === "down") {
        const dir = key === "down" ? 1 : -1
        setSelIdx((selIdx() + dir + n) % n)
        setEditing(null)
      } else if (key === "enter" || key === "return") {
        const k = flatKeys()[selIdx()]
        if (editing() === k) save(k)
        else { setEditing(k); setEditVal("") }
      }
    })
  })

  onCleanup(() => {
    if (navHandler()) setNavHandler(null)
  })

  const save = async (key: string) => {
    try {
      await writeEnv({ [key]: editVal() })
      const next = { ...env(), [key]: editVal() }
      setEnv(next)
      setEditing(null)
      props.setStatus(`✓ 已保存 ${key}`)
    } catch (e) {
      props.setStatus(`保存失败: ${e}`)
    }
  }

  return (
    <box flexDirection="row" flexGrow={1} minHeight={0}>
      {/* Left: service status */}
      <box flexDirection="column" width={40} flexShrink={0} backgroundColor={theme.backgroundPanel}
        border={["right"]} borderColor={theme.border} paddingLeft={2} paddingRight={2} paddingTop={1}>
        <text fg={theme.textMuted} attributes={1}>服务状态</text>
        <box height={1} />
        {services().map((s) => (
          <box flexDirection="row" gap={1}>
            <text fg={s.healthy ? theme.success : theme.error}>{s.healthy ? "●" : "○"}</text>
            <text fg={theme.text}>{s.name}</text>
            <text fg={s.healthy ? theme.success : theme.error}>{s.status}</text>
          </box>
        ))}
      </box>

      {/* Right: env keys */}
      <box flexDirection="column" flexGrow={1} minWidth={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
        <text fg={theme.textMuted} attributes={1}>API 密钥配置</text>
        <box height={1} />

        <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left", "right"]} borderColor={theme.border} paddingTop={1} paddingBottom={1}>
          {flatKeys().map((key, i) => {
            const section = ENV_SCHEMA.find((s) => s.items.some((it) => it.key === key))?.title ?? ""
            const label = ENV_SCHEMA.find((s) => s.items.some((it) => it.key === key))?.items.find((it) => it.key === key)?.label ?? key
            const val = env()[key] ?? ""
            const isSet = val.trim().length > 0
            const isSel = i === selIdx()
            const isHover = i === hoverIdx()
            const isEdit = editing() === key
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
                <text fg={isSel ? theme.background : isSet ? theme.success : theme.textMuted} width={2}>{isSet ? "✓" : "○"}</text>
                <text fg={isSel ? theme.background : theme.text} width="25%" attributes={isSel ? 1 : 0}>{label}</text>
                <text fg={isSel ? theme.background : theme.textMuted} width="20%">{key}</text>
                <text fg={theme.textDim} width="15%">[{section}]</text>
                {isEdit ? (
                  <text fg={isSel ? theme.background : theme.text} onMouseUp={() => save(key)}>
                    {editVal() || " "} [保存]
                  </text>
                ) : (
                  <text fg={isSel ? theme.background : theme.textDim} onMouseUp={() => { setEditing(key); setEditVal("") }}>
                    {isSet ? "•••••• [编辑]" : "[设置]"}
                  </text>
                )}
              </box>
            )
          })}
        </box>

        <box paddingTop={1}>
          <text fg={theme.textDim}>  点击密钥行进入编辑，输入新值后点 [保存] 提交到 .env</text>
        </box>
      </box>
    </box>
  )
}
