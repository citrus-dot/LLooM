import { useEffect, useRef, useState } from 'react';
import { App, Avatar, Button, Input, Layout, List, Spin, Tag, Tooltip } from 'antd';
import { PlusOutlined, SendOutlined, DeleteOutlined, EditOutlined, RobotOutlined, UserOutlined } from '@ant-design/icons';
import {
  listConversations,
  loadConversation,
  saveConversation,
  deleteConversation,
  renameConversation,
  orchestrateStream,
  ConversationMeta,
  ChatMessage,
  SseEvent,
} from '../api';
import Markdown from '../components/Markdown';

// ── 视图模型 ──

interface SubTaskView {
  id: number;
  description: string;
  model?: string;
  task_type?: string;
  status?: 'pending' | 'running' | 'done' | 'failed';
  duration?: number;
}

interface PlanView {
  sub_tasks: SubTaskView[];
  models_used?: string[];
  aggregator?: string;
  total_duration?: number;
  cached?: boolean;
}

interface DisplayMsg extends ChatMessage {
  detail?: string;
  plan?: PlanView;
}

/** 每个对话拥有一份完全独立的状态：消息、加载中、是否已加载、草稿输入。 */
interface ConvState {
  messages: DisplayMsg[];
  loading: boolean;
  loaded: boolean;
  input: string;
}

const emptyState = (): ConvState => ({ messages: [], loading: false, loaded: false, input: '' });

const { Sider, Content } = Layout;

// 执行计划卡片：展示任务被拆成了哪些子任务、分别由哪个模型执行、当前状态。
function PlanCard({ plan }: { plan: PlanView }) {
  return (
    <div className="plan-card">
      <div className="plan-head">
        <span className="plan-title">智能编排计划</span>
        {plan.models_used && plan.models_used.length > 0 && (
          <span className="plan-models">
            {plan.models_used.map((m) => (
              <Tag key={m} color="geekblue" style={{ marginInlineEnd: 4 }}>
                {m}
              </Tag>
            ))}
          </span>
        )}
      </div>
      <ol className="plan-list">
        {plan.sub_tasks.map((t) => (
          <li key={t.id} className="plan-item">
            <span className={`plan-dot ${t.status ?? 'done'}`} />
            <span className="plan-desc">{t.description}</span>
            {t.model && (
              <Tooltip title={`子任务 #${t.id} 执行模型`}>
                <Tag color="blue" className="plan-model">
                  {t.model}
                </Tag>
              </Tooltip>
            )}
          </li>
        ))}
      </ol>
      {plan.aggregator && (
        <div className="plan-foot">
          汇总模型：<Tag color="purple">{plan.aggregator}</Tag>
        </div>
      )}
    </div>
  );
}

