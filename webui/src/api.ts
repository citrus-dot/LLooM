// Typed API client for the LLooM REST API.

export interface ServiceStatus {
  name: string;
  status: string;
  healthy: boolean;
  detail?: string;
}

export interface ServicesStatus {
  services: ServiceStatus[];
  total: number;
  healthy: number;
  running: number;
}

export interface Model {
  id: number;
  name: string;
  provider: string;
  litellm_model: string;
  api_base: string;
  api_key_env: string;
  task_type: string;
  input_cost_per_token: number;
  output_cost_per_token: number;
  rpm: number;
  is_active: number;
}

export interface UsageStats {
  model_count: number;
  total_spend: number;
  model_spend: UsageRow[];
  routing_stats: Record<string, number>;
  cache_enabled: boolean;
  /** P2.b 累计缓存节省金额（USD）。*/
  total_cache_saved?: number;
}

export interface UsageRow {
  model_name: string;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost: number;
  request_count: number;
  cache_hits: number;
  /** P2.b 该模型缓存命中累计节省。*/
  cache_saved?: number;
}

export interface Budget {
  id: number;
  scope: string;
  scope_id: string;
  max_budget: number;
  duration: string;
}

export interface BudgetCheck {
  within_budget: boolean;
  budget: Budget | null;
  spent: number;
}

export interface ConversationMeta {
  id: string;
  title: string;
  updated_at: string;
  message_count: number;
}

export interface Conversation {
  id: string;
  title: string;
  messages: ChatMessage[];
}

export interface ChatMessage {
  role: string;
  content: string;
  /** Optional metadata persisted alongside the message (model, tokens, cost,
   * cache state, plan, status). Old conversations have no meta; the UI must
   * tolerate undefined. See CONTEXT-PLAN §3.1. */
  meta?: Record<string, any>;
}

export interface SseEvent {
  event: string;
  data: any;
}

// ── HTTP helpers ──

