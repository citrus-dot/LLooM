import { useSyncExternalStore } from 'react';
import {
  listConversations,
  loadConversation,
  saveConversation,
  deleteConversation,
  renameConversation,
  appendMessage,
  updateMessage,
  streamOrchestrate,
  ConversationMeta,
  ChatMessage,
  SseEvent,
} from '../api';

export interface SubTaskView {
  id: number;
  description: string;
  model?: string;
  task_type?: string;
  status?: 'pending' | 'running' | 'done' | 'failed';
  duration?: number;
  error?: string;
}

export interface PlanView {
  sub_tasks: SubTaskView[];
  models_used?: string[];
  aggregator?: string;
  total_duration?: number;
  cached?: boolean;
}

export interface DisplayMsg extends ChatMessage {
  detail?: string;
  plan?: PlanView;
  cacheHit?: boolean;
  cacheSim?: number;
  /** Persisted seq in the conversation (used for two-phase updates). */
  seq?: number;
  /** Lifecycle marker for crash recovery: 'generating' on load becomes 'interrupted'. */
  status?: string;
  model?: string;
  inputTokens?: number;
  outputTokens?: number;
  cost?: number;
  /** C1: reasoning model's chain-of-thought (collapsed in UI, persisted in meta). */
  reasoning?: string;
}

export interface ConvState {
  messages: DisplayMsg[];
  loading: boolean;
  loaded: boolean;
  input: string;
}

const emptyState = (): ConvState => ({ messages: [], loading: false, loaded: false, input: '' });

interface StoreState {
  convs: ConversationMeta[];
  activeId: string | null;
  convMap: Record<string, ConvState>;
  draft: ConvState;
}

let state: StoreState = {
  convs: [],
  activeId: null,
  convMap: {},
  draft: emptyState(),
};

const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());
const subscribe = (cb: () => void) => {
  listeners.add(cb);
  return () => listeners.delete(cb);
};
const getSnapshot = () => state;

type Updater = (s: StoreState) => StoreState;
const update = (u: Updater) => {
  state = u(state);
  emit();
};

const getConv = (id: string | null): ConvState => {
  if (id === null) return state.draft;
  return state.convMap[id] ?? emptyState();
};

const setConv = (id: string | null, updater: (c: ConvState) => ConvState) => {
  if (id === null) {
    update((s) => ({ ...s, draft: updater(s.draft) }));
  } else {
    update((s) => ({ ...s, convMap: { ...s.convMap, [id]: updater(s.convMap[id] ?? emptyState()) } }));
  }
};

export async function refreshConvs() {
  try {
    const { conversations } = await listConversations();
    update((s) => ({ ...s, convs: conversations }));
  } catch {
    /* ignore */
  }
}

export async function selectConv(id: string) {
  update((s) => ({ ...s, activeId: id }));
  if (state.convMap[id]?.loaded) return;
  setConv(id, (c) => ({ ...c, loading: true }));
  try {
    const conv = await loadConversation(id);
    // Map persisted messages — including their meta (model/tokens/cost/plan/
    // status) — so reloaded conversations keep their plan cards and cost tags
    // instead of collapsing to bare role/content.
    update((s) => ({
      ...s,
      convMap: {
        ...s.convMap,
        [id]: {
          messages: conv.messages.map((m) => {
            const meta = (m as any).meta || {};
            return {
              role: m.role,
              content: m.content,
              seq: (m as any).seq,
              status: meta.status,
              model: meta.model,
              inputTokens: meta.input_tokens,
              outputTokens: meta.output_tokens,
              cost: meta.cost,
              cacheHit: meta.cache_hit,
              cacheSim: meta.cache_sim,
              plan: meta.plan,
              reasoning: meta.reasoning,
            } as DisplayMsg;
          }),
          loading: false,
          loaded: true,
          input: '',
        },
      },
    }));
  } catch {
    setConv(id, (c) => ({ ...c, loading: false }));
  }
}

export function newConv() {
  update((s) => ({ ...s, activeId: null, draft: emptyState() }));
}

export async function deleteConv(id: string) {
  try {
    await deleteConversation(id);
    update((s) => {
      const convMap = { ...s.convMap };
      delete convMap[id];
      return { ...s, convMap, activeId: s.activeId === id ? null : s.activeId };
    });
    refreshConvs();
  } catch {
    /* ignore */
  }
}

export async function renameConv(id: string, title: string) {
  const t = title.trim();
  if (!t) return;
  try {
    await renameConversation(id, t);
  } catch {
    /* ignore */
  }
  refreshConvs();
}

