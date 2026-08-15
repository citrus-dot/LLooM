// Settings route — env keys (API keys) editing, service control, OpenCode-style.

import { createSignal, onMount } from "solid-js"
import { theme } from "../theme"
import { readEnv, writeEnv, smartRestart, getServiceLogs, restartService, stopService, startService } from "../api"
import { dialogOpen } from "../app"
import { useDialog } from "../ui/dialog"
import { useBindings } from "@opentui/keymap/solid"
import { healthServices, pollHealth } from "../health"
import { Table } from "../ui"

const ENV_SCHEMA: { title: string; items: { key: string; label: string }[] }[] = [
  { title: "DashScope", items: [{ key: "DASHSCOPE_API_KEY", label: "API Key" }, { key: "DASHSCOPE_API_BASE", label: "API Base" }] },
  { title: "OpenAI", items: [{ key: "OPENAI_API_KEY", label: "API Key" }, { key: "OPENAI_BASE_URL", label: "Base URL" }] },
  { title: "Anthropic", items: [{ key: "ANTHROPIC_API_KEY", label: "API Key" }] },
  { title: "核心配置", items: [{ key: "OLLAMA_API_BASE", label: "Ollama 地址" }, { key: "LLOOM_WEB_PORT", label: "Web 端口" }, { key: "LLOOM_DATA_DIR", label: "数据目录" }] },
]

// Display name → control API name (Core Server is the host itself; not manageable).
const SERVICE_KEYS: Record<string, string> = { "Ollama": "ollama", "AI Service": "ai" }

