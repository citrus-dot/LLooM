import { useEffect, useState } from 'react';
import { Alert, Button, Card, Descriptions, Form, InputNumber, Space, Switch, Tag, Typography, message } from 'antd';
import { DatabaseOutlined, ReloadOutlined, SafetyCertificateOutlined } from '@ant-design/icons';
import { getProxyConfig, getRuntimeSettings, getServicesStatus, saveRuntimeSettings } from '../api';
import type { ProxyConfig, RuntimeSettings, ServiceStatus } from '../api';

const { Text } = Typography;

export default function SettingsPage() {
  const [services, setServices] = useState<ServiceStatus[]>([]);
  const [proxy, setProxy] = useState<ProxyConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm<RuntimeSettings>();

  const refresh = async () => {
    try {
      const [status, proxyConfig, settings] = await Promise.all([
        getServicesStatus(), getProxyConfig(), getRuntimeSettings(),
      ]);
      setServices(status.services);
      setProxy(proxyConfig);
      form.setFieldsValue(settings);
    } catch (e) {
      message.error(`读取设置失败: ${e}`);
    }
  };

  useEffect(() => { void refresh(); }, []);

  const save = async (values: RuntimeSettings) => {
    setSaving(true);
    try {
      form.setFieldsValue(await saveRuntimeSettings(values));
      message.success('运行参数已保存并生效');
    } catch (e) {
      message.error(`保存设置失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div className="page-hero">
        <div><h2>系统设置</h2><p>调整运行参数，查看网络入口和服务状态。</p></div>
        <Button icon={<ReloadOutlined />} onClick={refresh}>刷新</Button>
      </div>
      <Card title="运行参数（保存后生效）" className="clean-card">
        <Form form={form} layout="vertical" onFinish={save} style={{ maxWidth: 400 }}>
          <Form.Item
            name="health_probe_sec"
            label="故障模型健康探测间隔（秒）"
            extra="仅对降级或不可用的模型自动探测；云端探测可能产生少量费用。"
            rules={[{ required: true, type: 'number', min: 5, max: 86400, message: '请输入 5～86400 秒' }]}
          >
            <InputNumber min={5} max={86400} precision={0} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item
            name="shadow_ratio"
            label="路由影子采样比例（0～1）"
            extra="影子评测会额外调用基线模型；设为 0 可关闭并避免该项额外费用。"
            rules={[{ required: true, type: 'number', min: 0, max: 1, message: '请输入 0～1' }]}
          >
            <InputNumber min={0} max={1} step={0.01} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="block_pii" label="个人信息处理" valuePropName="checked"
            extra="关闭时先掩码再发送给模型；开启后检测到有效证件或卡号即拒绝请求。">
            <Switch checkedChildren="拒绝" unCheckedChildren="掩码" />
          </Form.Item>
          <Form.Item name="block_jailbreak" label="拦截提示词注入" valuePropName="checked"
            extra="关闭后不再自动拒绝疑似越狱提示词。">
            <Switch />
          </Form.Item>
          <Button type="primary" htmlType="submit" loading={saving}>保存运行参数</Button>
        </Form>
      </Card>
      <div className="settings-grid">
        <Card title={<Space><SafetyCertificateOutlined />运行状态</Space>} className="clean-card">
          <Space direction="vertical" style={{ width: '100%' }}>
            {services.map((service) => (
              <div className="status-row" key={service.name}>
                <div><b>{service.name}</b>{service.detail && <div className="muted">{service.detail}</div>}</div>
                <Tag color={service.healthy ? 'success' : 'error'}>{service.status}</Tag>
              </div>
            ))}
          </Space>
        </Card>
        <Card title={<Space><DatabaseOutlined />网络与存储（需修改启动配置并重启）</Space>} className="clean-card">
          <Descriptions column={1} size="small">
            <Descriptions.Item label="OpenAI Base URL"><Text code copyable>{proxy?.base_url ?? '加载中…'}</Text></Descriptions.Item>
            <Descriptions.Item label="监听地址"><Text code>{proxy?.bind ?? '-'}</Text></Descriptions.Item>
            <Descriptions.Item label="访问鉴权">{proxy?.auth_enabled ? <Tag color="success">已启用 · {proxy.key_count} 个 Key</Tag> : <Tag color="warning">未创建 Key</Tag>}</Descriptions.Item>
            <Descriptions.Item label="缓存"><Tag color="blue">精确缓存 + FastEmbed 语义缓存</Tag></Descriptions.Item>
            <Descriptions.Item label="数据目录"><Text code>./data</Text></Descriptions.Item>
          </Descriptions>
        </Card>
      </div>
      {!proxy?.auth_enabled && <Alert type="warning" showIcon message="外部 API 当前未配置 Key" description="请在 API Keys 页面创建 Key；未配置 Key 时远程 /v1/* 请求会被拒绝。" />}
    </Space>
  );
}
