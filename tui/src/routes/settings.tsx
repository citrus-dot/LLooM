// Settings route — service status & control, OpenAI-compatible proxy access
// (N1). API keys are per-model settings edited on the Models route; env (.env)
// editing was removed — proxy token lives in the server's settings KV instead.

import { createSignal, onMount, Show } from "solid-js"
import { theme } from "../theme"
import {
  getServiceLogs,
  restartService,
  stopService,
  startService,
  getProxyConfig,
  setProxyToken,
  proxySelftest,
  type ProxyConfig,
} from "../api"
import { useDialog } from "../ui/dialog"
import { dialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { healthServices, pollHealth } from "../health"

// Display name → control API name (Core Server is the host itself; not manageable).
const SERVICE_KEYS: Record<string, string> = { "Ollama": "ollama", "AI Service": "ai" }

export function Settings(props: { setStatus: (s: string) => void }) {
  const services = healthServices
  const dialog = useDialog()
  const [proxy, setProxy] = createSignal<ProxyConfig | null>(null)

  // ── OpenAI 兼容代理接入（N1） ──
  const refreshProxy = async () => {
    try {
      setProxy(await getProxyConfig())
    } catch {
      /* server down — health banner already surfaces it */
    }
  }
  onMount(() => {
    void refreshProxy()
  })

  const authDetail = () => {
    const p = proxy()
    if (!p) return ""
    if (!p.auth_enabled) return "客户端无需 Key（仅环回绑定时安全）"
    const src =
      p.token_source === "ui"
        ? "本服务配置库"
        : p.token_source === "env"
          ? "环境变量 LLOOM_PROXY_TOKEN"
          : ""
    return `${p.token_masked} · ${src}`
  }

  const doSetKey = () => {
    dialog.prompt("设置代理 API Key（立即生效，免重启）", {
      placeholder: "Bearer token；作为客户端 API Key 下发",
      onConfirm: async (v) => {
        const t = v.trim()
        if (!t) return
        try {
          setProxy(await setProxyToken(t))
          props.setStatus("✓ 代理 API Key 已保存，立即生效")
        } catch (e) {
          props.setStatus(`设置失败: ${e}`)
        }
      },
    })
  }

  const doClearKey = async () => {
    try {
      setProxy(await setProxyToken(null))
      props.setStatus("✓ 已清除本端配置的代理 Key")
    } catch (e) {
      props.setStatus(`清除失败: ${e}`)
    }
  }

  const doSelftest = async () => {
    props.setStatus("⏳ 代理自测中...")
    try {
      const r = await proxySelftest()
      const lines = [`结果  : ${r.ok ? "✓ 通过" : "✗ 失败"}`]
      if (r.http != null) lines.push(`HTTP  : ${r.http}`)
      if (r.models != null) lines.push(`模型数: ${r.models}`)
      if (r.latency_ms != null) lines.push(`延迟  : ${r.latency_ms}ms`)
      lines.push("", r.detail)
      dialog.logs("代理连通性自测（环回 /v1/models）", { logs: lines.join("\n") })
      props.setStatus(r.ok ? "✓ 代理自测通过" : "代理自测失败")
    } catch (e) {
      props.setStatus(`自测失败: ${e}`)
    }
  }

  const proxyMenu = () => {
    const items = [
      {
        title: "设置 API Key",
        desc: "写入本地库，立即生效（免重启）",
        onSelect: () => doSetKey(),
      },
      {
        title: "连通性自测",
        desc: "服务端环回请求 /v1/models",
        onSelect: () => void doSelftest(),
      },
      ...(proxy()?.token_source === "ui"
        ? [
            {
              title: "清除 Key",
              desc: "回落环境变量或恢复不鉴权",
              danger: true,
              onSelect: () => void doClearKey(),
            },
          ]
        : []),
    ]
    dialog.menu("OpenAI 兼容代理", { items })
  }

  // p = proxy（本页无输入框，不冲突）
  useBindings(() => ({
    enabled: () => !dialogOpen(),
    bindings: [{ key: "p", cmd: () => proxyMenu(), desc: "OpenAI 代理菜单" }],
  }))

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

  return (
    <box flexDirection="column" flexGrow={1} minHeight={0} backgroundColor={theme.backgroundPanel}
      paddingLeft={2} paddingRight={2} paddingTop={1}>
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

      {/* ── OpenAI 兼容代理接入（N1）：右键本区或按 p 弹菜单 ── */}
      <box height={1} />
      <box onMouseUp={() => proxyMenu()}>
        <box flexDirection="row" gap={1}>
          <text fg={theme.primary} attributes={1}>⏵ OpenAI 兼容代理</text>
          <text fg={theme.textDim}>· 按 p / 右键打开菜单</text>
        </box>
      </box>
      <Show when={proxy()} keyed>
        {(p) => (
          <box flexDirection="column" paddingLeft={2}>
            <box flexDirection="row" gap={1}>
              <text fg={theme.textDim}>Base URL</text>
              <text fg={theme.text}>{p.base_url}</text>
            </box>
            <box flexDirection="row" gap={1}>
              <text fg={theme.textDim}>鉴权</text>
              <text fg={p.auth_enabled ? theme.success : theme.warning}>
                {p.auth_enabled ? "启用" : "未鉴权"}
              </text>
              <text fg={theme.textMuted}>{authDetail()}</text>
            </box>
            {p.bind !== "127.0.0.1" && p.bind !== "localhost" && (
              <box flexDirection="row" gap={1}>
                <text fg={theme.textDim}>绑定</text>
                <text fg={theme.info}>{p.bind}</text>
                <text fg={theme.textMuted}>· 局域网接入把 127.0.0.1 换成本机 IP</text>
              </box>
            )}
            <text fg={theme.textDim}>model: auto 智能路由（推荐）· 注册模型名直连 · 未知名回落 auto</text>
          </box>
        )}
      </Show>
      <Show when={!proxy()}>
        <text fg={theme.textMuted} paddingLeft={2}>（正在读取接入信息...）</text>
      </Show>
    </box>
  )
}
