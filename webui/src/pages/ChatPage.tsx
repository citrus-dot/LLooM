import { useEffect, useRef, useState } from 'react';
import { Layout, Input, Button, List, Avatar, Spin, Tag, message } from 'antd';
import { PlusOutlined, SendOutlined, DeleteOutlined, RobotOutlined, UserOutlined } from '@ant-design/icons';
import {
  listConversations,
  loadConversation,
  saveConversation,
  deleteConversation,
  orchestrateStream,
  ConversationMeta,
  ChatMessage,
  SseEvent,
} from '../api';

interface DisplayMsg extends ChatMessage {
  detail?: string;
}

const { Sider, Content } = Layout;

export default function ChatPage() {
  const [convs, setConvs] = useState<ConversationMeta[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [msgs, setMsgs] = useState<DisplayMsg[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

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
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [msgs, loading]);

  const selectConv = async (id: string) => {
    setActiveId(id);
    try {
      const conv = await loadConversation(id);
      setMsgs(conv.messages.map((m) => ({ ...m })));
    } catch (e) {
      message.error(`加载对话失败: ${e}`);
    }
  };

  const newConv = () => {
    setActiveId(null);
    setMsgs([]);
  };

  const delConv = async (id: string) => {
    try {
      await deleteConversation(id);
      if (activeId === id) {
        setActiveId(null);
        setMsgs([]);
      }
      refreshConvs();
    } catch (e) {
      message.error(`删除失败: ${e}`);
    }
  };

  const persist = async (finalMsgs: DisplayMsg[]) => {
    const messages: ChatMessage[] = finalMsgs.map((m) => ({ role: m.role, content: m.content }));
    const firstUser = finalMsgs.find((m) => m.role === 'user');
    const title = firstUser ? firstUser.content.slice(0, 20) : '新对话';
    try {
      const res = await saveConversation({ id: activeId ?? undefined, title, messages });
      setActiveId(res.id);
      refreshConvs();
    } catch {
      /* silent */
    }
  };

  const send = async () => {
    const q = input.trim();
    if (!q || loading) return;
    setInput('');
    const userMsg: DisplayMsg = { role: 'user', content: q };
    const next = [...msgs, userMsg];
    setMsgs(next);
    setLoading(true);

    try {
      const history: ChatMessage[] = next
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .map((m) => ({ role: m.role, content: m.content }));

      const events: SseEvent[] = await orchestrateStream(q, history);

      let response = '';
      const modelsUsed: string[] = [];
      let errorMsg: string | null = null;
      let cached = false;

      for (const ev of events) {
        const d = ev.data || ev;
        if (ev.event === 'task_start' && d.model) modelsUsed.push(d.model);
        else if (ev.event === 'result' && d.response !== undefined) {
          response = d.response;
          cached = !!d.cache_hit;
        } else if (d.error) errorMsg = d.block_reason || d.detail || '未知错误';
      }

      if (errorMsg) {
        const errMsg: DisplayMsg = { role: 'assistant', content: `请求被拦截: ${errorMsg}` };
        const finalMsgs = [...next, errMsg];
        setMsgs(finalMsgs);
        await persist(finalMsgs);
      } else {
        const uniq = [...new Set(modelsUsed)];
        let detail = uniq.length ? `调用模型: ${uniq.join(' | ')}` : undefined;
        if (cached) detail = detail ? `${detail} · 来自缓存` : '来自语义缓存';
        const aiMsg: DisplayMsg = {
          role: 'assistant',
          content: response || '(无返回)',
          detail,
        };
        const finalMsgs = [...next, aiMsg];
        setMsgs(finalMsgs);
        await persist(finalMsgs);
      }
    } catch (e) {
      const errMsg: DisplayMsg = { role: 'assistant', content: `请求失败: ${e}` };
      const finalMsgs = [...next, errMsg];
      setMsgs(finalMsgs);
      await persist(finalMsgs);
    } finally {
      setLoading(false);
    }
  };

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
            renderItem={(c) => (
              <List.Item
                style={{
                  cursor: 'pointer',
                  padding: '8px 12px',
                  borderRadius: 6,
                  background: c.id === activeId ? '#e6f4ff' : 'transparent',
                }}
                onClick={() => selectConv(c.id)}
                actions={[
                  <DeleteOutlined
                    key="del"
                    onClick={(e) => {
                      e.stopPropagation();
                      delConv(c.id);
                    }}
                  />,
                ]}
              >
                <List.Item.Meta
                  title={<span style={{ fontSize: 13 }}>{c.title || '新对话'}</span>}
                  description={<span style={{ fontSize: 12, color: '#999' }}>{c.message_count} 条</span>}
                />
              </List.Item>
            )}
          />
          {convs.length === 0 && (
            <div style={{ color: '#999', textAlign: 'center', padding: 24 }}>暂无对话<br />输入消息自动新建</div>
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
          {msgs.length === 0 && !loading && (
            <div style={{ color: '#999', textAlign: 'center', padding: 40 }}>
              你好！我是智能编排助手。复杂任务会自动分解为子任务，选择最优模型执行。
            </div>
          )}
          {msgs.map((m, i) =>
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
                  <Avatar size="small" icon={<UserOutlined />} />
                </div>
              </div>
            ) : (
              <div key={i} style={{ display: 'flex', justifyContent: 'flex-start', marginBottom: 12 }}>
                <div style={{ maxWidth: '70%', display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                  <Avatar size="small" icon={<RobotOutlined />} style={{ background: '#0984e3' }} />
                  <div>
                    <div
                      style={{
                        background: '#f6f8fa',
                        padding: '8px 12px',
                        borderRadius: 8,
                        whiteSpace: 'pre-wrap',
                        wordBreak: 'break-word',
                      }}
                    >
                      {m.content}
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
          {loading && (
            <div style={{ textAlign: 'center', padding: 16 }}>
              <Spin />
              <span style={{ marginLeft: 8, color: '#999' }}>思考中...</span>
            </div>
          )}
        </div>

        <div style={{ display: 'flex', gap: 8 }}>
          <Input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onPressEnter={send}
            placeholder="输入消息... (Enter 发送)"
            disabled={loading}
          />
          <Button type="primary" icon={<SendOutlined />} onClick={send} loading={loading}>
            发送
          </Button>
        </div>
      </Content>
    </Layout>
  );
}
