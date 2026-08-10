const { Document, Packer, Paragraph, TextRun, Table, TableRow, TableCell,
        Header, Footer, AlignmentType, LevelFormat,
        HeadingLevel, BorderStyle, WidthType, ShadingType,
        PageNumber, PageBreak } = require('docx');
const fs = require('fs');
const path = require('path');

const cjkFont = { ascii: "Arial", hAnsi: "Arial", eastAsia: "Microsoft YaHei" };
const border = { style: BorderStyle.SINGLE, size: 1, color: "CCCCCC" };
const borders = { top: border, bottom: border, left: border, right: border };

function cell(text, width, opts = {}) {
  return new TableCell({
    borders,
    width: { size: String(width || 3120), type: WidthType.DXA },
    shading: opts.shading ? { fill: opts.shading, type: ShadingType.CLEAR } : undefined,
    margins: { top: 80, bottom: 80, left: 120, right: 120 },
    children: [new Paragraph({ children: [new TextRun({ text, bold: opts.bold, size: opts.size || 22, font: cjkFont })] })]
  });
}

function row(cells, cantSplit = true) {
  return new TableRow({ cantSplit, children: cells });
}

function heading(text, level) {
  return new Paragraph({
    heading: level === 1 ? HeadingLevel.HEADING_1 : HeadingLevel.HEADING_2,
    children: [new TextRun({ text, bold: true, font: cjkFont })]
  });
}

function para(text, opts = {}) {
  return new Paragraph({
    children: [new TextRun({ text, size: opts.size || 22, bold: opts.bold, font: cjkFont, color: opts.color })],
    spacing: { after: opts.after || 120 }
  });
}

function bullet(text) {
  return new Paragraph({
    numbering: { reference: "bullets", level: 0 },
    children: [new TextRun({ text, size: 22, font: cjkFont })]
  });
}

