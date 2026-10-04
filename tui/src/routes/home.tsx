import { createSignal, createEffect, onMount, onCleanup, Show } from "solid-js"
import { theme } from "../theme"
import { getServicesStatus, getStats, type ServicesStatus } from "../api"
import { setRoute, setActiveSessionId, setInitialQuery } from "../app"
import type { TextareaRenderable } from "@opentui/core"
import { PageHeader, StatCard } from "../ui"

export function Home(props: { setStatus: (s: string) => void }) {
  const [services, setServices] = createSignal<ServicesStatus | null>(null)
  const [stats, setStats] = createSignal<{ total_spend: number; model_count: number; cache_enabled: boolean } | null>(null)
  let inputRef: TextareaRenderable | undefined
  createEffect(() => { if (inputRef && !inputRef.focused) inputRef.focus() })
  const refresh = async () => { try { const [svc,st]=await Promise.all([getServicesStatus(),getStats()]); setServices(svc); setStats(st) } catch(e){ props.setStatus(`无法连接服务器: ${e}`) } }
  onMount(async()=>{ await refresh(); inputRef?.focus(); const t=setInterval(refresh,30000); onCleanup(()=>clearInterval(t)) })
  const submit=()=>{ const q=(inputRef?.plainText??"").trim(); if(!q)return; inputRef?.clear(); setInitialQuery(q); setActiveSessionId(null); setRoute("session") }
  return <box flexDirection="column" flexGrow={1} minHeight={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
    <PageHeader title="概览" subtitle="本地 LLM 路由、成本与缓存状态" />
    <box flexDirection="row" paddingBottom={1}>
      <StatCard flexGrow value={`$${(stats()?.total_spend??0).toFixed(4)}`} label="累计花费" tone="warning" />
      <StatCard flexGrow value={String(stats()?.model_count??0)} label="可用模型" tone="primary" />
      <StatCard flexGrow value={`${services()?.healthy??0}/${services()?.total??0}`} label="健康服务" tone="success" />
      <StatCard flexGrow value="L1 + L2" label="缓存层级" tone="secondary" />
    </box>
    <box flexDirection="row" flexGrow={1} minHeight={0}>
      <box width="30%" flexDirection="column" paddingRight={2} border={["right"]} borderColor={theme.border}>
        <text fg={theme.text} attributes={1}>运行状态</text>
        <box height={1}/>
        <Show when={services()} fallback={<text fg={theme.textDim}>正在连接…</text>}>
          {services()!.services.map(s=><box flexDirection="row"><text fg={s.healthy?theme.success:theme.error}>{s.healthy?"●":"○"}</text><text fg={theme.text} paddingLeft={1}>{s.name}</text><box flexGrow={1}/><text fg={theme.textMuted}>{s.status}</text></box>)}
        </Show>
        <box height={1}/><text fg={theme.textDim}>OpenAI API  http://127.0.0.1:7861/v1</text>
      </box>
      <box flexGrow={1} flexDirection="column" paddingLeft={2}>
        <text fg={theme.text} attributes={1}>开始对话</text>
        <text fg={theme.textDim}>输入任务，LLooM 会自动选择模型并记录成本。</text>
        <box height={1}/>
        <box backgroundColor={theme.backgroundElement} border={["left"]} borderColor={theme.primary} paddingLeft={1} paddingRight={1} paddingTop={1} paddingBottom={1}>
          <textarea ref={(r:TextareaRenderable)=>{inputRef=r}} onSubmit={submit} placeholder="问点什么…  Enter 发送 · Shift+Enter 换行" width="100%" keyBindings={[{name:"return",action:"submit"},{name:"return",shift:true,action:"newline"}]}/>
        </box>
        <box height={1}/><text fg={theme.textDim}>快捷入口  [2] 对话  [3] 模型  [4] 用量  [5] 设置</text>
      </box>
    </box>
  </box>
}
