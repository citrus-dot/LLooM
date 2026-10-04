import { useEffect, useState } from 'react';
import { Alert, Button, Card, Descriptions, Space, Tag, Typography, message } from 'antd';
import { DatabaseOutlined, ReloadOutlined, SafetyCertificateOutlined } from '@ant-design/icons';
import { getProxyConfig, getServicesStatus, ProxyConfig, ServiceStatus } from '../api';

const { Text } = Typography;

export default function SettingsPage() {
  const [services,setServices]=useState<ServiceStatus[]>([]); const [proxy,setProxy]=useState<ProxyConfig|null>(null);
  const refresh=async()=>{ try{const [s,p]=await Promise.all([getServicesStatus(),getProxyConfig()]);setServices(s.services);setProxy(p);}catch(e){message.error(`读取设置失败: ${e}`);} };
  useEffect(()=>{void refresh();},[]);
  return <Space direction="vertical" size={16} style={{width:'100%'}}>
    <div className="page-hero"><div><h2>系统设置</h2><p>查看运行状态、网络入口和本地存储。访问凭证请前往 API Keys。</p></div><Button icon={<ReloadOutlined/>} onClick={refresh}>刷新</Button></div>
    <div className="settings-grid">
      <Card title={<Space><SafetyCertificateOutlined/>运行状态</Space>} className="clean-card">
        <Space direction="vertical" style={{width:'100%'}}>{services.map(s=><div className="status-row" key={s.name}><div><b>{s.name}</b>{s.detail&&<div className="muted">{s.detail}</div>}</div><Tag color={s.healthy?'success':'error'}>{s.status}</Tag></div>)}</Space>
      </Card>
      <Card title={<Space><DatabaseOutlined/>网络与存储</Space>} className="clean-card">
        <Descriptions column={1} size="small">
          <Descriptions.Item label="OpenAI Base URL"><Text code copyable>{proxy?.base_url??'加载中…'}</Text></Descriptions.Item>
          <Descriptions.Item label="监听地址"><Text code>{proxy?.bind??'-'}</Text></Descriptions.Item>
          <Descriptions.Item label="访问鉴权">{proxy?.auth_enabled?<Tag color="success">已启用 · {proxy.key_count} 个 Key</Tag>:<Tag color="warning">未创建 Key</Tag>}</Descriptions.Item>
          <Descriptions.Item label="缓存"><Tag color="blue">精确缓存 + FastEmbed 语义缓存</Tag></Descriptions.Item>
          <Descriptions.Item label="数据目录"><Text code>./data</Text></Descriptions.Item>
        </Descriptions>
      </Card>
    </div>
    {!proxy?.auth_enabled&&<Alert type="warning" showIcon message="外部 API 当前未鉴权" description="请在 API Keys 页面创建至少一个 Key。创建后 /v1/* 将要求 Bearer 鉴权。"/>}
  </Space>;
}
