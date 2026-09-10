// Settings route — service status & control. API keys are per-model settings
// edited on the Models route; env (.env) editing was removed.

import { theme } from "../theme"
import { getServiceLogs, restartService, stopService, startService } from "../api"
import { useDialog } from "../ui/dialog"
import { healthServices, pollHealth } from "../health"

// Display name → control API name (Core Server is the host itself; not manageable).
const SERVICE_KEYS: Record<string, string> = { "Ollama": "ollama", "AI Service": "ai" }

export function Settings(props: { setStatus: (s: string) => void }) {
  const services = healthServices
  const dialog = useDialog()

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
    </box>
  )
}