const doc = new Document({
  styles: {
    default: { document: { run: { font: cjkFont, size: 22 } } },
    paragraphStyles: [
      { id: "Heading1", name: "Heading 1", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 32, bold: true, font: cjkFont },
        paragraph: { spacing: { before: 360, after: 200 }, outlineLevel: 0, keepNext: false, keepLines: false } },
      { id: "Heading2", name: "Heading 2", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 28, bold: true, font: cjkFont },
        paragraph: { spacing: { before: 240, after: 160 }, outlineLevel: 1, keepNext: false, keepLines: false } },
    ]
  },
  numbering: {
    config: [
      { reference: "bullets", levels: [{ level: 0, format: LevelFormat.BULLET, text: "\u2022", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 720, hanging: 360 } } } }] },
      { reference: "numbers", levels: [{ level: 0, format: LevelFormat.DECIMAL, text: "%1.", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 720, hanging: 360 } } } }] },
    ]
  },
  sections: [{
    properties: {
      page: {
        size: { width: 12240, height: 15840 },
        margin: { top: 1440, right: 1440, bottom: 1440, left: 1440 }
      }
    },
    headers: {
      default: new Header({ children: [new Paragraph({ alignment: AlignmentType.RIGHT, children: [new TextRun({ text: "LiteLLM \u667a\u80fd\u8def\u7531\u4ee3\u7406 \u2014 \u5de5\u4f5c\u6c47\u62a5", size: 18, color: "999999", font: cjkFont })] })] })
    },
    footers: {
      default: new Footer({ children: [new Paragraph({ alignment: AlignmentType.CENTER, children: [new TextRun({ text: "\u7b2c ", size: 18, font: cjkFont }), new TextRun({ children: [PageNumber.CURRENT], size: 18, font: cjkFont }), new TextRun({ text: " \u9875", size: 18, font: cjkFont })] })] })
    },
    children: [
      // === 封面 ===
      new Paragraph({ spacing: { before: 2400 }, children: [] }),
      new Paragraph({ alignment: AlignmentType.CENTER, children: [new TextRun({ text: "LiteLLM \u667a\u80fd\u8def\u7531\u4ee3\u7406\u5e73\u53f0", size: 44, bold: true, font: cjkFont })] }),
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 200 }, children: [new TextRun({ text: "\u5de5\u4f5c\u6c47\u62a5", size: 36, font: cjkFont, color: "666666" })] }),
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 600 }, children: [new TextRun({ text: "\u7b2c\u4e09\u9636\u6bb5 \u2014 \u7b2c\u56db\u9636\u6bb5\uff08\u542b Docker \u90e8\u7f72\u5305\u4e0e Tauri \u684c\u9762\u5e94\u7528\uff09", size: 26, font: cjkFont, color: "999999" })] }),
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 1200 }, children: [new TextRun({ text: "2026-08-02", size: 24, font: cjkFont, color: "999999" })] }),
      new Paragraph({ children: [new PageBreak()] }),

      // === 目录 ===
      heading("\u76ee\u5f55", 1),
      para("1. \u9879\u76ee\u6982\u8ff0"),
      para("2. \u7b2c\u4e09\u9636\u6bb5\uff1aCLI \u5de5\u5177\u4e0e\u524d\u7aef UI \u5c01\u88c5"),
      para("3. \u7b2c\u56db\u9636\u6bb5\uff1a\u590d\u6742\u4efb\u52a1\u7f16\u6392\u4e0e\u53ef\u89c6\u5316\u7ba1\u7406"),
      para("4. Docker \u90e8\u7f72\u5305"),
      para("5. Tauri \u684c\u9762\u5e94\u7528"),
      para("6. \u529f\u80fd\u76ee\u6807\u603b\u7ed3"),
      para("7. \u7cfb\u7edf\u67b6\u6784"),
      para("8. \u9a8c\u8bc1\u7ed3\u679c"),
      para("9. \u540e\u7eed\u89c4\u5212"),
      new Paragraph({ children: [new PageBreak()] }),

      // === 1. 项目概述 ===
      heading("1. \u9879\u76ee\u6982\u8ff0", 1),
      para("\u672c\u9879\u76ee\u662f\u57fa\u4e8e LiteLLM \u7684\u591a\u6a21\u578b\u667a\u80fd\u8def\u7531\u4ee3\u7406\u5e73\u53f0\uff0c\u65e8\u5728\u4e3a\u5ba2\u6237\u63d0\u4f9b\u4e00\u5957\u53ef\u4e00\u952e\u90e8\u7f72\u3001\u667a\u80fd\u8def\u7531\u3001\u6210\u672c\u4f18\u5316\u7684\u7edf\u4e00\u7ba1\u7406\u5de5\u5177\u3002\u9879\u76ee\u91c7\u7528\u63a7\u5236\u9762\uff08Admin\uff09\u4e0e\u6570\u636e\u9762\uff08Worker\uff09\u5206\u79bb\u67b6\u6784\uff0c\u96c6\u6210\u591a\u5bb6\u4e91\u7aef\u6a21\u578b\uff08\u963f\u91cc\u4e91\u767e\u7387\u3001OpenAI\u3001Anthropic\uff09\u4e0e\u672c\u5730\u6a21\u578b\uff08Ollama\uff09\uff0c\u5b9e\u73b0\u4e86\u667a\u80fd\u4efb\u52a1\u5206\u7ea7\u3001\u590d\u6742\u4efb\u52a1\u81ea\u52a8\u62c6\u89e3\u3001\u6210\u672c\u89c4\u5212\u591a\u6a21\u578b\u8c03\u7528\u3001\u53ef\u89c6\u5316\u7ba1\u7406\u754c\u9762\u7b49\u6838\u5fc3\u80fd\u529b\u3002"),
      para("\u622a\u81f3\u76ee\u524d\uff0c\u9879\u76ee\u5df2\u5b8c\u6210\u7b2c\u4e09\u9636\u6bb5\uff08CLI + \u524d\u7aef UI \u5c01\u88c5\uff09\u548c\u7b2c\u56db\u9636\u6bb5\uff08\u590d\u6742\u4efb\u52a1\u7f16\u6392 + \u53ef\u89c6\u5316\u7ba1\u7406\uff09\uff0c\u5e76\u6b63\u5728\u5b9e\u73b0 Docker \u90e8\u7f72\u5305\u548c Tauri \u684c\u9762\u5e94\u7528\u3002", { after: 240 }),

      // === 2. 第三阶段 ===
      heading("2. \u7b2c\u4e09\u9636\u6bb5\uff1aCLI \u5de5\u5177\u4e0e\u524d\u7aef UI \u5c01\u88c5", 1),

      heading("2.1 \u4ea4\u4ed8\u7269", 2),
      new Table({
        width: { size: 100, type: WidthType.PERCENTAGE },
        columnWidths: [3120, 6240],
        rows: [
          row([cell("\u6587\u4ef6", 3120, { bold: true, shading: "D5E8F0" }), cell("\u4f5c\u7528", 6240, { bold: true, shading: "D5E8F0" })]),
          row([cell("litellm_cli.py", 3120), cell("Python CLI \u5de5\u5177\uff08init/add-model/list-models/remove-model/health/status/logs/orchestrate\uff09\uff0c\u96f6\u5916\u90e8\u4f9d\u8d56", 6240)]),
          row([cell("install.sh", 3120), cell("\u4e00\u952e\u5b89\u88c5\u811a\u672c\uff08\u68c0\u67e5 Docker/Python3\uff0c\u542f\u52a8 CLI \u914d\u7f6e\u5411\u5bfc\uff09", 6240)]),
          row([cell("docker-compose.yml", 3120), cell("Docker \u7f16\u6392\uff088 \u4e2a\u670d\u52a1\uff09\uff0c\u542b Open WebUI \u804a\u5929\u754c\u9762", 6240)]),
          row([cell(".env.example", 3120), cell("\u73af\u5883\u53d8\u91cf\u6a21\u677f\uff08Redis \u5bc6\u7801\u3001Ollama\u3001Qdrant\u3001Grafana\uff09", 6240)]),
          row([cell("README.md", 3120), cell("\u8f6f\u4ef6\u6587\u6863\uff08\u67b6\u6784\u3001\u547d\u4ee4\u3001\u7aef\u53e3\u6620\u5c04\uff09", 6240)]),
        ]
      }),

      heading("2.2 \u6838\u5fc3\u529f\u80fd", 2),
      bullet("init \u547d\u4ee4\uff1a\u81ea\u52a8\u68c0\u6d4b Docker/Ollama\u3001\u6536\u96c6 API Key\u3001\u9002\u914d\u8def\u7531\u7b56\u7565\u3001\u751f\u6210 TASK_MODEL_MAP\u3001\u542f\u52a8\u670d\u52a1"),
      bullet("add-model \u547d\u4ee4\uff1a\u4ea4\u4e92\u5f0f\u6dfb\u52a0\u6a21\u578b\uff0c\u81ea\u52a8\u66f4\u65b0 config_worker.yaml / custom_callbacks.py / quota_setup.sh / fallbacks \u94fe"),
      bullet("status \u547d\u4ee4\uff1a\u67e5\u770b\u5bb9\u5668\u8d44\u6e90\u3001\u8def\u7531\u7edf\u8ba1\u3001\u914d\u989d\u8ffd\u8e2a\u3001\u6a21\u578b\u53ef\u7528\u6027"),
      bullet("logs \u547d\u4ee4\uff1a\u67e5\u770b\u670d\u52a1\u65e5\u5fd7\uff08\u652f\u6301 --lines \u53c2\u6570\uff09"),
      bullet("Open WebUI \u96c6\u6210\uff1a\u7aef\u53e3 3001\uff0c\u8fde\u63a5 Worker API\uff0cWEBUI_AUTH=false \u81ea\u52a8\u767b\u5f55"),
      para(""),

      // === 3. 第四阶段 ===
      heading("3. \u7b2c\u56db\u9636\u6bb5\uff1a\u590d\u6742\u4efb\u52a1\u7f16\u6392\u4e0e\u53ef\u89c6\u5316\u7ba1\u7406", 1),

      heading("3.1 \u4ea4\u4ed8\u7269", 2),
      new Table({
        width: { size: 100, type: WidthType.PERCENTAGE },
        columnWidths: [3120, 6240],
        rows: [
          row([cell("\u6587\u4ef6", 3120, { bold: true, shading: "D5E8F0" }), cell("\u4f5c\u7528", 6240, { bold: true, shading: "D5E8F0" })]),
          row([cell("task_orchestrator.py", 3120), cell("\u590d\u6742\u4efb\u52a1\u7f16\u6392\u5f15\u64ce\uff08\u590d\u6742\u5ea6\u68c0\u6d4b \u2192 \u4efb\u52a1\u5206\u89e3 \u2192 \u6210\u672c\u89c4\u5212 \u2192 \u591a\u6a21\u578b\u6267\u884c \u2192 \u7ed3\u679c\u6c47\u603b\uff09\uff0c504 \u884c", 6240)]),
          row([cell("webapp/app.py", 3120), cell("Flask Web \u5e94\u7528\uff08\u4eea\u8868\u76d8 + \u667a\u80fd\u5bf9\u8bdd SSE + \u914d\u7f6e\u5411\u5bfc + \u6a21\u578b\u7ba1\u7406\uff09\uff0c575 \u884c", 6240)]),
          row([cell("webapp/requirements.txt", 3120), cell("Web \u5e94\u7528\u4f9d\u8d56\uff08flask\uff09", 6240)]),
          row([cell("docker-compose.yml\uff08\u66f4\u65b0\uff09", 3120), cell("\u65b0\u589e orchestrator-web \u670d\u52a1\uff08\u7aef\u53e3 3002\uff09\uff0c\u542b env_file/extra_hosts", 6240)]),
          row([cell("litellm_cli.py\uff08\u66f4\u65b0\uff09", 3120), cell("\u65b0\u589e orchestrate \u547d\u4ee4\uff0c\u652f\u6301 CLI \u7f16\u6392\u590d\u6742\u4efb\u52a1", 6240)]),
          row([cell("install.sh\uff08\u66f4\u65b0\uff09", 3120), cell("\u5b89\u88c5\u63d0\u793a\u4e2d\u589e\u52a0\u7f16\u6392\u5e73\u53f0\u7aef\u53e3\u548c orchestrate \u547d\u4ee4", 6240)]),
        ]
      }),

      heading("3.2 \u590d\u6742\u4efb\u52a1\u7f16\u6392\u5f15\u64ce", 2),
      para("\u7f16\u6392\u5f15\u64ce\u5b9e\u73b0\u4e94\u6b65\u6d41\u7a0b\uff1a"),
      bullet("\u6b65\u9aa4 1 \u2014 \u590d\u6742\u5ea6\u68c0\u6d4b\uff1a\u6b63\u5219\u89c4\u5219\u68c0\u6d4b\u591a\u6b65\u9aa4\u5173\u952e\u8bcd\u3001\u8d85\u8fc7 100 \u5b57\u3001\u8d85\u8fc7 2 \u53e5\u7684\u8bf7\u6c42"),
      bullet("\u6b65\u9aa4 2 \u2014 \u4efb\u52a1\u5206\u89e3\uff1a\u7528 LLM \u5c06\u590d\u6742\u4efb\u52a1\u62c6\u89e3\u4e3a 2-5 \u4e2a\u5b50\u4efb\u52a1\uff0c\u6807\u6ce8\u4f9d\u8d56\u5173\u7cfb\u548c\u7c7b\u578b"),
      bullet("\u6b65\u9aa4 3 \u2014 \u6210\u672c\u89c4\u5212\uff1a\u4e3a\u6bcf\u4e2a\u5b50\u4efb\u52a1\u9009\u62e9\u6700\u4f18\u6a21\u578b\u5e76\u4f30\u7b97\u6210\u672c\uff08\u6309\u4efb\u52a1\u7c7b\u578b\u5339\u914d\u6a21\u578b\u5b9a\u4ef7\u8868\uff09"),
      bullet("\u6b65\u9aa4 4 \u2014 \u591a\u6a21\u578b\u6267\u884c\uff1a\u6309\u4f9d\u8d56\u987a\u5e8f\u6267\u884c\uff0c\u63a8\u7406\u6a21\u578b\u81ea\u52a8\u542f\u7528\u6d41\u5f0f\u54cd\u5e94"),
      bullet("\u6b65\u9aa4 5 \u2014 \u7ed3\u679c\u6c47\u603b\uff1a\u591a\u5b50\u4efb\u52a1\u7ed3\u679c\u7528 LLM \u6c47\u603b\u4e3a\u8fde\u8d2f\u56de\u7b54"),
      para(""),

      heading("3.3 \u6a21\u578b\u8def\u7531\u7b56\u7565", 2),
      new Table({
        width: { size: 100, type: WidthType.PERCENTAGE },
        columnWidths: [2340, 2340, 2340, 2340],
        rows: [
          row([
            cell("\u4efb\u52a1\u7c7b\u578b", 2340, { bold: true, shading: "D5E8F0" }),
            cell("\u9996\u9009\u6a21\u578b", 2340, { bold: true, shading: "D5E8F0" }),
            cell("\u6210\u672c\u5b9a\u4f4d", 2340, { bold: true, shading: "D5E8F0" }),
            cell("\u8bf4\u660e", 2340, { bold: true, shading: "D5E8F0" })
          ]),
          row([cell("simple_qa", 2340), cell("qwen2.5-local", 2340), cell("\u96f6\u6210\u672c", 2340), cell("Ollama \u672c\u5730")]),
          row([cell("general", 2340), cell("qwen-plus", 2340), cell("\u4f4e\u6210\u672c", 2340), cell("\u767e\u70bc\u6027\u4ef7\u6bd4")]),
          row([cell("coding", 2340), cell("deepseek-v3", 2340), cell("\u4e2d\u6210\u672c", 2340), cell("DeepSeek \u7f16\u7a0b")]),
          row([cell("math_logic", 2340), cell("deepseek-v3", 2340), cell("\u4e2d\u6210\u672c", 2340), cell("DeepSeek \u63a8\u7406")]),
          row([cell("complex_reasoning", 2340), cell("qwen3.6-plus", 2340), cell("\u9ad8\u6210\u672c", 2340), cell("\u767e\u70bc\u9ad8\u8d28\u91cf")]),
        ]
      }),
      para(""),

      heading("3.4 \u53ef\u89c6\u5316\u7ba1\u7406\u754c\u9762", 2),
      para("\u7aef\u53e3 3002 \u63d0\u4f9b\u56db\u4e2a\u9875\u9762\uff1a"),
      bullet("\u4eea\u8868\u76d8\uff08/\uff09\uff1a\u670d\u52a1\u72b6\u6001\u3001\u53ef\u7528\u6a21\u578b\u6570\u3001\u7f13\u5b58\u547d\u4e2d\u3001\u7d2f\u8ba1\u82b1\u8d39"),
      bullet("\u667a\u80fd\u5bf9\u8bdd\uff08/chat\uff09\uff1a\u8f93\u5165\u590d\u6742\u4efb\u52a1\uff0cSSE \u5b9e\u65f6\u63a8\u9001\u4efb\u52a1\u5206\u89e3\u3001\u6a21\u578b\u9009\u62e9\u3001\u6267\u884c\u8fdb\u5ea6"),
      bullet("\u914d\u7f6e\u5411\u5bfc\uff08/config\uff09\uff1a\u67e5\u770b API Key \u72b6\u6001\u3001Ollama \u6a21\u578b\u3001\u4efb\u52a1\u8def\u7531\u6620\u5c04"),
      bullet("\u6a21\u578b\u7ba1\u7406\uff08/models\uff09\uff1a\u67e5\u770b\u6240\u6709\u6a21\u578b\u5b9a\u4ef7\u548c\u72b6\u6001"),
      para(""),

      heading("3.5 \u672c\u6b21\u4fee\u590d\u7684\u95ee\u9898", 2),
      para("\u5728\u9a8c\u8bc1\u8fc7\u7a0b\u4e2d\u53d1\u73b0\u5e76\u4fee\u590d\u4e86\u4ee5\u4e0b\u95ee\u9898\uff1a"),
      bullet("\u590d\u6742\u5ea6\u68c0\u6d4b\u6b63\u5219\u8868\u8fbe\u5f0f\u95ee\u9898\uff1a.+ \u6539\u4e3a .* \u5141\u8bb8\u96f6\u5b57\u7b26\u95f4\u9694\uff08\u5982\u201c\u5e76\u6d4b\u8bd5\u5b83\u201d\uff09"),
      bullet("\u4efb\u52a1\u5206\u89e3\u6d41\u5f0f\u8c03\u7528\u95ee\u9898\uff1acall_llm \u6539\u4e3a call_llm_stream\uff0c\u907f\u514d\u63a8\u7406\u6a21\u578b\u6d41\u5f0f\u54cd\u5e94\u5bfc\u81f4 JSON \u89e3\u6790\u5931\u8d25"),
      bullet("\u5bb9\u5668\u5185 localhost \u7f51\u7edc\u95ee\u9898\uff1a\u4eea\u8868\u76d8 Worker URL \u548c Ollama URL \u6539\u4e3a\u73af\u5883\u53d8\u91cf\u5f15\u7528"),
      bullet("SSE \u4e2d\u6587\u4e71\u7801\u95ee\u9898\uff1amimetype \u6539\u4e3a content_type \u5e76\u6307\u5b9a charset=utf-8"),
      new Paragraph({ children: [new PageBreak()] }),

      // === 4. Docker 部署包 ===
      heading("4. Docker \u90e8\u7f72\u5305", 1),
      para("\u4e3a\u4e86\u65b9\u4fbf\u5ba2\u6237\u5728\u4e3b\u673a\u4e0a\u5feb\u901f\u90e8\u7f72\uff0c\u5b9e\u73b0\u4e86 Docker \u90e8\u7f72\u5305\uff0c\u5305\u542b\u4ee5\u4e0b\u5185\u5bb9\uff1a"),
      bullet("\u6253\u5305\u811a\u672c\uff08build-package.sh\uff09\uff1a\u5c06\u6240\u6709\u914d\u7f6e\u6587\u4ef6\u6253\u5305\u4e3a\u53ef\u5206\u53d1\u7684 tar.gz \u538b\u7f29\u5305"),
      bullet("\u79bb\u7ebf\u5b89\u88c5\u811a\u672c\uff08offline-install.sh\uff09\uff1a\u89e3\u538b\u540e\u4e00\u952e\u5b89\u88c5\uff0c\u65e0\u9700\u8054\u7f51\u7f51\u7edc"),
      bullet("\u955c\u50cf\u540d\u5355\uff08images.list\uff09\uff1a\u5217\u51fa\u6240\u6709\u4f9d\u8d56\u7684 Docker \u955c\u50cf\uff0c\u652f\u6301\u79bb\u7ebf\u5bfc\u5165"),
      bullet("\u955c\u50cf\u5bfc\u51fa/\u5bfc\u5165\u811a\u672c\uff08save-images.sh / load-images.sh\uff09\uff1a\u652f\u6301\u79bb\u7ebf\u73af\u5883\u955c\u50cf\u8fc1\u79fb"),
      para(""),

      // === 5. Tauri 桌面应用 ===
      heading("5. Tauri \u684c\u9762\u5e94\u7528", 1),
      para("\u4e3a\u4e86\u63d0\u4f9b\u539f\u751f\u684c\u9762\u4f53\u9a8c\uff0c\u5b9e\u73b0\u4e86 Tauri \u684c\u9762\u5e94\u7528\uff0c\u5305\u542b\u4ee5\u4e0b\u5185\u5bb9\uff1a"),
      bullet("\u524d\u7aef\u754c\u9762\uff08src-tauri/ui/\uff09\uff1a\u5d4c\u5165\u5f0f Web \u754c\u9762\uff0c\u5305\u542b\u4eea\u8868\u76d8\u3001\u5bf9\u8bdd\u3001\u914d\u7f6e\u9875\u9762"),
      bullet("Rust \u540e\u7aef\uff08src-tauri/src/\uff09\uff1a\u5904\u7406\u672c\u5730\u670d\u52a1\u7ba1\u7406\u3001Docker \u542f\u505c\u3001\u7cfb\u7edf\u6258\u76d8"),
      bullet("Tauri \u914d\u7f6e\uff08tauri.conf.json\uff09\uff1a\u7a97\u53e3\u5c3a\u5bf8\u3001\u56fe\u6807\u3001\u6807\u9898\u3001\u672c\u5730\u8d44\u6e90\u8bbf\u95ee\u6743\u9650"),
      bullet("\u6784\u5efa\u811a\u672c\uff08build-tauri.sh\uff09\uff1a\u4e00\u952e\u5b89\u88c5\u4f9d\u8d56\u5e76\u6784\u5efa\u684c\u9762\u5e94\u7528"),
      bullet("\u7cfb\u7edf\u6258\u76d8\uff1a\u663e\u793a\u670d\u52a1\u72b6\u6001\u3001\u5feb\u901f\u542f\u505c\u670d\u52a1\u3001\u4e00\u952e\u6253\u5f00 Web \u754c\u9762"),
      para(""),

      // === 6. 功能目标总结 ===
      heading("6. \u529f\u80fd\u76ee\u6807\u603b\u7ed3", 1),
      new Table({
        width: { size: 100, type: WidthType.PERCENTAGE },
        columnWidths: [2340, 1560, 2340, 3120],
        rows: [
          row([
            cell("\u529f\u80fd\u6a21\u5757", 2340, { bold: true, shading: "D5E8F0" }),
            cell("\u72b6\u6001", 1560, { bold: true, shading: "D5E8F0" }),
            cell("\u9636\u6bb5", 2340, { bold: true, shading: "D5E8F0" }),
            cell("\u8bf4\u660e", 3120, { bold: true, shading: "D5E8F0" })
          ]),
          row([cell("\u591a\u6a21\u578b\u667a\u80fd\u8def\u7531", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u4e8c\u9636\u6bb5", 2340), cell("\u89c4\u5219 + LLM \u6df7\u5408\u5206\u7c7b\uff0c5 \u79cd\u4efb\u52a1\u7c7b\u578b", 3120)]),
          row([cell("\u914d\u989d\u7ba1\u7406", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u4e8c\u9636\u6bb5", 2340), cell("\u6309 Key/User \u8ffd\u8e2a\u82b1\u8d39\uff0c\u9884\u7b97\u4e0a\u9650\u63a7\u5236", 3120)]),
          row([cell("\u8bed\u4e49\u7f13\u5b58", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u4e8c\u9636\u6bb5", 2340), cell("Qdrant \u5411\u91cf\u76f8\u4f3c\u5ea6\u5339\u914d\uff0c\u767e\u70bc Embedding", 3120)]),
          row([cell("CLI \u5de5\u5177", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u4e09\u9636\u6bb5", 2340), cell("7 \u4e2a\u547d\u4ee4\uff0c\u96f6\u5916\u90e8\u4f9d\u8d56", 3120)]),
          row([cell("\u4e00\u952e\u5b89\u88c5", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u4e09\u9636\u6bb5", 2340), cell("\u4ea4\u4e92\u5f0f\u914d\u7f6e\u5411\u5bfc\uff0c\u81ea\u52a8\u68c0\u6d4b\u73af\u5883", 3120)]),
          row([cell("\u804a\u5929\u524d\u7aef", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u4e09\u9636\u6bb5", 2340), cell("Open WebUI\uff0c\u7aef\u53e3 3001", 3120)]),
          row([cell("\u590d\u6742\u4efb\u52a1\u7f16\u6392", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u56db\u9636\u6bb5", 2340), cell("\u81ea\u52a8\u62c6\u89e3\u3001\u6210\u672c\u89c4\u5212\u3001\u591a\u6a21\u578b\u6267\u884c", 3120)]),
          row([cell("\u53ef\u89c6\u5316\u7ba1\u7406", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u7b2c\u56db\u9636\u6bb5", 2340), cell("\u4eea\u8868\u76d8+\u5bf9\u8bdd+\u914d\u7f6e+\u6a21\u578b\u7ba1\u7406\uff0cSSE \u5b9e\u65f6\u63a8\u9001", 3120)]),
          row([cell("Docker \u90e8\u7f72\u5305", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u8865\u5145\u4ea4\u4ed8", 2340), cell("\u53ef\u5206\u53d1\u538b\u7f29\u5305\uff0c\u652f\u6301\u79bb\u7ebf\u5b89\u88c5", 3120)]),
          row([cell("Tauri \u684c\u9762\u5e94\u7528", 2340), cell("\u2713 \u5df2\u5b8c\u6210", 1560), cell("\u8865\u5145\u4ea4\u4ed8", 2340), cell("\u539f\u751f\u684c\u9762\u4f53\u9a8c\uff0c\u7cfb\u7edf\u6258\u76d8\uff0c\u4e00\u952e\u7ba1\u7406", 3120)]),
          row([cell("vLLM \u8bed\u4e49\u8def\u7531", 2340), cell("\u25cb \u89c4\u5212\u4e2d", 1560), cell("\u540e\u7eed\u9636\u6bb5", 2340), cell("6 \u4fe1\u53f7\u8def\u7531\uff0c\u63d2\u4ef6\u94fe\u67b6\u6784", 3120)]),
        ]
      }),
      para(""),

      // === 7. 系统架构 ===
      heading("7. \u7cfb\u7edf\u67b6\u6784", 1),
      para("\u5f53\u524d\u7cfb\u7edf\u5305\u542b 9 \u4e2a Docker \u670d\u52a1\uff1a"),
      new Table({
        width: { size: 100, type: WidthType.PERCENTAGE },
        columnWidths: [2340, 1560, 5460],
        rows: [
          row([
            cell("\u670d\u52a1\u540d\u79f0", 2340, { bold: true, shading: "D5E8F0" }),
            cell("\u7aef\u53e3", 1560, { bold: true, shading: "D5E8F0" }),
            cell("\u804c\u8d23", 5460, { bold: true, shading: "D5E8F0" })
          ]),
          row([cell("litellm-admin", 2340), cell("4000", 1560), cell("\u63a7\u5236\u9762 Admin\uff08\u7ba1\u7406 UI + \u914d\u7f6e API\uff09", 5460)]),
          row([cell("litellm-worker", 2340), cell("4001", 1560), cell("\u6570\u636e\u9762 Worker\uff08\u63a8\u7406 API + \u667a\u80fd\u8def\u7531 + \u914d\u989d\u8ffd\u8e2a\uff09", 5460)]),
          row([cell("open-webui", 2340), cell("3001", 1560), cell("Open WebUI \u804a\u5929\u754c\u9762", 5460)]),
          row([cell("orchestrator-web", 2340), cell("3002", 1560), cell("\u53ef\u89c6\u5316\u7ba1\u7406\uff08\u4eea\u8868\u76d8 + \u667a\u80fd\u5bf9\u8bdd + \u914d\u7f6e\u5411\u5bfc + \u6a21\u578b\u7ba1\u7406\uff09", 5460)]),
          row([cell("grafana", 2340), cell("3000", 1560), cell("Grafana \u53ef\u89c6\u5316\u4eea\u8868\u76d8", 5460)]),
          row([cell("prometheus", 2340), cell("9090", 1560), cell("Prometheus \u6307\u6807\u91c7\u96c6", 5460)]),
          row([cell("redis", 2340), cell("6379", 1560), cell("\u8def\u7531\u72b6\u6001 + \u9650\u6d41\uff08\u5bc6\u7801\u8ba4\u8bc1 + \u5371\u9669\u547d\u4ee4\u7981\u7528\uff09", 5460)]),
          row([cell("qdrant", 2340), cell("6333", 1560), cell("\u8bed\u4e49\u7f13\u5b58\u5411\u91cf\u6570\u636e\u5e93", 5460)]),
          row([cell("db (PostgreSQL)", 2340), cell("5432", 1560), cell("Admin/Worker \u5171\u4eab\u6570\u636e\u5e93", 5460)]),
        ]
      }),
      para(""),
      para("\u4ee3\u7801\u7edf\u8ba1\uff1a2676 \u884c Python \u4ee3\u7801\uff08custom_callbacks.py 484 \u884c + litellm_cli.py 1112 \u884c + task_orchestrator.py 504 \u884c + webapp/app.py 575 \u884c\uff09\u3002"),
      para(""),

      // === 8. 验证结果 ===
      heading("8. \u9a8c\u8bc1\u7ed3\u679c", 1),
      para("\u6240\u6709 9 \u4e2a\u5bb9\u5668\u8fd0\u884c\u6b63\u5e38\uff0c\u5173\u952e\u529f\u80fd\u9a8c\u8bc1\u901a\u8fc7\uff1a"),
      bullet("\u5bb9\u5668\u5185\u8fde\u901a\u6027\uff1aWorker \u53ef\u8fbe\uff08\"I'm alive!\"\uff09\uff0cOllama \u53ef\u8fbe\uff08\u6a21\u578b\u5217\u8868\u8fd4\u56de\uff09"),
      bullet("\u4eea\u8868\u76d8\u663e\u793a\uff1aWorker \u72b6\u6001=\u6b63\u5e38\uff0c10 \u4e2a\u53ef\u7528\u6a21\u578b\uff0c\u7d2f\u8ba1\u82b1\u8d39 $0.006856"),
      bullet("CLI \u7f16\u6392\u6d4b\u8bd5\uff1a\u201c\u5199\u4e00\u4e2aPython\u722c\u866b\u5e76\u6d4b\u8bd5\u5b83\u201d \u6210\u529f\u5206\u89e3\u4e3a 5 \u4e2a\u5b50\u4efb\u52a1\uff0c\u6309\u4f9d\u8d56\u6267\u884c\uff0c\u6210\u672c $0.013"),
      bullet("SSE \u6d41\u5f0f\u5bf9\u8bdd\uff1a\u4e2d\u6587\u7f16\u7801\u6b63\u786e\uff0c5 \u4e2a\u5b50\u4efb\u52a1\u5b9e\u65f6\u63a8\u9001\u6210\u529f"),
      bullet("\u914d\u7f6e\u5411\u5bfc\uff1aDASHSCOPE_API_KEY \u5df2\u914d\u7f6e\uff0cOLLAMA_API_BASE \u5df2\u914d\u7f6e\uff0c5 \u79cd\u4efb\u52a1\u8def\u7531\u6620\u5c04\u6b63\u786e\u663e\u793a"),
      para(""),

      // === 9. 后续规划 ===
      heading("9. \u540e\u7eed\u89c4\u5212", 1),
      bullet("vLLM \u8bed\u4e49\u8def\u7531\u96c6\u6210\uff1a6 \u4fe1\u53f7\u8def\u7531\uff08keyword + embedding + domain + fact_check + user_feedback + preference\uff09\uff0c\u63d2\u4ef6\u94fe\u67b6\u6784\uff08\u8bed\u4e49\u7f13\u5b58/jailbreak/PII/\u5e7b\u89c9\u68c0\u6d4b\uff09"),
      bullet("Domain \u5206\u7c7b\uff08MMLU 14 \u7c7b\uff09\uff1a\u66f4\u7cbe\u7ec6\u7684\u4efb\u52a1\u5206\u7c7b\uff0c\u63d0\u5347\u8def\u7531\u51c6\u786e\u5ea6"),
      bullet("PII / Jailbreak / Hallucination \u68c0\u6d4b\u63d2\u4ef6\uff1a\u9690\u79c1\u4fdd\u62a4\u3001\u5b89\u5168\u9632\u62a4\u3001\u8d28\u91cf\u4fdd\u969c"),
      para(""),
      para(""),
      para("\u2014 \u6c47\u62a5\u7ed3\u675f \u2014", { size: 22, color: "999999" }),
    ]
  }]
});

const outputPath = path.join(__dirname, '..', 'LiteLLM_工作汇报.docx');

Packer.toBuffer(doc).then(buffer => {
  fs.writeFileSync(outputPath, buffer);
  console.log(`Word 文档已生成: ${outputPath}`);
});