export function setInput(v: string) {
  setConv(state.activeId, (c) => ({ ...c, input: v }));
}

async function persistPhase1(
  targetId: string | null,
  userContent: string,
): Promise<{ id: string; userSeq: number; asstSeq: number }> {
  // Phase 1: append the user message + an empty assistant placeholder to disk
  // BEFORE the LLM call, so a crash mid-answer never loses the user's input.
  // New conversations (targetId === null) are created by the first append.
  let id = targetId;
  if (id === null) {
    // saveConversation creates the conversation row with the first message.
    const res = await saveConversation({
      title: userContent.slice(0, 20),
      messages: [{ role: 'user', content: userContent }],
    });
    id = res.id;
    const asst = await appendMessage(id, {
      role: 'assistant',
      content: '',
      meta: { status: 'generating', created_at: new Date().toISOString() },
    });
    return { id, userSeq: 1, asstSeq: asst.seq };
  }
  const user = await appendMessage(id, { role: 'user', content: userContent });
  const asst = await appendMessage(id, {
    role: 'assistant',
    content: '',
    meta: { status: 'generating', created_at: new Date().toISOString() },
  });
  return { id, userSeq: user.seq, asstSeq: asst.seq };
}

export async function send() {
  const targetId = state.activeId;
  const cur = getConv(targetId);
  const q = cur.input.trim();
  if (!q || cur.loading) return;

  const userMsg: DisplayMsg = { role: 'user', content: q };
  const placeholder: DisplayMsg = { role: 'assistant', content: '', status: 'generating' };
  // Optimistic UI: show the turn immediately.
  setConv(targetId, (c) => ({
    ...c,
    messages: [...c.messages, userMsg, placeholder],
    loading: true,
    input: '',
  }));

  let phase: { id: string; userSeq: number; asstSeq: number };
  try {
    phase = await persistPhase1(targetId, q);
  } catch (e: any) {
    const errMsg: DisplayMsg = { ...placeholder, content: `保存失败: ${e}`, status: 'done' };
    setConv(targetId, (c) => ({
      ...c,
      messages: c.messages.slice(0, -1).concat(errMsg),
      loading: false,
    }));
    return;
  }
  // Promote draft → persisted conversation in the store.
  if (targetId === null) {
    const draftMsgs = [...cur.messages, userMsg, placeholder];
    update((s) => ({
      ...s,
      activeId: phase.id,
      draft: emptyState(),
      convMap: {
        ...s.convMap,
        [phase.id]: {
          messages: draftMsgs.map((m, i) => ({
            ...m,
            seq: i === draftMsgs.length - 1 ? phase.asstSeq : i === draftMsgs.length - 2 ? phase.userSeq : undefined,
          })),
          loading: true,
          loaded: true,
          input: '',
        },
      },
    }));
  } else {
    setConv(phase.id, (c) => ({
      ...c,
      messages: c.messages.map((m, i, arr) => {
        const len = arr.length;
        if (i === len - 1) return { ...m, seq: phase.asstSeq, status: 'generating' };
        if (i === len - 2) return { ...m, seq: phase.userSeq };
        return m;
      }),
    }));
  }
  refreshConvs();

  const convId = phase.id;
  const asstSeq = phase.asstSeq;
  const plan: PlanView = { sub_tasks: [] };
  let response = '';
  let errorMsg: string | null = null;
  let cached = false;
  let cacheSim: number | undefined;
  let reasoning: string | undefined;
  let modelsUsed: string[] = [];
  let totalDuration = 0;
  let usage = { input_tokens: 0, output_tokens: 0, cost: 0, saved_cost: 0, model: '' };

  const patchPlan = () => {
    setConv(convId, (c) => ({
      ...c,
      messages: c.messages.map((m, i) =>
        i === c.messages.length - 1 ? { ...m, plan: { ...plan, sub_tasks: [...plan.sub_tasks] } } : m,
      ),
    }));
  };

  try {
    await streamOrchestrate(
      q,
      [], // server builds history from the conversation store
      (ev: SseEvent) => {
        const d = (ev.data || {}) as any;
        if (ev.event === 'decompose' && d.sub_tasks) {
          plan.sub_tasks = d.sub_tasks.map((t: any) => ({
            id: t.id,
            description: t.description,
            model: t.selected_model,
            task_type: t.task_type,
            status: 'pending',
          }));
          patchPlan();
        } else if (ev.event === 'task_start' && d.model) {
          const id = d.id;
          plan.sub_tasks = plan.sub_tasks.map((t) => (t.id === id ? { ...t, model: d.model, status: 'running' } : t));
          patchPlan();
        } else if (ev.event === 'token' && d.delta) {
          response += d.delta;
          setConv(convId, (c) => ({
            ...c,
            messages: c.messages.map((m, i) => (i === c.messages.length - 1 ? { ...m, content: response } : m)),
          }));
        } else if (ev.event === 'task_done') {
          const id = d.id;
          plan.sub_tasks = plan.sub_tasks.map((t) =>
            t.id === id
              ? {
                  ...t,
                  status: d.error ? 'failed' : 'done',
                  duration: d.duration,
                  model: d.model || t.model,
                  error: d.error,
                }
              : t,
          );
          patchPlan();
        } else if (ev.event === 'result' && d.response !== undefined) {
          response = d.response;
          cached = !!d.cache_hit;
          cacheSim = typeof d.cache_sim === 'number' ? d.cache_sim : undefined;
          if (d.models_used) modelsUsed = d.models_used;
          if (typeof d.total_duration === 'number') totalDuration = d.total_duration;
          if (typeof d.input_tokens === 'number') usage = { ...usage, input_tokens: d.input_tokens };
          if (typeof d.output_tokens === 'number') usage = { ...usage, output_tokens: d.output_tokens };
          if (typeof d.cost === 'number') usage = { ...usage, cost: d.cost };
          if (typeof d.saved_cost === 'number') usage = { ...usage, saved_cost: d.saved_cost };
          if (typeof d.model === 'string') usage = { ...usage, model: d.model };
        } else if (d && d.error) {
          errorMsg = d.block_reason || d.detail || d.error || '未知错误';
        }
      },
      convId,
    );

    if (errorMsg) {
      const finalContent = `请求被拦截: ${errorMsg}`;
      const meta = { status: 'done', error: true };
      await updateMessage(convId, asstSeq, { content: finalContent, meta });
      setConv(convId, (c) => ({
        ...c,
        messages: c.messages.map((m, i) =>
          i === c.messages.length - 1 ? { ...m, content: finalContent, status: 'done' } : m,
        ),
        loading: false,
      }));
      return;
    }

    const uniq = [...new Set(modelsUsed.length ? modelsUsed : usage.model ? [usage.model] : [])];
    const parts: string[] = [];
    if (uniq.length) parts.push(`模型: ${uniq.join(' + ')}`);
    if (plan.sub_tasks.length > 1) parts.push(`${plan.sub_tasks.length} 个子任务`);
    if (totalDuration) parts.push(`${totalDuration.toFixed(1)}s`);
    if (usage.input_tokens || usage.output_tokens)
      parts.push(`${usage.input_tokens}/${usage.output_tokens} tok`);
    if (usage.cost) parts.push(`¥${usage.cost.toFixed(4)}`);
    if (cached) parts.push('缓存命中');
    if (usage.saved_cost) parts.push(`省¥${usage.saved_cost.toFixed(4)}`);
    const detail = parts.join(' · ') || undefined;
    const meta = {
      status: 'done',
      model: usage.model || (uniq[0] ?? undefined),
      input_tokens: usage.input_tokens,
      output_tokens: usage.output_tokens,
      cost: usage.cost,
      saved_cost: usage.saved_cost,
      cache_hit: cached,
      cache_sim: cacheSim,
      plan: plan.sub_tasks.length ? plan : undefined,
    };
    await updateMessage(convId, asstSeq, { content: response || '(无返回)', meta });
    setConv(convId, (c) => ({
      ...c,
      messages: c.messages.map((m, i) =>
        i === c.messages.length - 1
          ? {
              ...m,
              content: response || '(无返回)',
              detail,
              plan: plan.sub_tasks.length ? { ...plan } : undefined,
              cacheHit: cached,
              cacheSim,
              reasoning,
              status: 'done',
              model: meta.model,
              inputTokens: usage.input_tokens,
              outputTokens: usage.output_tokens,
              cost: usage.cost,
            }
          : m,
      ),
      loading: false,
    }));
  } catch (e: any) {
    const finalContent = `请求失败: ${e}`;
    const meta = { status: 'done', error: true };
    try {
      await updateMessage(convId, asstSeq, { content: finalContent, meta });
    } catch {
      /* best-effort */
    }
    setConv(convId, (c) => ({
      ...c,
      messages: c.messages.map((m, i) =>
        i === c.messages.length - 1 ? { ...m, content: finalContent, status: 'done' } : m,
      ),
      loading: false,
    }));
  }
}

export function useChat(): StoreState {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
