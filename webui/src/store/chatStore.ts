import { useSyncExternalStore } from 'react';
import {
  listConversations,
  loadConversation,
  saveConversation,
  deleteConversation,
  renameConversation,
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
    update((s) => ({
      ...s,
      convMap: {
        ...s.convMap,
        [id]: { messages: conv.messages.map((x) => ({ ...x })), loading: false, loaded: true, input: '' },
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

async function persist(targetId: string | null, finalMsgs: DisplayMsg[]) {
  const messages: ChatMessage[] = finalMsgs.map((m) => ({ role: m.role, content: m.content }));
  const firstUser = finalMsgs.find((m) => m.role === 'user');
  // 仅新建（草稿首次保存）带 title 自动命名；已存在对话传空，后端保留原值，
  // 因此用户编辑过的标题在任何后续对话中都不会被覆盖。
  const title = targetId === null ? (firstUser ? firstUser.content.slice(0, 20) : '新对话') : '';
  try {
    const res = await saveConversation({ id: targetId ?? undefined, title, messages });
    if (targetId === null) {
      update((s) => ({
        ...s,
        activeId: res.id,
        draft: emptyState(),
        convMap: { ...s.convMap, [res.id]: { messages: finalMsgs, loading: false, loaded: true, input: '' } },
      }));
    } else {
      update((s) => ({
        ...s,
        convMap: { ...s.convMap, [res.id]: { ...(s.convMap[res.id] ?? emptyState()), messages: finalMsgs, loaded: true } },
      }));
    }
    refreshConvs();
  } catch {
    /* silent */
  }
}

export async function send() {
  const targetId = state.activeId;
  const cur = getConv(targetId);
  const q = cur.input.trim();
  if (!q || cur.loading) return;

  const userMsg: DisplayMsg = { role: 'user', content: q };
  const placeholder: DisplayMsg = { role: 'assistant', content: '' };
  const next = [...cur.messages, userMsg, placeholder];
  setConv(targetId, (c) => ({ ...c, messages: next, loading: true, input: '' }));

  const history: ChatMessage[] = cur.messages
    .filter((m) => m.role === 'user' || m.role === 'assistant')
    .map((m) => ({ role: m.role, content: m.content }));

  const plan: PlanView = { sub_tasks: [] };
  let response = '';
  let errorMsg: string | null = null;
  let cached = false;
  let cacheSim: number | undefined;
  let modelsUsed: string[] = [];
  let totalDuration = 0;

  const patchPlan = () => {
    setConv(targetId, (c) => ({
      ...c,
      messages: c.messages.map((m, i) =>
        i === c.messages.length - 1 ? { ...m, plan: { ...plan, sub_tasks: [...plan.sub_tasks] } } : m,
      ),
    }));
  };

  try {
    await streamOrchestrate(q, history, (ev: SseEvent) => {
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
        setConv(targetId, (c) => ({
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
        if (d.aggregator) plan.aggregator = d.aggregator;
        if (typeof d.total_duration === 'number') totalDuration = d.total_duration;
      } else if (d && d.error) {
        errorMsg = d.block_reason || d.detail || d.error || '未知错误';
      }
    });

    const finalMsgs = next.slice(0, -1);
    if (errorMsg) {
      const errMsg: DisplayMsg = { role: 'assistant', content: `请求被拦截: ${errorMsg}` };
      const msgs = [...finalMsgs, errMsg];
      setConv(targetId, (c) => ({ ...c, messages: msgs, loading: false }));
      await persist(targetId, msgs);
      return;
    }

    const uniq = [...new Set(modelsUsed)];
    const parts: string[] = [];
    if (uniq.length) parts.push(`模型: ${uniq.join(' + ')}`);
    if (plan.sub_tasks.length > 1) parts.push(`${plan.sub_tasks.length} 个子任务`);
    if (totalDuration) parts.push(`${totalDuration.toFixed(1)}s`);
    if (cached) parts.push('缓存命中');
    const detail = parts.join(' · ') || undefined;
    const aiMsg: DisplayMsg = {
      role: 'assistant',
      content: response || '(无返回)',
      detail,
      plan: plan.sub_tasks.length ? { ...plan } : undefined,
      cacheHit: cached,
      cacheSim,
    };
    const msgs = [...finalMsgs, aiMsg];
    setConv(targetId, (c) => ({ ...c, messages: msgs, loading: false }));
    await persist(targetId, msgs);
  } catch (e: any) {
    const errMsg: DisplayMsg = { role: 'assistant', content: `请求失败: ${e}` };
    const msgs = [...next.slice(0, -1), errMsg];
    setConv(targetId, (c) => ({ ...c, messages: msgs, loading: false }));
    await persist(targetId, msgs);
  }
}

export function useChat(): StoreState {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
