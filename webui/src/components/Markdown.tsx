import React, { useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github.css';

/**
 * ChatGPT / Claude 风格的 Markdown 渲染器。
 * - 支持 GFM（表格、删除线、任务列表、自动链接）
 * - 代码块带语言标签 + 一键复制，使用 highlight.js 高亮
 * - 链接强制新窗口打开
 * 不再把 # * 等符号以纯文本裸显。
 */

function CodeBlock({ className, children }: { className?: string; children?: React.ReactNode }) {
  const ref = useRef<HTMLElement>(null);
  const [copied, setCopied] = useState(false);
  const match = /language-(\w+)/.exec(className || '');
  const copy = () => {
    const text = ref.current?.textContent ?? '';
    navigator.clipboard?.writeText(text).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      },
      () => {},
    );
  };
  return (
    <div className="md-codeblock">
      <div className="md-codeblock-bar">
        <span className="md-codeblock-lang">{match ? match[1] : 'code'}</span>
        <button type="button" className="md-copy-btn" onClick={copy}>
          {copied ? '已复制' : '复制'}
        </button>
      </div>
      <pre>
        <code ref={ref} className={className}>
          {children}
        </code>
      </pre>
    </div>
  );
}

const components = {
  // 链接新窗口打开
  a: (props: any) => <a {...props} target="_blank" rel="noreferrer" />,
  // 用我们自己的 CodeBlock 接管代码块，pre 透传避免嵌套
  pre: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
  code: ({ className, children, ...props }: any) => {
    const match = /language-(\w+)/.exec(className || '');
    const raw = String(children ?? '');
    const isBlock = !!match || raw.includes('\n');
    if (!isBlock) {
      return (
        <code className="md-inline-code" {...props}>
          {children}
        </code>
      );
    }
    return (
      <CodeBlock className={className}>{children}</CodeBlock>
    );
  },
};

export default function Markdown({ content }: { content: string }) {
  return (
    <div className="md-body">
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]} components={components}>
        {content}
      </ReactMarkdown>
    </div>
  );
}