export default function ChatPage() {
  const { message } = App.useApp();
  const [convs, setConvs] = useState<ConversationMeta[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  // 已加载/正在加载的对话状态，按 id 独立存放
  const [convMap, setConvMap] = useState<Record<string, ConvState>>({});
  // 尚未保存的新对话（草稿）单独存放，独立于任何已存对话
  const [draft, setDraft] = useState<ConvState>(emptyState());
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const committingRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  // 当前激活对话的状态（草稿或已存对话）
  const getActive = (): ConvState => (activeId === null ? draft : convMap[activeId] ?? emptyState());
  const state = getActive();

  const setConvState = (id: string | null, updater: (s: ConvState) => ConvState) => {
    if (id === null) setDraft((s) => updater(s));
    else setConvMap((m) => ({ ...m, [id]: updater(m[id] ?? emptyState()) }));
  };

  const refreshConvs = async () => {
    try {
      const { conversations } = await listConversations();
      setConvs(conversations);
    } catch {
      /* ignore */
    }
  };

  useEffect(() => {
    refreshConvs();
  }, []);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
  }, [activeId, state.messages.length, state.loading]);

  const selectConv = async (id: string) => {
    setActiveId(id);
    // 已加载过 → 直接切换，复用内存中的上下文（独立、即时，不再显示统一的加载提示）
    if (convMap[id]?.loaded) return;
    setConvState(id, (s) => ({ ...s, loading: true }));
    try {
      const conv = await loadConversation(id);
      setConvMap((m) => ({
        ...m,
        [id]: { messages: conv.messages.map((x) => ({ ...x })), loading: false, loaded: true, input: '' },
      }));
    } catch (e) {
      message.error(`加载对话失败: ${e}`);
      setConvState(id, (s) => ({ ...s, loading: false }));
    }
  };

  const newConv = () => setActiveId(null);

  const delConv = async (id: string) => {
    try {
      await deleteConversation(id);
      setConvMap((m) => {
        const n = { ...m };
        delete n[id];
        return n;
      });
      if (activeId === id) setActiveId(null);
      refreshConvs();
    } catch (e) {
      message.error(`删除失败: ${e}`);
    }
  };

  const renameConv = async (id: string, title: string) => {
    if (committingRef.current) return;
    committingRef.current = true;
    const t = title.trim();
    setEditingId(null);
    setEditValue('');
    if (t) {
      try {
        await renameConversation(id, t);
      } catch (e) {
        message.error(`重命名失败: ${e}`);
      }
    }
    refreshConvs();
    committingRef.current = false;
  };

  const persist = async (targetId: string | null, finalMsgs: DisplayMsg[]) => {
    const messages: ChatMessage[] = finalMsgs.map((m) => ({ role: m.role, content: m.content }));
    const firstUser = finalMsgs.find((m) => m.role === 'user');
    const title = firstUser ? firstUser.content.slice(0, 20) : '新对话';
    try {
      const res = await saveConversation({ id: targetId ?? undefined, title, messages });
      if (targetId === null) {
        // 草稿首次保存：转为真实对话，保持上下文/状态不丢
        setActiveId(res.id);
        setConvMap((m) => ({
          ...m,
          [res.id]: { messages: finalMsgs, loading: false, loaded: true, input: '' },
        }));
        setDraft(emptyState());
      } else {
        setConvMap((m) => ({ ...m, [res.id]: { ...(m[res.id] ?? emptyState()), messages: finalMsgs, loaded: true } }));
      }
      refreshConvs();
    } catch {
      /* silent */
    }
  };

  const send = async () => {
    // 锁定目标对话：即便发送过程中切换了对话，回复也会落到正确的那条，互不干扰
    const targetId = activeId;
    const cur = getActive();
    const q = cur.input.trim();
    if (!q || cur.loading) return;

    setConvState(targetId, (s) => ({ ...s, input: '' }));
    const userMsg: DisplayMsg = { role: 'user', content: q };
    const next = [...cur.messages, userMsg];
    setConvState(targetId, (s) => ({ ...s, messages: next, loading: true }));

    try {
      const history: ChatMessage[] = next
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .map((m) => ({ role: m.role, content: m.content }));

      const events: SseEvent[] = await orchestrateStream(q, history);

      const plan: PlanView = { sub_tasks: [] };
      let response = '';
      let errorMsg: string | null = null;
      let cached = false;

      for (const ev of events) {
        const d = ev.data || ev;
        if (ev.event === 'decompose' && d.sub_tasks) {
          plan.sub_tasks = d.sub_tasks.map((t: any) => ({
            id: t.id,
            description: t.description,
            model: t.selected_model,
            task_type: t.task_type,
            status: 'done',
          }));
        } else if (ev.event === 'task_start' && d.model) {
          const st = plan.sub_tasks.find((s) => s.id === d.id);
          if (st) {
            st.model = d.model;
            st.status = 'done';
          }
        } else if (ev.event === 'task_done') {
          const st = plan.sub_tasks.find((s) => s.id === d.id);
          if (st) {
            st.status = d.error ? 'failed' : 'done';
            st.duration = d.duration;
          }
        } else if (ev.event === 'result' && d.response !== undefined) {
          response = d.response;
          cached = !!d.cache_hit;
          if (d.models_used) plan.models_used = d.models_used;
          if (d.aggregator) plan.aggregator = d.aggregator;
          if (typeof d.total_duration === 'number') plan.total_duration = d.total_duration;
        } else if (d && d.error) {
          errorMsg = d.block_reason || d.detail || d.error || '未知错误';
        }
      }

      if (errorMsg) {
        const errMsg: DisplayMsg = { role: 'assistant', content: `请求被拦截: ${errorMsg}` };
        const finalMsgs = [...next, errMsg];
        setConvState(targetId, (s) => ({ ...s, messages: finalMsgs, loading: false }));
        await persist(targetId, finalMsgs);
      } else {
        const uniq = [...new Set(plan.models_used && plan.models_used.length ? plan.models_used : [])];
        const parts: string[] = [];
        if (uniq.length) parts.push(`模型: ${uniq.join(' + ')}`);
        if (plan.sub_tasks.length > 1) parts.push(`${plan.sub_tasks.length} 个子任务`);
        if (plan.total_duration) parts.push(`${plan.total_duration.toFixed(1)}s`);
        if (cached) parts.push('缓存命中');
        const detail = parts.join(' · ') || undefined;
        const aiMsg: DisplayMsg = {
          role: 'assistant',
          content: response || '(无返回)',
          detail,
          plan: plan.sub_tasks.length ? plan : undefined,
        };
        const finalMsgs = [...next, aiMsg];
        setConvState(targetId, (s) => ({ ...s, messages: finalMsgs, loading: false }));
        await persist(targetId, finalMsgs);
      }
    } catch (e) {
      const errMsg: DisplayMsg = { role: 'assistant', content: `请求失败: ${e}` };
      const finalMsgs = [...next, errMsg];
      setConvState(targetId, (s) => ({ ...s, messages: finalMsgs, loading: false }));
      await persist(targetId, finalMsgs);
    }
  };

  const onInputChange = (v: string) => setConvState(activeId, (s) => ({ ...s, input: v }));

  return (
    <Layout style={{ background: 'transparent', height: 'calc(100vh - 130px)' }}>
      <Sider width={260} theme="light" style={{ borderRight: '1px solid #f0f0f0', padding: 12 }}>
        <Button type="primary" block icon={<PlusOutlined />} onClick={newConv}>
          新建对话
        </Button>
        <div style={{ marginTop: 12, maxHeight: 'calc(100vh - 220px)', overflow: 'auto' }}>
          <List
            size="small"
            dataSource={convs}
            renderItem={(c) => {
              const isEditing = editingId === c.id;
              return (
                <List.Item
                  style={{
                    cursor: 'pointer',
                    padding: '8px 12px',
                    borderRadius: 6,
                    background: c.id === activeId ? '#e6f4ff' : 'transparent',
                  }}
                  onClick={() => {
                    if (!isEditing) selectConv(c.id);
                  }}
                  onMouseEnter={() => setHoveredId(c.id)}
                  onMouseLeave={() => setHoveredId(null)}
                  actions={
                    isEditing
                      ? []
                      : [
                          <EditOutlined
                            key="edit"
                            title="重命名"
                            style={{
                              opacity: hoveredId === c.id ? 1 : 0,
                              transition: 'opacity 0.15s',
                            }}
                            onClick={(e) => {
                              e.stopPropagation();
                              setEditValue(c.title || '');
                              setEditingId(c.id);
                            }}
                          />,
                          <DeleteOutlined
                            key="del"
                            onClick={(e) => {
                              e.stopPropagation();
                              delConv(c.id);
                            }}
                          />,
                        ]
                  }
                >
                  {isEditing ? (
                    <Input
                      autoFocus
                      size="small"
                      value={editValue}
                      onChange={(e) => setEditValue(e.target.value)}
                      onClick={(e) => e.stopPropagation()}
                      onPressEnter={() => renameConv(c.id, editValue)}
                      onBlur={() => renameConv(c.id, editValue)}
                      style={{ fontSize: 13 }}
                    />
                  ) : (
                    <List.Item.Meta
                      title={<span style={{ fontSize: 13 }}>{c.title || '新对话'}</span>}
                      description={<span style={{ fontSize: 12, color: '#999' }}>{c.message_count} 条</span>}
                    />
                  )}
                </List.Item>
              );
            }}
          />
          {convs.length === 0 && (
            <div style={{ color: '#999', textAlign: 'center', padding: 24 }}>
              暂无对话
              <br />
              输入消息自动新建
            </div>
          )}
        </div>
      </Sider>

      <Content style={{ padding: '0 16px', display: 'flex', flexDirection: 'column' }}>
        <div
          ref={scrollRef}
          style={{
            flex: 1,
            overflow: 'auto',
            background: '#fff',
            border: '1px solid #f0f0f0',
            borderRadius: 8,
            padding: 16,
            marginBottom: 12,
          }}
        >
          {state.messages.length === 0 && !state.loading && (
            <div style={{ color: '#999', textAlign: 'center', padding: 40 }}>
              你好！我是智能编排助手。复杂任务会自动分解为子任务，选择最优模型执行。
            </div>
          )}
          {state.messages.map((m, i) =>
            m.role === 'user' ? (
              <div key={i} style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                <div style={{ maxWidth: '70%', display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                  <div
                    style={{
                      background: '#e6f4ff',
                      padding: '8px 12px',
                      borderRadius: 8,
                      whiteSpace: 'pre-wrap',
                      wordBreak: 'break-word',
                    }}
                  >
                    {m.content}
                  </div>
                  <Avatar size="small" icon={<UserOutlined />} style={{ width: 24, height: 24, flexShrink: 0 }} />
                </div>
              </div>
            ) : (
              <div key={i} style={{ display: 'flex', justifyContent: 'flex-start', marginBottom: 12 }}>
                <div style={{ maxWidth: '70%', display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                  <Avatar
                    size="small"
                    icon={<RobotOutlined />}
                    style={{ background: '#0984e3', width: 24, height: 24, flexShrink: 0 }}
                  />
                  <div style={{ minWidth: 0 }}>
                    {m.plan && (
                      <div style={{ marginBottom: 8 }}>
                        <PlanCard plan={m.plan} />
                      </div>
                    )}
                    <div
                      style={{
                        background: '#f6f8fa',
                        padding: '8px 12px',
                        borderRadius: 8,
                      }}
                    >
                      <Markdown content={m.content} />
                    </div>
                    {m.detail && (
                      <div style={{ marginTop: 4 }}>
                        <Tag color="blue">{m.detail}</Tag>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            ),
          )}
          {state.loading && (
            <div style={{ textAlign: 'center', padding: 16 }}>
              <Spin />
              <span style={{ marginLeft: 8, color: '#999' }}>思考中...</span>
            </div>
          )}
        </div>

        <div style={{ display: 'flex', gap: 8 }}>
          <Input
            value={state.input}
            onChange={(e) => onInputChange(e.target.value)}
            onPressEnter={send}
            placeholder="输入消息... (Enter 发送)"
            disabled={state.loading}
          />
          <Button type="primary" icon={<SendOutlined />} onClick={send} loading={state.loading}>
            发送
          </Button>
        </div>
      </Content>
    </Layout>
  );
}
