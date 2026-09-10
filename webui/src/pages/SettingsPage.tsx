import { useEffect, useRef, useState } from 'react';
import { Card, Button, Space, Tag, message, Descriptions, Progress, Alert, Switch, Slider } from 'antd';
import {
  CheckOutlined,
  CloseOutlined,
  CloudDownloadOutlined,
  DeleteOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import {
  getServicesStatus,
  cacheInit,
  cacheStatus,
  cacheCleanup,
  cacheThresholdGet,
  cacheThresholdSet,
  ServiceStatus,
  CacheStatus,
  CacheThresholdInfo,
} from '../api';

export default function SettingsPage() {
  const [services, setServices] = useState<ServiceStatus[]>([]);
  const [cache, setCache] = useState<CacheStatus | null>(null);
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

      {renderCacheCard()}
    </Space>
  );
}
