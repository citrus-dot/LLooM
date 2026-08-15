import { useState } from 'react';
import { Layout, Menu, Typography } from 'antd';
import {
  DashboardOutlined,
  LineChartOutlined,
  MessageOutlined,
  RobotOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import OverviewPage from './pages/OverviewPage';
import UsagePage from './pages/UsagePage';
import ChatPage from './pages/ChatPage';
import ModelsPage from './pages/ModelsPage';
import SettingsPage from './pages/SettingsPage';

const { Sider, Content } = Layout;

type PageKey = 'overview' | 'usage' | 'chat' | 'models' | 'settings';

const NAV_ITEMS = [
  { key: 'overview', icon: <DashboardOutlined />, label: '总览' },
  { key: 'usage', icon: <LineChartOutlined />, label: '用量' },
  { key: 'chat', icon: <MessageOutlined />, label: '对话' },
  { key: 'models', icon: <RobotOutlined />, label: '模型' },
  { key: 'settings', icon: <SettingOutlined />, label: '设置' },
];

export default function App() {
  const [page, setPage] = useState<PageKey>('overview');

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Sider width={200} theme="light" style={{ borderRight: '1px solid #f0f0f0' }}>
        <div style={{ padding: '16px 24px', fontSize: 18, fontWeight: 700 }}>
          LLooM
        </div>
        <Menu
          mode="inline"
          selectedKeys={[page]}
          items={NAV_ITEMS}
          onClick={(e) => setPage(e.key as PageKey)}
        />
      </Sider>
      <Layout>
        <Content style={{ padding: 24, overflow: 'auto' }}>
          <Typography.Title level={3} style={{ marginTop: 0, marginBottom: 16 }}>
            {NAV_ITEMS.find((i) => i.key === page)?.label}
          </Typography.Title>
          {page === 'overview' && <OverviewPage />}
          {page === 'usage' && <UsagePage />}
          {page === 'chat' && <ChatPage />}
          {page === 'models' && <ModelsPage />}
          {page === 'settings' && <SettingsPage />}
        </Content>
      </Layout>
    </Layout>
  );
}
