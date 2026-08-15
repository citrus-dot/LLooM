import { useEffect, useState } from 'react';
import { Card, Row, Col, Button, Space, Tag, Form, Input, message, Descriptions } from 'antd';
import { CheckOutlined, CloseOutlined, SaveOutlined, ThunderboltOutlined } from '@ant-design/icons';
import {
  getServicesStatus,
  readEnv,
  writeEnvBatch,
  smartRestart,
  ServiceStatus,
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

export default function SettingsPage() {
  const [services, setServices] = useState<ServiceStatus[]>([]);
  const [env, setEnv] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm();

  const refresh = async () => {
    try {
      const [s, e] = await Promise.all([getServicesStatus(), readEnv()]);
      setServices(s.services);
      setEnv(e);
      // Prefill form
      const values: Record<string, string> = {};
      ENV_SECTIONS.forEach((sec) =>
        sec.items.forEach((item) => {
          values[item.key] = e[item.key] ?? '';
        }),
      );
      // Include any extra keys the server exposes (nothing hidden).
      const schemaKeys = new Set(ENV_SECTIONS.flatMap((sec) => sec.items.map((i) => i.key)));
      Object.keys(e)
        .filter((k) => !schemaKeys.has(k))
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
      .filter((k) => !schemaKeys.has(k))
      .sort()
      .map((k) => ({ key: k, label: k, type: 'text' as const, desc: '' }));
    return extra.length ? [...ENV_SECTIONS, { title: '其他配置', items: extra }] : ENV_SECTIONS;
  };

  useEffect(() => {
    refresh();
  }, []);

  const saveAll = async () => {
    const values = form.getFieldsValue();
    const updates: Record<string, string> = {};
    let changed = 0;
    Object.entries(values).forEach(([k, v]) => {
      const val = String(v ?? '').trim();
      if (val !== (env[k] ?? '')) {
        updates[k] = val;
        changed++;
      }
    });
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
    const values = form.getFieldsValue();
    const changedKeys: string[] = [];
    Object.entries(values).forEach(([k, v]) => {
      if (String(v ?? '').trim() !== (env[k] ?? '')) changedKeys.push(k);
    });
    if (changedKeys.length === 0) {
      message.info('没有需要应用的更改');
      return;
    }
    await saveAll();
    message.loading('正在重启服务使配置生效...');
    try {
      const res = await smartRestart(changedKeys);
      if (res.ok) message.success(`配置已生效，已重启 ${res.restarted.join(', ')}`);
      else message.error(`重启失败: ${res.errors.join('; ')}`);
    } catch (e) {
      message.error(`智能重启失败: ${e}`);
    }
  };

  const hasValue = (key: string) => Boolean((env[key] ?? '').trim());

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Row gutter={16}>
        <Col span={10}>
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
                        placeholder={hasValue(item.key) ? '已配置（输入新值覆盖）' : item.desc}
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
    </Space>
  );
}
