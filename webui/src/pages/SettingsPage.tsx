import { useEffect, useRef, useState, type CSSProperties } from 'react';
import {
  Card,
  Button,
  Space,
  Tag,
  message,
  Descriptions,
  Progress,
  Alert,
  Switch,
  Slider,
  Input,
  Collapse,
  Typography,
} from 'antd';
import {
  CheckOutlined,
  CloseOutlined,
  CloudDownloadOutlined,
  CopyOutlined,
  DeleteOutlined,
  ReloadOutlined,
  ApiOutlined,
} from '@ant-design/icons';
import {
  getServicesStatus,
  cacheInit,
  cacheStatus,
  cacheCleanup,
  cacheThresholdGet,
  cacheThresholdSet,
  getProxyConfig,
  setProxyToken,
  proxySelftest,
  ProxyConfig,
  ServiceStatus,
  CacheStatus,
  CacheThresholdInfo,
} from '../api';

const { Text } = Typography;

export default function SettingsPage() {
  const [services, setServices] = useState<ServiceStatus[]>([]);
  const [cache, setCache] = useState<CacheStatus | null>(null);
  const [proxy, setProxy] = useState<ProxyConfig | null>(null);
  const [tokenInput, setTokenInput] = useState('');
  const [proxyBusy, setProxyBusy] = useState(false);
  const [selftest, setSelftest] = useState<{
    ok: boolean;
    detail: string;
    models?: number;
    latency_ms?: number;
  } | null>(null);
  const [thr, setThr] = useState<CacheThresholdInfo | null>(null);
  const [thrBusy, setThrBusy] = useState(false);
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

  // Semantic-cache similarity threshold: read current value + auto-tune state,
  // and let the user toggle auto-tune or pin a manual value (disables auto).
  const refreshThr = async () => {
    try {
      setThr(await cacheThresholdGet());
    } catch {
      /* AI service may be down */
    }
  };
  useEffect(() => {
    refreshThr();
  }, []);
  const handleAutoToggle = async (on: boolean) => {
    setThrBusy(true);
    try {
      const r = await cacheThresholdSet({ autoTune: on });
      setThr((t) => (t ? { ...t, auto_tune: r.auto_tune, threshold: r.threshold } : t));
    } catch {
      /* ignore */
    }
    setThrBusy(false);
  };
  const handleThrChange = async (v: number) => {
    setThrBusy(true);
    try {
      const r = await cacheThresholdSet({ threshold: v, autoTune: false });
      setThr((t) => (t ? { ...t, threshold: r.threshold, auto_tune: false } : t));
    } catch {
      /* ignore */
    }
    setThrBusy(false);
  };

  // Semantic-cache panel. Progress reflects the real byte-level download
  // (cache.percent / mirror / speed) instead of a fake elapsed/timeouts bar.
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

            {thr && (
              <div style={{ borderTop: '1px solid #f0f0f0', paddingTop: 8 }}>
                <div
                  style={{
                    fontSize: 12,
                    color: '#666',
                    marginBottom: 4,
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                  }}
                >
                  <span>语义缓存阈值（相似度）</span>
                  <Space size={4}>
                    <span style={{ fontSize: 12 }}>自动调优</span>
                    <Switch size="small" checked={thr.auto_tune} loading={thrBusy} onChange={handleAutoToggle} />
                  </Space>
                </div>
                <Slider
                  min={0.7}
                  max={0.92}
                  step={0.01}
                  value={thr.threshold}
                  disabled={thr.auto_tune}
                  onChange={(v) => handleThrChange(v as number)}
                  tooltip={{ formatter: (v?: number) => `${(v ?? 0).toFixed(2)}` }}
                />
                <div style={{ fontSize: 12, color: '#999' }}>
                  {thr.suggested
                    ? `系统建议 ${Number(thr.suggested).toFixed(2)}（基于 ${thr.labeled_samples} 次反馈）`
                    : `已收集 ${thr.labeled_samples} 次反馈，自动调优${thr.auto_tune ? '中' : '已关闭'}`}
                </div>
              </div>
            )}
          </Space>
        )}
      </Card>
    );
  };

  const refresh = async () => {
    try {
      const s = await getServicesStatus();
      setServices(s.services);
    } catch (e) {
      message.error(`读取服务状态失败: ${e}`);
    }
  };

  // ── OpenAI 兼容代理接入（N1 向导） ──
  const refreshProxy = async () => {
    try {
      setProxy(await getProxyConfig());
    } catch {
      /* server down */
    }
  };
  useEffect(() => {
    refreshProxy();
  }, []);

  const handleSaveToken = async () => {
    const v = tokenInput.trim();
    if (!v) {
      message.warning('请输入 Key，或点「清除」恢复不鉴权');
      return;
    }
    setProxyBusy(true);
    try {
      setProxy(await setProxyToken(v));
      setTokenInput('');
      message.success('API Key 已保存，立即生效（无需重启）');
    } catch (e) {
      message.error(`保存失败: ${e}`);
    }
    setProxyBusy(false);
  };

  const handleClearToken = async () => {
    setProxyBusy(true);
    try {
      setProxy(await setProxyToken(null));
      setTokenInput('');
      message.success('已清除本页配置的 Key，立即生效');
    } catch (e) {
      message.error(`清除失败: ${e}`);
    }
    setProxyBusy(false);
  };

  const handleSelftest = async () => {
    setProxyBusy(true);
    setSelftest(null);
    try {
      setSelftest(await proxySelftest());
    } catch (e) {
      setSelftest({ ok: false, detail: String(e) });
    }
    setProxyBusy(false);
  };

  // 代理接入卡片：向导式三步（拿地址 → 设 Key → 自测），附 model 字段注释与接入示例。
  const renderProxyCard = () => {
    const lan = !!proxy && proxy.bind !== '127.0.0.1' && proxy.bind !== 'localhost';
    const preStyle: CSSProperties = {
      background: '#fafafa',
      border: '1px solid #f0f0f0',
      borderRadius: 6,
      padding: '8px 10px',
      fontSize: 12,
      lineHeight: 1.6,
      overflowX: 'auto',
      margin: '4px 0 0',
    };
    const curlExample = `curl ${proxy?.base_url ?? 'http://127.0.0.1:7861/v1'}/chat/completions \\
  -H "Authorization: Bearer <你的Key>" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"auto","messages":[{"role":"user","content":"你好"}]}'`;
    const pyExample = `from openai import OpenAI

client = OpenAI(
    base_url="${proxy?.base_url ?? 'http://127.0.0.1:7861/v1'}",
    api_key="<你的Key>",
)
resp = client.chat.completions.create(
    model="auto",
    messages=[{"role": "user", "content": "你好"}],
)
print(resp.choices[0].message.content)`;

    return (
      <Card
        size="small"
        title={
          <Space size={6}>
            <ApiOutlined />
            <span>OpenAI 兼容代理</span>
          </Space>
        }
        extra={
          <Space size={4}>
            <Button size="small" icon={<ApiOutlined />} onClick={handleSelftest} loading={proxyBusy}>
              连通性自测
            </Button>
            <Button size="small" icon={<ReloadOutlined />} onClick={refreshProxy} />
          </Space>
        }
      >
        {!proxy ? (
          <span style={{ color: '#999', fontSize: 12 }}>正在获取配置...</span>
        ) : (
          <Space direction="vertical" size={10} style={{ width: '100%' }}>
            <Descriptions column={1} size="small" colon={false}>
              <Descriptions.Item label="接入地址">
                <Text code copyable={{ text: proxy.base_url, tooltips: ['复制 Base URL', '已复制'] }}>
                  {proxy.base_url}
                </Text>
              </Descriptions.Item>
              <Descriptions.Item label="鉴权状态">
                <Space size={6}>
                  {proxy.auth_enabled ? (
                    <Tag color="success" icon={<CheckOutlined />}>
                      已启用
                    </Tag>
                  ) : (
                    <Tag color="warning">未鉴权</Tag>
                  )}
                  {proxy.token_source === 'ui' && <Tag>Key 来自本页配置</Tag>}
                  {proxy.token_source === 'env' && <Tag>Key 来自环境变量</Tag>}
                </Space>
              </Descriptions.Item>
              {proxy.auth_enabled && (
                <Descriptions.Item label="API Key">
                  <Text code>{proxy.token_masked}</Text>
                </Descriptions.Item>
              )}
            </Descriptions>

            {lan && (
              <Alert
                type="info"
                showIcon
                message={`当前绑定 ${proxy.bind}`}
                description="局域网设备接入时，请把地址中的 127.0.0.1 替换为本机局域网 IP。"
              />
            )}
            {!proxy.auth_enabled && (
              <Alert
                type="warning"
                showIcon
                message="未设置 API Key：任何能访问该端口的程序都可直接调用"
                description="仅本机使用可忽略（服务默认只绑 127.0.0.1）；需要局域网/远程接入时请在下方设置 Key。"
              />
            )}
            {proxy.token_source === 'env' && (
              <div style={{ fontSize: 12, color: '#999' }}>
                Key 由环境变量 LLOOM_PROXY_TOKEN 提供（部署级配置）。在本页另设 Key 可覆盖它；清除后回落到环境变量。
              </div>
            )}

            {proxy.token_source !== 'env' && (
              <Space.Compact style={{ width: '100%' }}>
                <Input.Password
                  placeholder="设置 API Key（作为 Bearer token 下发给客户端）"
                  value={tokenInput}
                  onChange={(e) => setTokenInput(e.target.value)}
                  onPressEnter={handleSaveToken}
                  disabled={proxyBusy}
                  autoComplete="new-password"
                />
                <Button type="primary" onClick={handleSaveToken} loading={proxyBusy}>
                  保存
                </Button>
                {proxy.token_source === 'ui' && (
                  <Button danger onClick={handleClearToken} disabled={proxyBusy}>
                    清除
                  </Button>
                )}
              </Space.Compact>
            )}
            <div style={{ fontSize: 12, color: '#999' }}>
              Key 保存在本地数据库（与模型 API Key 同一约定），保存后立即生效，无需重启。
            </div>

            {selftest && (
              <Alert
                type={selftest.ok ? 'success' : 'error'}
                showIcon
                message={
                  selftest.ok
                    ? `自测通过 · ${selftest.models ?? 0} 个模型 · ${selftest.latency_ms ?? 0}ms`
                    : '自测失败'
                }
                description={selftest.detail}
              />
            )}

            <Collapse
              ghost
              size="small"
              items={[
                {
                  key: 'model',
                  label: 'model 字段怎么填？',
                  children: (
                    <div style={{ fontSize: 12, lineHeight: 1.8, color: '#555' }}>
                      <div>
                        <Text code>auto</Text>（推荐）——智能路由：LLooM 按任务类型、成本、质量与预算档自动选模型。
                      </div>
                      <div>
                        <Text code>注册模型名</Text>——直连该模型、跳过路由；可用名称见「模型管理」页或{' '}
                        <Text code>GET /v1/models</Text>（auto 恒在首位）。
                      </div>
                      <div>未知名也自动回落到 auto 路由，不会报错。</div>
                      <div style={{ color: '#999' }}>
                        当前限制：流式返回为整段下发（非逐字）；不支持 tools / 多模态；任务分解编排不在本通道。
                      </div>
                    </div>
                  ),
                },
                {
                  key: 'examples',
                  label: '接入示例（curl / Python）',
                  children: (
                    <div>
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                          fontSize: 12,
                          color: '#666',
                        }}
                      >
                        <span>curl</span>
                        <Text copyable={{ text: curlExample, tooltips: ['复制', '已复制'] }}>
                          <CopyOutlined />
                        </Text>
                      </div>
                      <pre style={preStyle}>{curlExample}</pre>
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                          fontSize: 12,
                          color: '#666',
                          marginTop: 8,
                        }}
                      >
                        <span>Python（openai SDK）</span>
                        <Text copyable={{ text: pyExample, tooltips: ['复制', '已复制'] }}>
                          <CopyOutlined />
                        </Text>
                      </div>
                      <pre style={preStyle}>{pyExample}</pre>
                    </div>
                  ),
                },
              ]}
            />
          </Space>
        )}
      </Card>
    );
  };

  useEffect(() => {
    refresh();
  }, []);

  return (
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

      {renderProxyCard()}
      {renderCacheCard()}
    </Space>
  );
}
