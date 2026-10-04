import { createSignal, onMount, Show } from "solid-js"
import { theme } from "../theme"
import { getProxyConfig, type ProxyConfig } from "../api"
import { healthServices } from "../health"
import { Card, PageHeader } from "../ui"

export function Settings(props: { setStatus: (s: string) => void }) {
  const services=healthServices; const [proxy,setProxy]=createSignal<ProxyConfig|null>(null)
  onMount(async()=>{try{setProxy(await getProxyConfig())}catch(e){props.setStatus(`设置加载失败: ${e}`)}})
  return <box flexDirection="column" flexGrow={1} minHeight={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
    <PageHeader title="设置" subtitle="只读运行信息；模型与 API Key 请在对应管理端配置" />
    <box flexDirection="row" flexGrow={1} minHeight={0}>
      <box width="50%" paddingRight={1}>
        <Card title="服务状态" flexGrow>
          <box flexDirection="column">
            {(services()??[]).map(s=><box flexDirection="column" paddingBottom={1}>
              <box flexDirection="row"><text fg={s.healthy?theme.success:theme.error}>{s.healthy?"●":"○"}</text><text fg={theme.text} paddingLeft={1}>{s.name}</text><box flexGrow={1}/><text fg={s.healthy?theme.success:theme.warning}>{s.status}</text></box>
              {s.detail&&<text fg={theme.textDim} paddingLeft={2} wrapMode="word">{s.detail}</text>}
            </box>)}
          </box>
        </Card>
      </box>
      <box width="50%" paddingLeft={1}>
        <Card title="API 与存储" flexGrow>
          <Show when={proxy()} fallback={<text fg={theme.textDim}>正在读取配置…</text>} keyed>{p=><box flexDirection="column">
            <box flexDirection="row"><text fg={theme.textDim} width={14}>BASE URL</text><text fg={theme.info}>{p.base_url}</text></box>
            <box flexDirection="row"><text fg={theme.textDim} width={14}>BIND</text><text fg={theme.text}>{p.bind}:{p.web_port}</text></box>
            <box flexDirection="row"><text fg={theme.textDim} width={14}>AUTH</text><text fg={p.auth_enabled?theme.success:theme.warning}>{p.auth_enabled?`${p.key_count} 个 Key 已启用`:`未创建 Key`}</text></box>
            <box flexDirection="row"><text fg={theme.textDim} width={14}>CACHE</text><text fg={theme.text}>SQLite exact + FastEmbed semantic</text></box>
            <box flexDirection="row"><text fg={theme.textDim} width={14}>DATA</text><text fg={theme.text}>./data</text></box>
            <box height={1}/><text fg={theme.textMuted}>API Key 管理仅在 WebUI 提供，避免多端配置冲突。</text>
          </box>}</Show>
        </Card>
      </box>
    </box>
  </box>
}