async function jget<T>(path: string): Promise<T> {
  const res = await fetch(path);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

async function jpost<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? '{}' : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

async function jdelete<T>(path: string): Promise<T> {
  const res = await fetch(path, { method: 'DELETE' });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

async function jput<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? '{}' : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

// ── Service endpoints ──

export function getServicesStatus(): Promise<ServicesStatus> {
  return jget('/api/services/status');
}

export function startService(name: string): Promise<{ message: string }> {
  return jpost(`/api/services/${name}/start`);
}

export function stopService(name: string): Promise<{ message: string }> {
  return jpost(`/api/services/${name}/stop`);
}

export function restartService(name: string): Promise<{ message: string }> {
  return jpost(`/api/services/${name}/restart`);
}

export function getServiceLogs(name: string): Promise<{ logs: string }> {
  return jget(`/api/services/${name}/logs`);
}

export function smartRestart(changedKeys: string[]): Promise<{ ok: boolean; restarted: string[]; errors: string[] }> {
  return jpost('/api/services/smart-restart', { changed_keys: changedKeys });
}

export function shutdownAll(): Promise<{ shutting_down: boolean }> {
  return jpost('/api/shutdown');
}

// ── Semantic cache ──

export function cacheInit(): Promise<{ status: string; detail?: string }> {
  return jpost('/api/cache/init');
}

export interface CacheStatus {
  status: string;
  ready: boolean;
  elapsed: number;
  timeout: number;
  detail: string;
  error: string;
  // Byte-level download progress reported by the model provisioner.
  phase: string;
  mirror: string;
  file: string;
  percent: number;
  file_done: number;
  file_total: number;
  file_percent: number;
  done_bytes: number;
  total_bytes: number;
  speed_bps: number;
}

export function cacheStatus(): Promise<CacheStatus> {
  return jget('/api/cache/status');
}

export function cacheCleanup(): Promise<{
  cleaned: boolean;
  removed_dir: boolean;
  model_kept: boolean;
  purged: string[];
}> {
  return jpost('/api/cache/cleanup');
}

export interface CacheThresholdInfo {
  threshold: number;
  auto_tune: boolean;
  labeled_samples: number;
  suggested: string | null;
}

export function cacheThresholdGet(): Promise<CacheThresholdInfo> {
  return jget('/api/cache/threshold');
}

// `threshold` (manual override) or `autoTune` toggle; both optional.
export function cacheThresholdSet(
  opts: { threshold?: number; autoTune?: boolean },
): Promise<{ ok: boolean; auto_tune: boolean; threshold: number }> {
  return jpost('/api/cache/threshold', opts);
}

// Inline "did this cached answer help?" feedback used for self-tuning.
export function cacheFeedback(
  sim: number,
  decision: 'hit' | 'miss',
  correct: boolean,
): Promise<{ ok: boolean; threshold: number; suggested: number | null; auto_tune: boolean }> {
  return jpost('/api/cache/feedback', { sim, decision, correct });
}

// ── Pricing (PriceSpec) + probe (PRICING-PLAN §10) ──

export interface PriceSpec {
  provider: string;
  model: string;
  input_cost: number;
  output_cost: number;
  cache_read_cost: number | null;
  cache_write_cost: number | null;
  reasoning_cost: number | null;
  tiered: TierBand[] | null;
  zone_ref: string | null;
  batch_multiplier: number;
  price_source: string;
  price_stale: boolean;
  effective_from: string | null;
}

export interface TierBand {
  max_input: number;
  input_cost: number;
  output_cost: number;
  cache_read_cost: number | null;
  cache_write_cost: number | null;
  reasoning_cost: number | null;
}

export interface CalibrationRow {
  provider: string;
  model: string;
  as_of: string;
  calls: number;
  est_cost: number;
  act_cost: number;
  input_side_ratio: number;
  cache_hit_rate: number;
  out_in_ratio: number;
  field_missing_count: number;
}

export interface ProbeStats {
  monthly_limit_usd: number;
  monthly_limit_cny: number;
  spend_usd: number;
  rounds: number;
  hit_verifications: number;
  hit_failures: number;
  failures: number;
}

export function listPriceSpecs(staleOnly = false): Promise<PriceSpec[]> {
  return jget(`/api/pricing/specs${staleOnly ? '?stale=true' : ''}`);
}

export function updatePriceSpec(
  provider: string,
  model: string,
  body: Partial<PriceSpec>,
): Promise<{ ok: boolean; provider: string; model: string }> {
  return jput(`/api/pricing/specs/${encodeURIComponent(provider)}/${encodeURIComponent(model)}`, body);
}

export function acceptPriceSpec(provider: string, model: string): Promise<{ ok: boolean; provider: string; model: string }> {
  return jpost(`/api/pricing/specs/${encodeURIComponent(provider)}/${encodeURIComponent(model)}/accept`);
}

export function refreshPricing(): Promise<{ ok: boolean; updated: number; remote_total: number; manual_kept: number }> {
  return jpost('/api/pricing/refresh');
}

export function listPriceCalibration(days = 30): Promise<CalibrationRow[]> {
  return jget(`/api/pricing/calibration?days=${days}`);
}

export function getProbeStats(): Promise<ProbeStats> {
  return jget('/api/probe/stats');
}

export function setProbeBudget(monthlyLimitCny: number): Promise<{ ok: boolean; monthly_limit_usd: number }> {
  return jput('/api/probe/budget', { monthly_limit_cny: monthlyLimitCny });
}

// ── Model endpoints ──

export function getModels(): Promise<{ models: Model[] }> {
  return jget('/api/models');
}

export function addModel(m: Partial<Model>): Promise<{ id: number }> {
  return jpost('/api/models', m);
}

export function removeModel(name: string): Promise<{ deleted: boolean }> {
  return jdelete(`/api/models/${encodeURIComponent(name)}`);
}

export function updateModel(name: string, updates: Partial<Model>): Promise<{ updated: boolean }> {
  return jput(`/api/models/${encodeURIComponent(name)}`, updates);
}

// ── Stats / usage ──

// N2.a 路由体检报告（三线成本/质量、AIQ、节省额、预算档分布、权重建议）
export interface RoutingReviewLine {
  cost: number;
  quality: number;
}

export interface RoutingSuggestion {
  task_type: string;
  samples: number;
  current: {
    model: string;
    est_cost: number;
    quality: number;
    cost_weight: number;
    quality_weight: number;
  };
  suggested: {
    model: string;
    est_cost: number;
    quality: number;
    cost_weight: number;
    quality_weight: number;
    latency_weight: number;
  };
}

export interface RoutingReview {
  ok: boolean;
  error?: string;
  id?: number;
  created_at?: string;
  samples?: number;
  weak?: RoutingReviewLine;
  current?: RoutingReviewLine;
  strong?: RoutingReviewLine;
  aiq?: number;
  saved_pct?: number;
  conclusion?: string;
  budget_tiers?: Record<string, number>;
  suggestions?: RoutingSuggestion[];
}

export function getRoutingReview(): Promise<RoutingReview> {
  return jget('/api/routing/review');
}

/** 手动跑一轮体检（幂等追加一份报告；无影子样本时返回 ok:false）。 */
export function refreshRoutingReview(): Promise<{ ok: boolean; error?: string; id?: number }> {
  return jpost('/api/routing/review/refresh');
}

/** 采纳建议权重（task_type 缺省 = 全部采纳；下一请求即生效）。 */
export function adoptRoutingSuggestion(taskType?: string): Promise<{ ok: boolean; adopted?: unknown[] }> {
  return jpost('/api/routing/review/adopt', taskType ? { task_type: taskType } : {});
}

export function getStats(): Promise<UsageStats> {
  return jget('/api/stats');
}

export function getUsage(): Promise<{ usage: UsageRow[]; total_spend: number; total_cache_saved?: number }> {
  return jget('/api/usage');
}

// ── Budgets ──

export function getBudgets(): Promise<{ budgets: Budget[] }> {
  return jget('/api/budgets');
}

export function setBudget(scope: string, scopeId: string, maxBudget: number, duration: string): Promise<{ set: boolean }> {
  return jpost('/api/budgets', { scope, scope_id: scopeId, max_budget: maxBudget, duration });
}

export function checkBudget(scope: string, scopeId: string): Promise<BudgetCheck> {
  return jget(`/api/budgets/check?scope=${encodeURIComponent(scope)}&scope_id=${encodeURIComponent(scopeId)}`);
}

// ── Config / env ──

export function readEnv(): Promise<Record<string, string>> {
  return jget('/api/config');
}

export function writeEnvBatch(updates: Record<string, string>): Promise<{ updated: string[] }> {
  return jpost('/api/config', { updates });
}

// ── Conversations ──

export function listConversations(): Promise<{ conversations: ConversationMeta[] }> {
  return jget('/api/conversations');
}

export function loadConversation(id: string): Promise<Conversation> {
  return jget(`/api/conversations/${encodeURIComponent(id)}`);
}

export function saveConversation(c: { id?: string; title?: string; messages: ChatMessage[] }): Promise<{ id: string; saved: boolean }> {
  return jpost('/api/conversations', c);
}

export function deleteConversation(id: string): Promise<{ deleted: boolean }> {
  return jdelete(`/api/conversations/${encodeURIComponent(id)}`);
}

export function renameConversation(id: string, title: string): Promise<{ id: string; renamed: boolean }> {
  return jput(`/api/conversations/${encodeURIComponent(id)}`, { title });
}

// ── Two-phase persistence (CONTEXT-PLAN §3.1) ──
// The user message and assistant placeholder are appended BEFORE the LLM call;
// the placeholder is filled in (content + meta) once the stream completes.

export function appendMessage(
  id: string,
  msg: { role: string; content: string; meta?: Record<string, any> },
): Promise<{ id: string; seq: number; appended: boolean }> {
  return jpost(`/api/conversations/${encodeURIComponent(id)}/messages`, msg);
}

export function updateMessage(
  id: string,
  seq: number,
  patch: { content?: string; meta?: Record<string, any> },
): Promise<{ id: string; seq: number; updated: boolean }> {
  return fetch(`/api/conversations/${encodeURIComponent(id)}/messages/${seq}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  }).then((r) => {
    if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
    return r.json();
  });
}

// ── SSE: chat / orchestrate ──

export async function orchestrateStream(query: string, history: ChatMessage[]): Promise<SseEvent[]> {
  const res = await fetch('/api/orchestrate/stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, history }),
  });
  const text = await res.text();
  return parseSse(text);
}

export async function chatStream(messages: ChatMessage[]): Promise<SseEvent[]> {
  const res = await fetch('/api/chat/stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ messages }),
  });
  const text = await res.text();
  return parseSse(text);
}

export function parseSse(text: string): SseEvent[] {
  const events: SseEvent[] = [];
  let cur: SseEvent | null = null;
  for (const line of text.split('\n')) {
    if (line.startsWith('event:')) {
      if (cur) events.push(cur);
      cur = { event: line.slice(7).trim(), data: undefined };
    } else if (line.startsWith('data: ')) {
      const d = line.slice(6);
      try {
        const parsed = JSON.parse(d);
        if (cur && cur.data === undefined) {
          cur.data = parsed;
        } else {
          events.push({ event: 'message', data: parsed });
        }
      } catch {
        /* skip */
      }
    }
  }
  if (cur) events.push(cur);
  return events;
}

/**
 * Incremental orchestration stream. Reads the SSE body as it arrives and calls
 * `onEvent` for every completed event (including `token` deltas), so the UI can
 * render the answer word-by-word without waiting for the full response.
 */
export async function streamOrchestrate(
  query: string,
  history: ChatMessage[],
  onEvent: (ev: SseEvent) => void,
  conversationId?: string,
): Promise<void> {
  const body: Record<string, unknown> = { query, history };
  if (conversationId) body.conversation_id = conversationId;
  const res = await fetch('/api/orchestrate/stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok || !res.body) {
    onEvent({ event: 'error', data: { detail: `HTTP ${res.status}` } });
    return;
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  let eventName: string | null = null;
  let dataBuf = '';
  const flush = () => {
    if (eventName !== null && dataBuf) {
      try {
        onEvent({ event: eventName, data: JSON.parse(dataBuf) });
      } catch {
        /* ignore malformed */
      }
      dataBuf = '';
    }
    eventName = null;
  };
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let nl: number;
    while ((nl = buf.indexOf('\n')) >= 0) {
      let line = buf.slice(0, nl);
      buf = buf.slice(nl + 1);
      line = line.replace(/\r$/, '');
      if (line === '') {
        flush();
      } else if (line.startsWith('event:')) {
        eventName = line.slice(6).trim();
      } else if (line.startsWith('data:')) {
        dataBuf += line.slice(5).replace(/^ /, '');
      }
    }
  }
  flush();
}
