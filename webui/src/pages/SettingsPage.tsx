import { useEffect, useRef, useState } from 'react';
import { Card, Row, Col, Button, Space, Tag, Form, Input, message, Descriptions, Progress, Alert } from 'antd';
import {
  CheckOutlined,
  CloseOutlined,
  SaveOutlined,
  ThunderboltOutlined,
  CloudDownloadOutlined,
  DeleteOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import {
  getServicesStatus,
  readEnv,
  writeEnvBatch,
  smartRestart,
  cacheInit,
  cacheStatus,
  cacheCleanup,
  ServiceStatus,
  CacheStatus,
} from '../api';

interface EnvItem {
  key: string;
  label: string;
  type: 'text' | 'password';
  desc: string;
}

const ENV_SECTIONS: { title: string; items: EnvItem[] }[] = [
  {
    title: '阿里云百炼（DashScope）',
    items: [
      { key: 'DASHSCOPE_API_KEY', label: 'API Key', type: 'password', desc: '主要 LLM 供应商' },
      { key: 'DASHSCOPE_API_BASE', label: 'API Base', type: 'text', desc: '默认 dashscope.aliyuncs.com' },
    ],
  },
  {
    title: 'OpenAI',
    items: [
      { key: 'OPENAI_API_KEY', label: 'API Key', type: 'password', desc: 'sk-...' },
      { key: 'OPENAI_BASE_URL', label: 'Base URL', type: 'text', desc: '可选代理地址' },
    ],
  },
  {
    title: 'Anthropic',
    items: [{ key: 'ANTHROPIC_API_KEY', label: 'API Key', type: 'password', desc: 'sk-ant-...' }],
  },
  {
    title: '核心配置',
    items: [
      { key: 'OLLAMA_API_BASE', label: 'Ollama Base', type: 'text', desc: '本地 Ollama 地址' },
      { key: 'LLOOM_WEB_PORT', label: 'Web 端口', type: 'text', desc: '默认 7861' },
      { key: 'LLOOM_DATA_DIR', label: '数据目录', type: 'text', desc: 'SQLite/对话' },
    ],
  },
];

// Operational/internal variables that must NOT be user-editable through the
// settings UI — editing them (e.g. LLOOM_AI_SERVICE_URL / LLOOM_AI_PORT) can
// break the server↔AI-service wiring. They are hidden from the free-form
// "其他配置" section so they can't be mis-edited. User-facing keys such as
// LLOOM_WEB_PORT / OLLAMA_API_BASE / LLOOM_DATA_DIR stay editable (schema).
const INTERNAL_ENV_KEYS = new Set<string>([
  'LLOOM_AI_SERVICE_URL',
  'LLOOM_AI_PORT',
  'LLOOM_API_PORT',
  'LLOOM_HOST',
  'LLOOM_PID_FILE',
  'LLOOM_ENV',
  'LLOOM_LOG_LEVEL',
  'LLOOM_INSTALL_DIR',
  'LOG_LEVEL',
  'RUST_LOG',
]);
const isInternalKey = (k: string): boolean =>
  INTERNAL_ENV_KEYS.has(k) || k.startsWith('LLOOM_AI_');

export default function SettingsPage() {
  const [services, setServices] = useState<ServiceStatus[]>([]);
  const [env, setEnv] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm();
  const [cache, setCache] = useState<CacheStatus | null>(null);
  const cacheTimer = useRef<ReturnType<typeof setInterval> | null>(null);

  const refreshCache = async () => {
    try {
      setCache(await cacheStatus());
    } catch {
      /* AI service may be down */
    }
  };

  // Poll cache status while initialization is running.
  useEffect(() => {
    refreshCache();
    return () => {
      if (cacheTimer.current) clearInterval(cacheTimer.current);
    };
  }, []);
  useEffect(() => {
    if (cache?.status === 'running') {
      if (!cacheTimer.current) {
        // 1s keeps the byte-level progress bar feeling live; a full download is
        // only ~10s on a healthy mirror.
        cacheTimer.current = setInterval(refreshCache, 1000);
      }
    } else if (cacheTimer.current) {
      clearInterval(cacheTimer.current);
      cacheTimer.current = null;
    }
  }, [cache?.status]);

  const handleCacheInit = async () => {
    try {
      await cacheInit();
      message.info('缓存初始化已开始，正在通过镜像源下载 embedding 模型...');
      refreshCache();
    } catch (e) {
      message.error(`启动初始化失败: ${e}`);
    }
  };

  const handleCacheCleanup = async () => {
    try {
      const res = await cacheCleanup();
      message.success(
        res.model_kept
          ? '已清理缓存向量，模型已保留（重新初始化会很快）'
          : '已清理缓存数据，可重新初始化',
      );
      refreshCache();
    } catch (e) {
      message.error(`清理失败: ${e}`);
    }
  };

  // Compact semantic-cache panel. Lives in the narrow left column (span=10)
  // directly beneath 环境检查, so it shares that column's width. Progress
  // reflects the real byte-level download (cache.percent / mirror / speed)
  // instead of a fake elapsed/timeouts bar.
  const renderCacheCard = () => {
    const running = cache?.status === 'running';
    const statusTag = !cache ? (
      <span style={{ color: '#999' }}>…</span>
    ) : cache.ready ? (
      <Tag color="success">已就绪</Tag>
    ) : cache.status === 'running' ? (
      <Tag color="processing">初始化中</Tag>
    ) : cache.status === 'timeout' ? (
      <Tag color="warning">超时</Tag>
    ) : cache.status === 'error' ? (
      <Tag color="error">失败</Tag>
    ) : (
      <Tag>未初始化</Tag>
    );

    return (
      <Card
        size="small"
        title="语义缓存"
        extra={
          <Space size={4}>
            <Button
              size="small"
              type="primary"
              icon={<CloudDownloadOutlined />}
              onClick={handleCacheInit}
              disabled={running || cache?.ready}
            >
              初始化
            </Button>
            <Button size="small" icon={<ReloadOutlined />} onClick={refreshCache}>
              刷新
            </Button>
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              onClick={handleCacheCleanup}
              disabled={running}
            >
              清理
            </Button>
          </Space>
        }
      >
        {!cache ? (
          <span style={{ color: '#999', fontSize: 12 }}>正在获取状态...</span>
        ) : (
          <Space direction="vertical" size={8} style={{ width: '100%' }}>
            <Descriptions column={1} size="small" colon={false}>
              <Descriptions.Item label="状态">{statusTag}</Descriptions.Item>
              <Descriptions.Item label="进度">
                {running && cache.percent != null
                  ? `${cache.percent}% · ${(cache.done_bytes / 1048576).toFixed(1)}MB / ${(
                      cache.total_bytes / 1048576
                    ).toFixed(0)}MB · ${cache.mirror}`
                  : `已用时 ${cache.elapsed}s`}
              </Descriptions.Item>
              {running && cache.file_percent != null && (
                <Descriptions.Item label="本文件">
                  {cache.file || '—'} · {cache.file_percent}%
                </Descriptions.Item>
              )}
              <Descriptions.Item label="说明">
                {cache.detail || cache.error || '语义缓存可在初始化 embedding 模型后启用，加速重复问答。'}
              </Descriptions.Item>
            </Descriptions>

            {running && cache.percent != null && (
              <Progress percent={cache.percent} status="active" size="small" />
            )}
            {running && cache.speed_bps > 0 && (
              <div style={{ color: '#999', fontSize: 12 }}>
                当前文件：{cache.file} · {(cache.speed_bps / 1048576).toFixed(1)} MB/s
              </div>
            )}
            {cache.status === 'timeout' && (
              <Alert
                type="warning"
                showIcon
                message="初始化超时"
                description="下载可能卡住了。点击「清理」删除半成品数据后重试，或保持缓存禁用（不影响对话，仅无加速）。"
              />
            )}
            {cache.status === 'error' && (
              <Alert type="error" showIcon message="初始化失败" description={cache.error || '未知错误'} />
            )}

            <div style={{ color: '#999', fontSize: 12, lineHeight: 1.5 }}>
              首次初始化需下载 all-MiniLM-L6-v2 模型（约 87MB），由内置镜像调度从 hf-mirror.com /
              modelscope.cn 高速拉取并完成 sha256 校验。初始化完成前对话不受影响，仅语义缓存未启用。
            </div>
          </Space>
        )}
      </Card>
    );
  };

  const refresh = async () => {
    try {
      const [s, e] = await Promise.all([getServicesStatus(), readEnv()]);
      setServices(s.services);
      setEnv(e);
      // Prefill form. Secret (password-type) fields are NEVER prefilled with
      // their real value — the API masks them as "****xxxx" anyway, and
      // echoing a real key into a form field is a leak risk. Instead the field
      // is left blank with a placeholder hinting it's already configured.
      const schemaKeys = new Set(ENV_SECTIONS.flatMap((sec) => sec.items.map((i) => i.key)));
      const isSecret = (k: string) => {
        const up = k.toUpperCase();
        return up.endsWith('_API_KEY') || up.endsWith('_KEY') || up.endsWith('_TOKEN') || up.endsWith('_SECRET');
      };
      const values: Record<string, string> = {};
      ENV_SECTIONS.forEach((sec) =>
        sec.items.forEach((item) => {
          if (!isSecret(item.key)) {
            values[item.key] = e[item.key] ?? '';
          }
        }),
      );
      Object.keys(e)
        .filter((k) => !schemaKeys.has(k) && !isSecret(k) && !isInternalKey(k))
        .sort()
        .forEach((k) => {
          values[k] = e[k] ?? '';
        });
      form.setFieldsValue(values);
    } catch (e) {
      message.error(`读取配置失败: ${e}`);
    }
  };

  // Sections for rendering: schema groups + an "其他配置" group with extra keys.
  const allSections = () => {
    const schemaKeys = new Set(ENV_SECTIONS.flatMap((sec) => sec.items.map((i) => i.key)));
    const extra = Object.keys(env)
      .filter((k) => !schemaKeys.has(k) && !isInternalKey(k))
      .sort()
      .map((k) => ({ key: k, label: k, type: 'text' as const, desc: '' }));
    return extra.length ? [...ENV_SECTIONS, { title: '其他配置', items: extra }] : ENV_SECTIONS;
  };

  useEffect(() => {
    refresh();
  }, []);

  const isSecretKey = (k: string) => {
    const up = k.toUpperCase();
    return up.endsWith('_API_KEY') || up.endsWith('_KEY') || up.endsWith('_TOKEN') || up.endsWith('_SECRET');
  };

  // Build the updates map. Secret fields left blank mean "keep existing" and
  // are skipped (the backend also rejects "****" mask values defensively).
  const buildUpdates = (): Record<string, string> => {
    const values = form.getFieldsValue();
    const updates: Record<string, string> = {};
    Object.entries(values).forEach(([k, v]) => {
      const val = String(v ?? '').trim();
      const secret = isSecretKey(k);
      if (isInternalKey(k)) return; // never write operational vars back
      if (secret && val === '') return; // blank secret = keep existing
      if (secret && val.startsWith('****')) return; // mask echoed back = keep
      if (val !== (env[k] ?? '')) {
        updates[k] = val;
      }
    });
    return updates;
  };

  const saveAll = async () => {
    const updates = buildUpdates();
    const changed = Object.keys(updates).length;
    if (changed === 0) {
      message.info('没有需要保存的更改');
      return;
    }
    setSaving(true);
    try {
      await writeEnvBatch(updates);
      setEnv({ ...env, ...updates });
      message.success(`已保存 ${changed} 项配置`);
    } catch (e) {
      message.error(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const smartApply = async () => {
    const updates = buildUpdates();
    const changedKeys = Object.keys(updates);
    if (changedKeys.length === 0) {
      message.info('没有需要应用的更改');
      return;
    }
    await writeEnvBatch(updates);
    setEnv({ ...env, ...updates });
    message.loading('正在重启服务使配置生效...');
    try {
      const res = await smartRestart(changedKeys);
      if (res.ok) message.success(`配置已生效，已重启 ${res.restarted.join(', ')}`);
      else message.error(`重启失败: ${res.errors.join('; ')}`);
    } catch (e) {
      message.error(`智能重启失败: ${e}`);
    }
  };

  // Whether a key is set at all (masked values count as set). Used for the
  // "已配置（输入新值覆盖）" placeholder on secret fields.
  const isSet = (key: string) => Boolean((env[key] ?? '').trim());

  return (
    <Row gutter={16} align="top">
      {/* Left rail: environment check with the semantic-cache panel stacked
          directly beneath it, so both share one column width. */}
      <Col span={10}>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <Card title="环境检查">
            {services.map((s) => (
              <div
                key={s.name}
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  padding: '8px 0',
                  borderBottom: '1px solid #f5f5f5',
                }}
              >
                <span>{s.name}</span>
                {s.healthy ? (
                  <Tag color="success" icon={<CheckOutlined />}>
                    {s.status}
                  </Tag>
                ) : (
                  <Tag color="error" icon={<CloseOutlined />}>
                    {s.status}
                  </Tag>
                )}
              </div>
            ))}
            <Descriptions size="small" column={1} style={{ marginTop: 8 }}>
              <Descriptions.Item label="服务健康">
                {services.filter((s) => s.healthy).length}/{services.length}
              </Descriptions.Item>
            </Descriptions>
          </Card>

          {renderCacheCard()}
        </Space>
      </Col>
      <Col span={14}>
          <Card
            title="API 密钥配置"
            extra={
              <Space>
                <Button icon={<SaveOutlined />} onClick={saveAll} loading={saving}>
                  保存全部
                </Button>
                <Button type="primary" icon={<ThunderboltOutlined />} onClick={smartApply}>
                  智能应用配置
                </Button>
              </Space>
            }
          >
            <Form form={form} layout="vertical">
              {allSections().map((sec) => (
                <div key={sec.title}>
                  <div style={{ fontWeight: 600, color: '#333', margin: '12px 0 8px' }}>{sec.title}</div>
                  {sec.items.map((item) => (
                    <Form.Item key={item.key} name={item.key} label={item.label} style={{ marginBottom: 12 }}>
                      <Input.Password
                        placeholder={isSet(item.key) ? '已配置（输入新值覆盖）' : item.desc}
                        autoComplete="off"
                      />
                    </Form.Item>
                  ))}
                </div>
              ))}
            </Form>
          </Card>
        </Col>
      </Row>
  );
}
