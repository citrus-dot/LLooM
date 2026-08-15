// LLooM TUI REST client — all data flows through the lloom-server REST API.

const BASE = "http://localhost:7861"

export interface ServiceStatus {
  name: string
  status: string
  healthy: boolean
}

export interface ServicesStatus {
  services: ServiceStatus[]
  total: number
  healthy: number
}

export interface Model {
  name: string
  provider: string
  litellm_model: string
  input_cost_per_token: number
  output_cost_per_token: number
  task_type: string
}

export interface UsageRow {
  model_name: string
  total_input_tokens: number
  total_output_tokens: number
  total_cost: number
  request_count: number
}

export interface Conversation {
  id: string
  title: string
  message_count: number
}

export interface ChatMessage {
  role: string
  content: string
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`)
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json() as Promise<T>
}

async function post<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? "{}" : JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json() as Promise<T>
}

async function del<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { method: "DELETE" })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json() as Promise<T>
}

export async function getServicesStatus(): Promise<ServicesStatus> {
  return get("/api/services/status")
}

export async function startService(name: string): Promise<{ message: string }> {
  return post(`/api/services/${name}/start`)
}

export async function stopService(name: string): Promise<{ message: string }> {
  return post(`/api/services/${name}/stop`)
}

export async function restartService(name: string): Promise<{ message: string }> {
  return post(`/api/services/${name}/restart`)
}

export async function getServiceLogs(name: string): Promise<{ logs: string }> {
  return get(`/api/services/${name}/logs`)
}

export async function getModels(): Promise<{ models: Model[] }> {
  return get("/api/models")
}

export async function deleteModel(name: string): Promise<{ deleted: boolean }> {
  return del(`/api/models/${encodeURIComponent(name)}`)
}

export async function getStats(): Promise<{ total_spend: number; model_count: number }> {
  return get("/api/stats")
}

export async function getUsage(): Promise<{ usage: UsageRow[]; total_spend: number }> {
  return get("/api/usage")
}

export async function getBudgets(): Promise<{ budgets: { scope: string; scope_id: string; max_budget: number }[] }> {
  return get("/api/budgets")
}

export async function listConversations(): Promise<{ conversations: Conversation[] }> {
  return get("/api/conversations")
}

export async function loadConversation(id: string): Promise<{ messages: ChatMessage[]; title: string }> {
  return get(`/api/conversations/${encodeURIComponent(id)}`)
}

export async function saveConversation(c: { id?: string; title?: string; messages: ChatMessage[] }): Promise<{ id: string }> {
  return post("/api/conversations", c)
}

export async function deleteConversation(id: string): Promise<{ deleted: boolean }> {
  return del(`/api/conversations/${encodeURIComponent(id)}`)
}

export async function readEnv(): Promise<Record<string, string>> {
  return get("/api/config")
}

export async function writeEnv(updates: Record<string, string>): Promise<{ updated: string[] }> {
  return post("/api/config", { updates })
}

// SSE chat stream → full response text
export async function chatStream(messages: ChatMessage[]): Promise<string> {
  const res = await fetch(`${BASE}/api/chat/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ messages }),
  })
  const text = await res.text()
  let out = ""
  for (const line of text.split("\n")) {
    if (line.startsWith("data: ")) {
      try {
        const v = JSON.parse(line.slice(6))
        if (v.content) out += v.content
      } catch {}
    }
  }
  return out
}

// SSE orchestrate stream → parsed events
export interface SseEvent {
  event: string
  data: any
}

export async function orchestrateStream(query: string): Promise<SseEvent[]> {
  const res = await fetch(`${BASE}/api/orchestrate/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query, history: [] }),
  })
  const text = await res.text()
  const events: SseEvent[] = []
  let cur: SseEvent | null = null
  for (const line of text.split("\n")) {
    if (line.startsWith("event:")) {
      if (cur) events.push(cur)
      cur = { event: line.slice(7).trim(), data: null }
    } else if (line.startsWith("data: ")) {
      try {
        const v = JSON.parse(line.slice(6))
        if (cur && cur.data === null) cur.data = v
        else events.push({ event: "data", data: v })
      } catch {}
    }
  }
  if (cur) events.push(cur)
  return events
}