export function Settings(props: { setStatus: (s: string) => void }) {
  const [env, setEnv] = createSignal<Record<string, string>>({})
  const services = healthServices
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)
  const dialog = useDialog()

  // All editable keys: schema-declared keys first, then any remaining keys the
  // server exposes via /api/config (so nothing is hidden).
  const flatKeys = () => {
    const schemaKeys = ENV_SCHEMA.flatMap((s) => s.items.map((i) => i.key))
    const extra = Object.keys(env()).filter((k) => !schemaKeys.includes(k)).sort()
    return [...schemaKeys, ...extra]
  }

  const sectionOf = (key: string) => {
    const s = ENV_SCHEMA.find((s) => s.items.some((it) => it.key === key))
    return s ? s.title : "其他配置"
  }

  const save = async (key: string, val: string) => {
    try {
      await writeEnv({ [key]: val })
      setEnv({ ...env(), [key]: val })
      props.setStatus(`✓ 已保存 ${key}`)
      // After a config change, offer to apply it by smart-restarting affected
      // services (same as WebUI's "应用配置").
      dialog.menu(`配置已保存，立即应用?`, {
        items: [
          { title: "重启服务生效", desc: "智能重启 AI / Ollama", onSelect: () => void applyChanges([key]) },
          { title: "稍后再说", onSelect: () => {} },
        ],
      })
    } catch (e) {
      props.setStatus(`保存失败: ${e}`)
    }
  }

  const applyChanges = async (changedKeys: string[]) => {
    try {
      props.setStatus("⏳ 重启服务使配置生效...")
      const res = await smartRestart(changedKeys)
      if (res.ok) {
        props.setStatus(`✓ 配置已生效，已重启 ${res.restarted.join(", ") || "(无)"}`)
      } else {
        props.setStatus(`重启失败: ${res.errors.join("; ")}`)
      }
      await pollHealth()
    } catch (e) {
      props.setStatus(`智能重启失败: ${e}`)
    }
  }

  const editKey = (key: string) => {
    const label = ENV_SCHEMA.find((s) => s.items.some((it) => it.key === key))?.items.find((it) => it.key === key)?.label ?? key
    dialog.prompt(`设置 ${label} (${key})`, {
      value: env()[key] ?? "",
      placeholder: "输入密钥值...",
      onConfirm: (v) => save(key, v.trim()),
    })
  }

  const serviceKey = (displayName: string) => SERVICE_KEYS[displayName]
  const controllable = (displayName: string) => serviceKey(displayName) !== undefined

  const showLogs = async (displayName: string) => {
    const key = serviceKey(displayName)
    if (!key) return
    dialog.logs(`${displayName} 日志`, {
      onRefresh: async () => {
        const res = await getServiceLogs(key)
        return res.logs
      },
    })
  }

  const doRestart = async (displayName: string) => {
    const key = serviceKey(displayName)
    if (!key) return
    try {
      props.setStatus(`⏳ 重启 ${displayName}...`)
      await restartService(key)
      props.setStatus(`✓ 已重启 ${displayName}`)
    } catch (e) {
      props.setStatus(`重启失败: ${e}`)
    }
    await pollHealth()
  }

  const doStop = async (displayName: string) => {
    const key = serviceKey(displayName)
    if (!key) return
    try {
      await stopService(key)
      props.setStatus(`✓ 已停止 ${displayName}`)
    } catch (e) {
      props.setStatus(`停止失败: ${e}`)
    }
    await pollHealth()
  }

  const doStart = async (displayName: string) => {
    const key = serviceKey(displayName)
    if (!key) return
    try {
      await startService(key)
      props.setStatus(`✓ 已启动 ${displayName}`)
    } catch (e) {
      props.setStatus(`启动失败: ${e}`)
    }
    await pollHealth()
  }

  const serviceMenu = (displayName: string) => {
    const svc = services()?.find((s) => s.name === displayName)
    const healthy = svc?.healthy ?? false
    dialog.menu(displayName, {
      items: [
        { title: "查看日志", desc: "打开日志弹框", onSelect: () => showLogs(displayName) },
        { title: "重启", desc: "停止后重新启动", onSelect: () => doRestart(displayName) },
        {
          title: healthy ? "停止" : "启动",
          desc: healthy ? "停止该服务" : "启动该服务",
          danger: healthy,
          onSelect: () => (healthy ? doStop(displayName) : doStart(displayName)),
        },
      ],
    })
  }

  onMount(async () => {
    try {
      setEnv(await readEnv())
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
  })

  useBindings(() => ({
    enabled: () => !dialogOpen(),
    bindings: [
      {
        key: "up",
        cmd: () => {
          const n = flatKeys().length
          if (n > 0) setSelIdx((selIdx() - 1 + n) % n)
        },
        desc: "Previous key",
      },
      {
        key: "down",
        cmd: () => {
          const n = flatKeys().length
          if (n > 0) setSelIdx((selIdx() + 1) % n)
        },
        desc: "Next key",
      },
      {
        key: "return",
        cmd: () => {
          const k = flatKeys()[selIdx()]
          if (k) editKey(k)
        },
        desc: "Edit key",
      },
    ],
  }))

  return (
    <box flexDirection="row" flexGrow={1} minHeight={0}>
      {/* Left: service status */}
      <box flexDirection="column" width={40} flexShrink={0} backgroundColor={theme.backgroundPanel}
        border={["right"]} borderColor={theme.border} paddingLeft={2} paddingRight={2} paddingTop={1}>
        <text fg={theme.textMuted} attributes={1}>服务状态</text>
        <box height={1} />
        {(services() ?? []).map((s) => (
          <box flexDirection="column">
            <box
              flexDirection="row"
              gap={1}
              onMouseUp={(evt: { button?: number }) => { if (evt?.button === 2 && controllable(s.name)) serviceMenu(s.name) }}
            >
              <text fg={s.healthy ? theme.success : theme.error}>{s.healthy ? "●" : "○"}</text>
              <text fg={theme.text}>{s.name}</text>
              <text fg={s.healthy ? theme.success : theme.error}>{s.status}</text>
            </box>
            {s.detail && <text fg={theme.warning} wrapMode="word" paddingLeft={3}>{s.detail}</text>}
          </box>
        ))}
        <box height={1} />
        <text fg={theme.textDim}>  右键服务名弹出操作菜单</text>
      </box>

      {/* Right: env keys */}
      <box flexDirection="column" flexGrow={1} minWidth={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
        <text fg={theme.textMuted} attributes={1}>API 密钥配置</text>
        <box height={1} />

        <Table
          columns={[
            {
              title: "状态",
              width: "6%",
              render: (key, { selected }) => {
                const isSet = (env()[key] ?? "").trim().length > 0
                return <text fg={selected ? theme.background : isSet ? theme.success : theme.textMuted}>{isSet ? "✓" : "○"}</text>
              },
            },
            {
              title: "配置项",
              width: "25%",
              render: (key, { selected }) => {
                const label = ENV_SCHEMA.find((s) => s.items.some((it) => it.key === key))?.items.find((it) => it.key === key)?.label ?? key
                return <text fg={selected ? theme.background : theme.text} attributes={selected ? 1 : 0}>{label}</text>
              },
            },
            { title: "Key", width: "20%", render: (key, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{key}</text> },
            { title: "分组", width: "15%", render: (key, { selected }) => <text fg={selected ? theme.background : theme.textDim}>{sectionOf(key)}</text> },
            {
              title: "值",
              render: (key, { selected }) => {
                const isSet = (env()[key] ?? "").trim().length > 0
                return <text fg={selected ? theme.background : theme.textDim}>{isSet ? "••••••" : "(空)"}</text>
              },
            },
          ]}
          rows={flatKeys()}
          rowKey={(k) => k}
          selectedIndex={selIdx()}
          hoverIndex={hoverIdx()}
          onHover={setHoverIdx}
          onSelect={setSelIdx}
          onRowUp={(key) => editKey(key)}
          emptyText="暂无配置"
        />

        <box paddingTop={1}>
          <text fg={theme.textDim}>  点击密钥行或按 Enter 弹出编辑框，⏎ 保存 · esc 取消 · ↑↓ 选择</text>
        </box>
      </box>
    </box>
  )
}
