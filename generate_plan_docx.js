const { Document, Packer, Paragraph, TextRun, Table, TableRow, TableCell,
        HeadingLevel, AlignmentType, BorderStyle, WidthType, ShadingType,
        LevelFormat, PageBreak, PageNumber, Header, Footer, VerticalAlign } = require('docx');
const fs = require('fs');

const cjkFont = "PingFang SC";
const asciiFont = "Arial";
const font = { ascii: asciiFont, hAnsi: asciiFont, eastAsia: cjkFont };

const border = { style: BorderStyle.SINGLE, size: 1, color: "CCCCCC" };
const borders = { top: border, bottom: border, left: border, right: border };
const cellMargins = { top: 60, bottom: 60, left: 100, right: 100 };

function h1(text) {
  return new Paragraph({ heading: HeadingLevel.HEADING_1, children: [new TextRun({ text, font, bold: true, size: 32 })] });
}
function h2(text) {
  return new Paragraph({ heading: HeadingLevel.HEADING_2, children: [new TextRun({ text, font, bold: true, size: 28 })] });
}
function h3(text) {
  return new Paragraph({ heading: HeadingLevel.HEADING_3, children: [new TextRun({ text, font, bold: true, size: 24 })] });
}
function p(text) {
  return new Paragraph({ children: [new TextRun({ text, font, size: 22 })] });
}
function pBold(text) {
  return new Paragraph({ children: [new TextRun({ text, font, bold: true, size: 22 })] });
}
function code(text) {
  return new Paragraph({
    children: [new TextRun({ text, font: { ascii: "Courier New", hAnsi: "Courier New", eastAsia: cjkFont }, size: 20 })],
    spacing: { before: 60, after: 60 }
  });
}
function bullet(text, level = 0) {
  return new Paragraph({
    numbering: { reference: "bullets", level },
    children: [new TextRun({ text, font, size: 22 })]
  });
}
function numbered(text, ref = "num1", level = 0) {
  return new Paragraph({
    numbering: { reference: ref, level },
    children: [new TextRun({ text, font, size: 22 })]
  });
}

function makeTable(headers, rows, colWidths) {
  const totalWidth = colWidths.reduce((a, b) => a + b, 0);
  const headerRow = new TableRow({
    cantSplit: true,
    children: headers.map((text, i) => new TableCell({
      borders,
      width: { size: colWidths[i], type: WidthType.DXA },
      shading: { fill: "D5E8F0", type: ShadingType.CLEAR },
      margins: cellMargins,
      verticalAlign: VerticalAlign.CENTER,
      children: [new Paragraph({ children: [new TextRun({ text, font, bold: true, size: 20 })] })]
    }))
  });
  const dataRows = rows.map(row => new TableRow({
    cantSplit: true,
    children: row.map((text, i) => new TableCell({
      borders,
      width: { size: colWidths[i], type: WidthType.DXA },
      margins: cellMargins,
      verticalAlign: VerticalAlign.CENTER,
      children: [new Paragraph({ children: [new TextRun({ text: String(text), font, size: 20 })] })]
    }))
  }));
  return new Table({
    width: { size: totalWidth, type: WidthType.DXA },
    columnWidths: colWidths,
    rows: [headerRow, ...dataRows]
  });
}

function spacer() {
  return new Paragraph({ children: [new TextRun({ text: "", font, size: 12 })] });
}

const doc = new Document({
  styles: {
    default: {
      document: { run: { font, size: 22 } }
    },
    paragraphStyles: [
      { id: "Heading1", name: "Heading 1", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 32, bold: true, font },
        paragraph: { spacing: { before: 240, after: 120 }, outlineLevel: 0, keepNext: false, keepLines: false } },
      { id: "Heading2", name: "Heading 2", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 28, bold: true, font },
        paragraph: { spacing: { before: 200, after: 100 }, outlineLevel: 1, keepNext: false, keepLines: false } },
      { id: "Heading3", name: "Heading 3", basedOn: "Normal", next: "Normal", quickFormat: true,
        run: { size: 24, bold: true, font },
        paragraph: { spacing: { before: 160, after: 80 }, outlineLevel: 2, keepNext: false, keepLines: false } },
    ]
  },
  numbering: {
    config: [
      { reference: "bullets", levels: [
        { level: 0, format: LevelFormat.BULLET, text: "\u2022", alignment: AlignmentType.LEFT,
          style: { paragraph: { indent: { left: 720, hanging: 360 } } } },
        { level: 1, format: LevelFormat.BULLET, text: "\u25E6", alignment: AlignmentType.LEFT,
          style: { paragraph: { indent: { left: 1440, hanging: 360 } } } },
      ]},
      { reference: "num1", levels: [{ level: 0, format: LevelFormat.DECIMAL, text: "%1.", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 720, hanging: 360 } } } }] },
      { reference: "num2", levels: [{ level: 0, format: LevelFormat.DECIMAL, text: "%1.", alignment: AlignmentType.LEFT, style: { paragraph: { indent: { left: 720, hanging: 360 } } } }] },
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
      default: new Header({ children: [new Paragraph({ alignment: AlignmentType.RIGHT, children: [new TextRun({ text: "LLooM v2 \u91CD\u6784\u8BA1\u5212", font, size: 18, color: "999999" })] })] })
    },
    footers: {
      default: new Footer({ children: [new Paragraph({ alignment: AlignmentType.CENTER, children: [
        new TextRun({ text: "\u7B2C ", font, size: 18, color: "999999" }),
        new TextRun({ children: [PageNumber.CURRENT], font, size: 18, color: "999999" }),
        new TextRun({ text: " \u9875", font, size: 18, color: "999999" })
      ] })] })
    },
    children: [
      // === 封面 ===
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 4000 }, children: [
        new TextRun({ text: "LLooM v2", font, bold: true, size: 56 })
      ]}),
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 200 }, children: [
        new TextRun({ text: "\u91CD\u6784\u8BA1\u5212\u4ECB\u7ECD\u6587\u6863", font, bold: true, size: 36 })
      ]}),
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 400 }, children: [
        new TextRun({ text: "Core + GUI \u4E24\u5C42\u67B6\u6784 \u00B7 \u81EA\u5305\u542B\u684C\u9762\u5E94\u7528", font, size: 24, color: "666666" })
      ]}),
      new Paragraph({ alignment: AlignmentType.CENTER, spacing: { before: 200 }, children: [
        new TextRun({ text: "2026\u5E748\u6708", font, size: 22, color: "999999" })
      ]}),
      new Paragraph({ children: [new PageBreak()] }),

      // === 目录 ===
      h1("\u76EE\u5F55"),
      p("1. \u67B6\u6784\u603B\u89C8"),
      p("2. \u6253\u5305\u6784\u5EFA\u6D41\u6C34\u7EBF"),
      p("3. Phase 0: \u9879\u76EE\u9AA8\u67B6\u4E0E\u6253\u5305\u9A8C\u8BC1"),
      p("4. Phase 1: \u6838\u5FC3\u76EE\u68071 \u2014 \u6A21\u578B\u96C6\u7EA6\u5316\u7BA1\u7406"),
      p("5. Phase 2: \u6838\u5FC3\u76EE\u68072 \u2014 \u667A\u80FD\u8C03\u7528\u89C4\u5212"),
      p("6. Phase 3: \u6838\u5FC3\u76EE\u68073 \u2014 \u8BED\u4E49\u611F\u77E5\u4EFB\u52A1\u5206\u914D"),
      p("7. Phase 4: \u5B89\u5168\u5C42"),
      p("8. Phase 5: API \u670D\u52A1\u5C42"),
      p("9. Phase 6: CLI \u5DE5\u5177"),
      p("10. Phase 7: Tauri GUI \u4E0E\u8FDB\u7A0B\u7BA1\u7406"),
      p("11. Phase 8: \u6253\u5305\u6784\u5EFA"),
      p("12. Phase 9: \u96C6\u6210\u6D4B\u8BD5\u4E0E\u53D1\u5E03"),
      p("13. \u5F00\u53D1\u987A\u5E8F\u4E0E\u4F9D\u8D56\u5173\u7CFB"),
      p("14. v1 \u2192 v2 \u4EE3\u7801\u8FC1\u79FB\u6E05\u5355"),
      new Paragraph({ children: [new PageBreak()] }),

      // === 1. 架构总览 ===
      h1("1. \u67B6\u6784\u603B\u89C8"),
      p("LLooM v2 \u91C7\u7528 Core + GUI \u4E24\u5C42\u67B6\u6784\uFF0C\u57FA\u4E8E LiteLLM Python SDK \u4F5C\u4E3A\u5E93\u76F4\u63A5\u8C03\u7528\uFF0C\u65E0\u9700 Docker \u5BB9\u5668\u5316\u90E8\u7F72\u3002\u5E94\u7528\u5B8C\u5168\u81EA\u5305\u542B\uFF1A\u5D4C\u5165\u5F0F Python 3.11 \u8FD0\u884C\u65F6 + Ollama \u4E8C\u8FDB\u5236\u5185\u7F6E\uFF0C\u7528\u6237\u53CC\u51FB .app \u5373\u53EF\u8FD0\u884C\u3002"),
      spacer(),
      pBold("\u6838\u5FC3\u7EC4\u6210\u90E8\u5206\uFF1A"),
      bullet("Tauri \u684C\u9762\u5E94\u7528 (GUI)\uFF1A5 \u9875\u9762\uFF08\u603B\u89C8/\u7528\u91CF/\u5BF9\u8BDD/\u6A21\u578B\u7BA1\u7406/\u8BBE\u7F6E\uFF09\uFF0C\u542F\u52A8\u65F6\u81EA\u52A8\u62C9\u8D77 Ollama \u548C Python API \u5B50\u8FDB\u7A0B"),
      bullet("Python API \u670D\u52A1\uFF1auvicorn + FastAPI\uFF0C\u76D1\u542C localhost:7860\uFF0C\u63D0\u4F9B REST + SSE \u7AEF\u70B9"),
      bullet("Core \u6838\u5FC3\u5C42\uFF1aModelManager(\u76EE\u68071) + SmartRouter(\u76EE\u68072) + Orchestrator(\u76EE\u68073) + SecurityFilter + SemanticCache"),
      bullet("litellm SDK\uFF1aRouter + Cache + completion() + callbacks\uFF0C\u4F5C\u4E3A Python \u5E93\u76F4\u63A5 import"),
      bullet("Ollama \u4E8C\u8FDB\u5236\uFF1a\u5185\u7F6E\u5728\u5E94\u7528\u4E2D\uFF0C\u96F6\u6210\u672C\u5146\u5E95\u5C42 (qwen2.5:latest)"),
      spacer(),
      pBold("\u4E0E v1 \u7684\u6838\u5FC3\u533A\u522B\uFF1A"),
      spacer(),
      makeTable(
        ["\u7EF4\u5EA6", "v1 (\u73B0\u7248)", "v2 (\u65B0\u7248)"],
        [
          ["\u5916\u90E8\u670D\u52A1", "Docker 10 \u5BB9\u5668", "\u65E0\uFF08\u7EAF Python\uFF09"],
          ["\u6570\u636E\u5E93", "PostgreSQL", "SQLite\uFF08\u672C\u5730\u6587\u4EF6\uFF09"],
          ["\u7F13\u5B58", "Qdrant + Redis", "ChromaDB + disk"],
          ["\u76D1\u63A7", "Prometheus + Grafana", "SQLite \u8BB0\u5F55 + Tauri \u5185\u7F6E\u56FE\u8868"],
          ["LLM \u8C03\u7528", "HTTP \u2192 LiteLLM Proxy", "litellm SDK \u5E93\u76F4\u63A5\u8C03\u7528"],
          ["\u542F\u52A8\u65B9\u5F0F", "./install.sh \u2192 docker compose", "\u53CC\u51FB .app \u81EA\u52A8\u542F\u52A8"],
          ["\u4F9D\u8D56\u4F53\u79EF", "~10GB Docker \u955C\u50CF", "~400-600MB\uFF08\u6A21\u578B\u53E6\u4E0B\u8F7D\uFF09"]
        ],
        [2000, 3680, 3680]
      ),
      new Paragraph({ children: [new PageBreak()] }),

      // === 2. 打包构建流水线 ===
      h1("2. \u6253\u5305\u6784\u5EFA\u6D41\u6C34\u7EBF"),
      p("\u5F00\u53D1\u6A21\u5F0F\u548C\u53D1\u5E03\u6784\u5EFA\u6D41\u7A0B\u4E0D\u540C\uFF1A"),
      spacer(),
      h3("\u5F00\u53D1\u6A21\u5F0F\uFF08\u65E5\u5E38\u5F00\u53D1\uFF09"),
      code("pip install -e .          # \u5B89\u88C5 core \u5305\u5230\u5F53\u524D Python \u73AF\u5883"),
      code("lloom init                 # \u521D\u59CB\u5316\u914D\u7F6E + \u62C9\u53D6 Ollama \u6A21\u578B"),
      code("lloom serve                # \u542F\u52A8 API \u670D\u52A1 (:7860)"),
      code("cd tauri-app && cargo tauri dev   # \u542F\u52A8 Tauri \u5F00\u53D1\u6A21\u5F0F"),
      spacer(),
      h3("\u53D1\u5E03\u6784\u5EFA\uFF08\u6253\u5305\u53D1\u5E03\uFF09"),
      numbered("pyinstaller --onecore --name lloom-server api/server.py \u2192 \u751F\u6210\u72EC\u7ACB\u4E8C\u8FDB\u5236", "num1"),
      numbered("\u4E0B\u8F7D ollama macOS ARM64 \u4E8C\u8FDB\u5236 \u2192 \u653E\u5165 tauri-app/src-tauri/resources/ollama", "num1"),
      numbered("\u9996\u6B21\u8FD0\u884C\u65F6\u81EA\u52A8\u62C9\u53D6 qwen2.5:latest \u6A21\u578B\uFF08\u7EA6 4GB\uFF0C\u4E0D\u6253\u5305\u8FDB\u5E94\u7528\u907F\u514D\u4F53\u79EF\u8FC7\u5927\uFF09", "num1"),
      numbered("cargo tauri build \u2192 \u751F\u6210 LLooM.app", "num1"),
      numbered("\u538B\u7F29 .app \u2192 GitHub Release", "num1"),
      spacer(),
      h3("Tauri \u542F\u52A8\u6D41\u7A0B"),
      numbered("App \u542F\u52A8 \u2192 \u68C0\u67E5\u5E76\u542F\u52A8 Ollama \u5B50\u8FDB\u7A0B \u2192 \u8F6E\u8BE2 localhost:11434 \u5C31\u7EEA\uFF08\u6700\u591A 30s\uFF09", "num2"),
      numbered("\u542F\u52A8 Python API \u5B50\u8FDB\u7A0B \u2192 \u8F6E\u8BE2 localhost:7860/api/health \u5C31\u7EEA\uFF08\u6700\u591A 15s\uFF09", "num2"),
      numbered("\u52A0\u8F7D\u524D\u7AEF UI", "num2"),
      numbered("\u9996\u6B21\u8FD0\u884C \u2192 \u68C0\u67E5 Ollama \u6A21\u578B \u2192 \u4E0D\u5B58\u5728\u5219\u63D0\u793A\u62C9\u53D6", "num2"),
      numbered("\u9000\u51FA\u65F6 \u2192 kill \u4E24\u4E2A\u5B50\u8FDB\u7A0B", "num2"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 0 ===
      h1("3. Phase 0: \u9879\u76EE\u9AA8\u67B6\u4E0E\u6253\u5305\u9A8C\u8BC1"),
      pBold("\u76EE\u6807\uFF1A\u642D\u5EFA\u9879\u76EE\u7ED3\u6784\uFF0C\u9A8C\u8BC1 PyInstaller + Tauri \u6253\u5305\u53EF\u884C\u6027"),
      spacer(),
      pBold("\u9879\u76EE\u76EE\u5F55\u7ED3\u6784\uFF1A"),
      code("lloom/"),
      code("  core/                     # \u6838\u5FC3\u5C42"),
      code("    __init__.py"),
      code("    config.py               # \u914D\u7F6E\u7BA1\u7406 (.env \u8BFB\u5199)"),
      code("    database.py              # SQLite schema + CRUD"),
      code("    model_manager.py        # \u76EE\u68071"),
      code("    smart_router.py          # \u76EE\u68072"),
      code("    orchestrator.py          # \u76EE\u68073"),
      code("    security.py              # PII/\u8D8A\u72F1/\u57DF\u5206\u7C7B"),
      code("    cache.py                 # \u8BED\u4E49\u7F13\u5B58 (ChromaDB)"),
      code("    callbacks.py             # litellm \u56DE\u8C03 (\u7528\u91CF\u8BB0\u5F55)"),
      code("  api/"),
      code("    __init__.py"),
      code("    server.py                # FastAPI \u670D\u52A1"),
      code("  cli/"),
      code("    lloom.py                 # CLI \u5165\u53E3"),
      code("  tauri-app/                 # \u4ECE v1 \u8FC1\u79FB"),
      code("  data/                      # \u8FD0\u884C\u65F6\u6570\u636E (gitignore)"),
      code("  pyproject.toml"),
      code("  .env.example"),
      spacer(),
      pBold("\u4F9D\u8D56\u5B9A\u4E49\uFF1A"),
      bullet("litellm >= 1.82.0 \u2014 LLM SDK \u6838\u5FC3"),
      bullet("fastapi >= 0.115.0 + uvicorn >= 0.30.0 \u2014 API \u670D\u52A1"),
      bullet("chromadb >= 0.5.0 \u2014 \u672C\u5730\u5411\u91CF\u7F13\u5B58"),
      bullet("click >= 8.1.0 \u2014 CLI \u6846\u67B6"),
      bullet("python-dotenv >= 1.0.0 \u2014 \u73AF\u5883\u53D8\u91CF\u7BA1\u7406"),
      spacer(),
      pBold("\u6253\u5305\u9A8C\u8BC1\uFF08\u5173\u952E\u9A8C\u8BC1\u70B9\uFF09\uFF1A"),
      bullet("\u5199\u6700\u5C0F api/server.py\uFF08\u53EA\u8FD4\u56DE {\"status\": \"ok\"}\uFF09"),
      bullet("\u7528 PyInstaller \u6253\u5305\uFF0C\u786E\u8BA4 litellm + fastapi + chromadb \u90FD\u80FD\u6B63\u786E\u6253\u5305"),
      bullet("\u5728 Tauri \u4E2D\u62C9\u8D77\u6253\u5305\u540E\u7684\u4E8C\u8FDB\u5236\uFF0C\u786E\u8BA4\u80FD\u6B63\u5E38\u542F\u52A8"),
      bullet("\u6B64\u6B65\u9A8C\u8BC1\u6210\u529F\u540E\u624D\u7EE7\u7EED\u540E\u7EED\u5F00\u53D1"),
      spacer(),
      pBold("\u5173\u952E\u98CE\u9669\uFF1A"),
      bullet("litellm SDK \u4F9D\u8D56\u8F83\u591A\uFF0CPyInstaller \u53EF\u80FD\u9057\u6F0F\u9690\u5F0F\u5BFC\u5165 \u2192 \u9700\u8981 hiddenimports \u914D\u7F6E"),
      bullet("ChromaDB \u4F9D\u8D56 onnxruntime + \u6A21\u578B\u6587\u4EF6\uFF0C\u6253\u5305\u4F53\u79EF\u53EF\u80FD\u504F\u5927 \u2192 \u9700\u8981\u9A8C\u8BC1"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 1 ===
      h1("4. Phase 1: \u6838\u5FC3\u76EE\u68071 \u2014 \u6A21\u578B\u96C6\u7EA6\u5316\u7BA1\u7406"),
      pBold("\u76EE\u6807\uFF1A\u7EDF\u4E00\u6CE8\u518C\u6A21\u578B\uFF0C\u8FFD\u8E2A token \u7528\u91CF\uFF0C\u8BA1\u7B97\u6210\u672C\uFF0C\u63A7\u5236\u9884\u7B97"),
      spacer(),
      h2("SQLite \u6570\u636E\u5C42"),
      p("\u4E09\u5F20\u8868\uFF1amodels\uFF08\u6A21\u578B\u5B9A\u4E49\uFF09\u3001usage_records\uFF08\u6BCF\u6B21\u8C03\u7528\u7684 token/\u6210\u672C\u8BB0\u5F55\uFF09\u3001budgets\uFF08\u9884\u7B97\u914D\u7F6E\uFF09\u3002models \u8868\u5B58\u50A8\u6A21\u578B\u540D\u79F0\u3001\u4F9B\u5E94\u5546\u3001API \u7AEF\u70B9\u3001\u5355\u4EF7\u3001RPM \u7B49\u4FE1\u606F\u3002usage_records \u8BB0\u5F55\u6BCF\u6B21\u8C03\u7528\u7684 input/output tokens \u548C\u6210\u672C\u3002budgets \u8868\u652F\u6301\u6309\u7528\u6237\u6216\u6309\u6A21\u578B\u8BBE\u7F6E\u9884\u7B97\u4E0A\u9650\u3002"),
      spacer(),
      h2("ModelManager \u7C7B"),
      p("\u63D0\u4F9B\u6A21\u578B CRUD\u3001\u7528\u91CF\u8BB0\u5F55\u3001\u7528\u91CF\u7EDF\u8BA1\u67E5\u8BE2\u3001\u9884\u7B97\u63A7\u5236\u7B49\u65B9\u6CD5\u3002to_router_model_list() \u5C06\u6A21\u578B\u914D\u7F6E\u8F6C\u6362\u4E3A litellm.Router \u6240\u9700\u7684 model_list \u683C\u5F0F\u3002"),
      spacer(),
      h2("litellm \u56DE\u8C03"),
      p("UsageTrackerCallback \u7EE7\u627F CustomLogger\uFF0C\u5728 log_success_event \u4E2D\u4ECE kwargs \u83B7\u53D6 response_cost\uFF08litellm \u81EA\u52A8\u8BA1\u7B97\u7684\u6210\u672C\uFF09\u548C token \u7528\u91CF\uFF0C\u5199\u5165 SQLite\u3002\u590D\u7528 v1 \u7684\u5B9A\u4EF7\u8868\uFF08\u4ECE config_worker.yaml \u8FC1\u79FB\u5230 SQLite models \u8868\uFF09\u3002"),
      spacer(),
      pBold("\u590D\u7528 v1 \u4EE3\u7801\uFF1A"),
      bullet("\u6A21\u578B\u5B9A\u4EF7\u8868 \u2192 \u4ECE config_worker.yaml \u7684 input_cost_per_token/output_cost_per_token \u8FC1\u79FB\u5230 SQLite"),
      bullet("quota_setup.sh \u903B\u8F91 \u2192 ModelManager.set_budget()"),
      bullet("litellm_cli.py \u7684 add-model \u4EA4\u4E92\u903B\u8F91 \u2192 CLI Phase 6"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 2 ===
      h1("5. Phase 2: \u6838\u5FC3\u76EE\u68072 \u2014 \u667A\u80FD\u8C03\u7528\u89C4\u5212"),
      pBold("\u76EE\u6807\uFF1A\u6839\u636E\u8BF7\u6C42\u5185\u5BB9\u81EA\u52A8\u5206\u7C7B\uFF0C\u9009\u62E9\u6700\u4F18\u6A21\u578B\uFF0C\u5931\u8D25\u65F6\u81EA\u52A8 Fallback"),
      spacer(),
      h2("\u4E24\u5C42\u5206\u7C7B\u5668"),
      p("\u7B2C\u4E00\u5C42\u6B63\u5219\u89C4\u5219\u5339\u914D\uFF08\u96F6\u6210\u672C\u96F6\u5EF6\u8FDF\uFF09\uFF0C\u7B2C\u4E8C\u5C42 LLM \u5151\u5E95\u3002\u5206\u7C7B\u5668\u81EA\u52A8\u9009\u62E9\uFF1aDASHSCOPE_API_KEY \u6709\u503C \u2192 qwen3.6-flash\uFF08\u4E91\u7AEF\u5FEB\u901F\uFF09\uFF1B\u65E0\u503C \u2192 qwen2.5:latest\uFF08Ollama \u672C\u5730\uFF09\u3002"),
      spacer(),
      h2("Fallback \u94FE"),
      p("\u4ECE SQLite \u52A8\u6001\u8BFB\u53D6\u6A21\u578B\u914D\u7F6E\u6784\u5EFA Fallback \u94FE\uFF1acomplex_reasoning \u2192 general \u2192 simple_qa(\u672C\u5730)\uFF0C\u6240\u6709\u94FE\u6700\u7EC8 fallback \u5230 qwen2.5-local (Ollama)\u3002"),
      spacer(),
      h2("\u63A8\u7406\u6A21\u578B\u6D41\u5F0F"),
      p("\u68C0\u6D4B\u5230\u63A8\u7406\u6A21\u578B\uFF08qwen3.6-flash/plus\u3001deepseek-v3\uFF09\u65F6\u81EA\u52A8\u542F\u7528 stream=true\uFF0C\u907F\u514D reasoning tokens \u5BFC\u81F4 HTTP \u8D85\u65F6\u3002\u8FD9\u662F v1 \u4E2D\u8E29\u8FC7\u7684\u5751\u3002"),
      spacer(),
      pBold("\u590D\u7528 v1 \u4EE3\u7801\uFF1A"),
      bullet("TASK_MODEL_MAP \u6B63\u5219\u89C4\u5219 \u2192 RULES \u5B57\u5178"),
      bullet("INFERENCE_MODELS \u96C6\u5408 \u2192 \u76F4\u63A5\u590D\u7528"),
      bullet("async_pre_call_hook \u903B\u8F91 \u2192 route() \u65B9\u6CD5"),
      bullet("Fallback \u94FE \u2192 \u4ECE config_worker.yaml \u7684 fallbacks \u8FC1\u79FB"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 3 ===
      h1("6. Phase 3: \u6838\u5FC3\u76EE\u68073 \u2014 \u8BED\u4E49\u611F\u77E5\u4EFB\u52A1\u5206\u914D"),
      pBold("\u76EE\u6807\uFF1A\u68C0\u6D4B\u590D\u6742\u4EFB\u52A1\u81EA\u52A8\u5206\u89E3\uFF0C\u8BED\u4E49\u7F13\u5B58\u76F8\u4F3C\u8BF7\u6C42\uFF0C\u57DF\u5206\u7C7B\u589E\u5F3A\u8DEF\u7531"),
      spacer(),
      h2("\u8BED\u4E49\u7F13\u5B58"),
      p("\u4F7F\u7528 ChromaDB PersistentClient \u5B9E\u73B0\u672C\u5730\u5411\u91CF\u7F13\u5B58\u3002Embedding \u6765\u6E90\uFF1aDASHSCOPE_API_KEY \u6709\u503C \u2192 DashScope text-embedding-v3\uFF1B\u65E0\u503C \u2192 ChromaDB \u5185\u7F6E all-MiniLM-L6-v2\u3002\u7F13\u5B58\u7C92\u5EA6\u6309 (query, model) \u7EC4\u5408\u5B58\u50A8\uFF0CTTL 24h\u3002"),
      spacer(),
      h2("\u4EFB\u52A1\u7F16\u6392\u5668"),
      p("\u4E94\u6B65\u6D41\u7A0B\uFF1a\u590D\u6742\u5EA6\u68C0\u6D4B \u2192 \u4EFB\u52A1\u5206\u89E3 \u2192 \u6210\u672C\u89C4\u5212 \u2192 \u987A\u5E8F\u6267\u884C \u2192 \u7ED3\u679C\u6C47\u603B\u3002\u652F\u6301\u5BF9\u8BDD\u4E0A\u4E0B\u6587\uFF08\u6700\u8FD1 10 \u8F6E\uFF09\uFF0C\u5BF9 simple_qa/general \u7C7B\u578B\u542F\u7528\u8BED\u4E49\u7F13\u5B58\u3002\u6D41\u5F0F\u8F93\u51FA SSE \u4E8B\u4EF6\uFF08decompose/task_start/task_done/result\uFF09\u3002"),
      spacer(),
      h2("\u57DF\u5206\u7C7B"),
      p("\u590D\u7528 MMLU 14 \u7C7B\u5173\u952E\u8BCD\u9884\u8FC7\u6EE4 + LLM \u5151\u5E95\u903B\u8F91\u3002\u5206\u7C7B\u7ED3\u679C\u4F20\u5165 SmartRouter\uFF0CSTEM \u57DF \u2192 math_logic \u6A21\u578B\uFF0CCS \u57DF \u2192 coding \u6A21\u578B\u3002"),
      spacer(),
      pBold("\u590D\u7528 v1 \u4EE3\u7801\uFF1A"),
      bullet("task_orchestrator.py \u7684 is_complex() \u6B63\u5219 \u2192 \u76F4\u63A5\u590D\u7528"),
      bullet("decompose()/aggregate() \u7684 prompt \u2192 \u76F4\u63A5\u590D\u7528\uFF0curllib \u2192 litellm SDK"),
      bullet("SubTask dataclass \u2192 \u76F4\u63A5\u590D\u7528"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 4 ===
      h1("7. Phase 4: \u5B89\u5168\u5C42"),
      pBold("\u76EE\u6807\uFF1aPII \u8131\u654F + \u8D8A\u72F1\u62E6\u622A + \u57DF\u5206\u7C7B\uFF0C\u7EAF\u51FD\u6570\u5B9E\u73B0\uFF0C\u65E0 Flask \u4F9D\u8D56"),
      spacer(),
      makeTable(
        ["\u68C0\u6D4B", "\u5B9E\u73B0\u65B9\u5F0F", "\u52A8\u4F5C"],
        [
          ["PII \u8131\u654F", "\u6B63\u5219\uFF087 \u7C7B\uFF1a\u90AE\u7BB1/\u624B\u673A/\u8EAB\u4EFD\u8BC1/\u94F6\u884C\u5361\u7B49\uFF09", "\u66FF\u6362\u4E3A [MASKED]"],
          ["\u8D8A\u72F1\u62E6\u622A", "\u6A21\u5F0F\u5339\u914D\uFF085 \u7C7B\uFF1aDAN/\u6307\u4EE4\u8986\u76D6/\u89D2\u8272\u64CD\u7EB5\u7B49\uFF09", "\u62D2\u7EDD\u8BF7\u6C42"],
          ["\u57DF\u5206\u7C7B", "\u5173\u952E\u8BCD\u9884\u8FC7\u6EE4 + LLM \u5151\u5E95", "\u6CE8\u5165\u8DEF\u7531\u589E\u5F3A"]
        ],
        [2000, 5000, 2360]
      ),
      spacer(),
      p("\u5B89\u5168\u68C0\u6D4B\u5728 SmartRouter.route() \u8C03\u7528\u524D\u6267\u884C\uFF0C\u4E0D\u5355\u72EC\u5F00\u670D\u52A1\u3002\u6B63\u5219\u4E2D\u4E2D\u6587\u7528 (?<!\\d)/(?!\\d) \u66FF\u4EE3 \\b\uFF08v1 \u8E29\u8FC7\u7684\u5751\uFF09\u3002"),
      spacer(),
      pBold("\u590D\u7528 v1 \u4EE3\u7801\uFF1A"),
      bullet("semantic_router/app.py \u7684\u5168\u90E8\u6B63\u5219\u89C4\u5219 \u2192 \u76F4\u63A5\u590D\u7528"),
      bullet("MMLU 14 \u7C7B\u5173\u952E\u8BCD\u8868 \u2192 \u76F4\u63A5\u590D\u7528"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 5 ===
      h1("8. Phase 5: API \u670D\u52A1\u5C42"),
      pBold("\u76EE\u6807\uFF1aFastAPI \u63D0\u4F9B REST + SSE \u7AEF\u70B9\uFF0C\u4E3A Tauri \u548C CLI \u63D0\u4F9B\u7EDF\u4E00\u63A5\u53E3"),
      spacer(),
      makeTable(
        ["\u7AEF\u70B9", "\u65B9\u6CD5", "\u529F\u80FD", "\u4F9D\u8D56"],
        [
          ["/api/health", "GET", "\u670D\u52A1\u5065\u5EB7\u68C0\u67E5 + \u6A21\u578B\u53EF\u7528\u6027", "Phase 0"],
          ["/api/models", "GET/POST", "\u5217\u51FA/\u6CE8\u518C\u6A21\u578B", "Phase 1"],
          ["/api/models/{name}", "DELETE", "\u5220\u9664\u6A21\u578B", "Phase 1"],
          ["/api/usage", "GET", "\u7528\u91CF\u7EDF\u8BA1\uFF08\u6309\u6A21\u578B/\u65F6\u95F4/\u7528\u6237\uFF09", "Phase 1"],
          ["/api/budget", "GET/POST", "\u67E5\u8BE2/\u8BBE\u7F6E\u9884\u7B97", "Phase 1"],
          ["/api/chat", "POST", "\u666E\u901A\u5BF9\u8BDD\uFF08SmartRouter \u8DEF\u7531\uFF09", "Phase 2"],
          ["/api/chat/stream", "POST", "SSE \u6D41\u5F0F\u5BF9\u8BDD", "Phase 2"],
          ["/api/orchestrate/stream", "POST", "SSE \u6D41\u5F0F\u7F16\u6392", "Phase 3"],
          ["/api/conversations", "GET/POST/DELETE", "\u5BF9\u8BDD\u5386\u53F2 CRUD", "Phase 3"],
          ["/api/stats", "GET", "\u7EFC\u5408\u72B6\u6001\uFF08Tauri \u603B\u89C8\u9875\u7528\uFF09", "1,2,3"],
          ["/api/config", "GET/POST", ".env \u914D\u7F6E\u8BFB\u5199", "Phase 0"],
          ["/api/cache/stats", "GET", "\u7F13\u5B58\u547D\u4E2D\u7387\u7EDF\u8BA1", "Phase 3"],
          ["/api/cache/clear", "POST", "\u6E05\u7A7A\u7F13\u5B58", "Phase 3"]
        ],
        [2800, 1200, 3600, 1760]
      ),
      spacer(),
      p("SSE \u54CD\u5E94\u5FC5\u987B\u6307\u5B9A charset=utf-8\uFF08v1 \u8E29\u5751\u8BB0\u5F55\uFF09\u3002"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 6 ===
      h1("9. Phase 6: CLI \u5DE5\u5177"),
      pBold("\u76EE\u6807\uFF1a\u63D0\u4F9B\u547D\u4EE4\u884C\u64CD\u4F5C\uFF0C\u590D\u73B0\u6838\u5FC3\u529F\u80FD"),
      spacer(),
      bullet("lloom init \u2014 \u4EA4\u4E92\u5F0F\u521D\u59CB\u5316\uFF08API Key \u6536\u96C6 + Ollama \u6A21\u578B\u62C9\u53D6\uFF09"),
      bullet("lloom serve \u2014 \u542F\u52A8 API \u670D\u52A1 (:7860)"),
      bullet("lloom model add/remove/list \u2014 \u6A21\u578B\u7BA1\u7406"),
      bullet("lloom status \u2014 \u7528\u91CF/\u8DEF\u7531/\u7F13\u5B58\u7EDF\u8BA1"),
      bullet("lloom chat \u2014 \u547D\u4EE4\u884C\u5BF9\u8BDD"),
      bullet("lloom orchestrate \u2014 \u590D\u6742\u4EFB\u52A1\u7F16\u6392"),
      spacer(),
      p("\u7528 click \u6846\u67B6\u3002CLI \u76F4\u63A5\u8C03\u7528 core \u5C42 Python \u5305\uFF0C\u4E0D\u9700\u8981 API \u670D\u52A1\u8FD0\u884C\u3002init \u5411\u5BFC\u590D\u7528 v1 litellm_cli.py \u7684 7 \u6B65\u903B\u8F91\uFF08\u7CBE\u7B80\u53BB\u6389 Docker \u68C0\u6D4B\uFF09\u3002"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 7 ===
      h1("10. Phase 7: Tauri GUI \u4E0E\u8FDB\u7A0B\u7BA1\u7406"),
      pBold("\u76EE\u6807\uFF1a\u9002\u914D Tauri \u684C\u9762\u5E94\u7528\uFF0C\u4ECE Docker \u547D\u4EE4\u5207\u6362\u4E3A API \u8C03\u7528 + \u5B50\u8FDB\u7A0B\u7BA1\u7406"),
      spacer(),
      h2("\u8FDB\u7A0B\u7BA1\u7406"),
      p("Tauri \u542F\u52A8\u65F6\u81EA\u52A8\u62C9\u8D77 Ollama \u5B50\u8FDB\u7A0B\uFF08resources/ollama serve\uFF09\u548C Python API \u5B50\u8FDB\u7A0B\uFF08resources/lloom-server --port 7860\uFF09\uFF0C\u8F6E\u8BE2\u7AEF\u53E3\u5C31\u7EEA\u540E\u52A0\u8F7D UI\u3002\u9000\u51FA\u65F6 kill \u4E24\u4E2A\u5B50\u8FDB\u7A0B\u3002"),
      spacer(),
      h2("\u9875\u9762\u9002\u914D"),
      makeTable(
        ["\u9875\u9762", "v1 \u65B9\u5F0F", "v2 \u65B9\u5F0F"],
        [
          ["\u603B\u89C8", "docker compose ps", "GET /api/health + GET /api/stats"],
          ["\u7528\u91CF", "Prometheus \u67E5\u8BE2", "GET /api/usage + SVG \u56FE\u8868"],
          ["\u5BF9\u8BDD", "SSE \u2192 webapp:3002", "SSE \u2192 localhost:7860/api/orchestrate/stream"],
          ["\u6A21\u578B", "CLI JSON \u547D\u4EE4", "GET/POST/DELETE /api/models"],
          ["\u8BBE\u7F6E", ".env \u6587\u4EF6\u8BFB\u5199", "GET/POST /api/config + smart restart"]
        ],
        [1500, 3680, 4180]
      ),
      spacer(),
      pBold("\u590D\u7528 v1 \u4EE3\u7801\uFF1A"),
      bullet("index.html \u6574\u4F53\u7ED3\u6784 \u2192 \u590D\u7528\uFF085 \u9875\u9762\u5E03\u5C40\u3001SVG \u56FE\u8868\u3001Markdown \u6E32\u67D3\u3001\u5BF9\u8BDD\u5386\u53F2\uFF09"),
      bullet("chat_request SSE \u4EE3\u7406\u547D\u4EE4 \u2192 \u590D\u7528\uFF08URL \u6539\u4E3A :7860\uFF09"),
      bullet("read_env/write_env \u2192 \u6539\u4E3A\u8C03\u7528 API"),
      bullet("PATH \u6CE8\u5165\u903B\u8F91 \u2192 \u4FDD\u7559\uFF08\u53EF\u80FD\u9700\u8981 ollama \u4E8C\u8FDB\u5236\u8DEF\u5F84\uFF09"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 8 ===
      h1("11. Phase 8: \u6253\u5305\u6784\u5EFA"),
      pBold("\u76EE\u6807\uFF1a\u751F\u6210\u5B8C\u5168\u81EA\u5305\u542B\u7684 .app"),
      spacer(),
      numbered("PyInstaller \u6253\u5305 Python \u6838\u5FC3\uFF1apyinstaller --onecore --name lloom-server \u914D\u7F6E hiddenimports\uFF08litellm/chromadb/uvicorn\uFF09", "num1"),
      numbered("\u4E0B\u8F7D Ollama macOS ARM64 \u4E8C\u8FDB\u5236\uFF0C\u653E\u5165 tauri-app/src-tauri/resources/ollama", "num1"),
      numbered("\u914D\u7F6E tauri.conf.json bundle.resources \u5305\u542B ollama \u548C lloom-server", "num1"),
      numbered("\u9996\u6B21\u8FD0\u884C\u65F6\u68C0\u67E5\u5E76\u63D0\u793A\u62C9\u53D6 qwen2.5:latest \u6A21\u578B\uFF08\u7EA6 4GB\uFF09", "num1"),
      numbered("cargo tauri build \u751F\u6210 LLooM.app", "num1"),
      spacer(),
      pBold("\u5173\u952E\u9A8C\u8BC1\u70B9\uFF1A"),
      bullet("\u8FD0\u884C ./dist/lloom-server --port 7860\uFF0C\u786E\u8BA4 API \u53EF\u7528"),
      bullet("litellm \u7684\u52A8\u6001\u5BFC\u5165\u53EF\u80FD\u9700\u8981\u591A\u6B21\u8C03\u6574 hiddenimports"),
      new Paragraph({ children: [new PageBreak()] }),

      // === Phase 9 ===
      h1("12. Phase 9: \u96C6\u6210\u6D4B\u8BD5\u4E0E\u53D1\u5E03"),
      spacer(),
      h2("\u7AEF\u5230\u7AEF\u6D4B\u8BD5"),
      bullet("\u5168\u65B0\u673A\u5668\u4E0A\u53CC\u51FB .app \u2192 \u9996\u6B21\u914D\u7F6E \u2192 \u5BF9\u8BDD \u2192 \u67E5\u770B\u7528\u91CF"),
      bullet("\u667A\u80FD\u8DEF\u7531\u9A8C\u8BC1\uFF08\u7B80\u5355\u95EE\u9898 \u2192 \u672C\u5730\u6A21\u578B\uFF0C\u7F16\u7801\u95EE\u9898 \u2192 deepseek-v3\uFF09"),
      bullet("\u8BED\u4E49\u7F13\u5B58\u9A8C\u8BC1\uFF08\u91CD\u590D\u95EE\u9898\u547D\u4E2D\u7F13\u5B58\uFF09"),
      bullet("\u590D\u6742\u4EFB\u52A1\u7F16\u6392\u9A8C\u8BC1\uFF08\u591A\u6B65\u9AA4\u5206\u89E3 \u2192 \u6267\u884C \u2192 \u6C47\u603B\uFF09"),
      bullet("\u9884\u7B97\u63A7\u5236\u9A8C\u8BC1\uFF08\u8D85\u9650\u540E\u62D2\u7EDD\u8BF7\u6C42\uFF09"),
      spacer(),
      h2("\u6587\u6863\u66F4\u65B0"),
      bullet("README.md\uFF1a\u5B89\u88C5\u65B9\u5F0F\u4ECE Docker \u6539\u4E3A\u201C\u4E0B\u8F7D .app \u53CC\u51FB\u8FD0\u884C\u201D"),
      bullet("\u9879\u76EE\u6587\u6863.md\uFF1a\u67B6\u6784\u56FE\u3001\u6838\u5FC3\u6A21\u5757\u8BF4\u660E\u3001\u6253\u5305\u6D41\u7A0B"),
      bullet("progress.md\uFF1av2 \u91CD\u6784\u8FDB\u5C55"),
      spacer(),
      h2("GitHub Release"),
      bullet("\u538B\u7F29 LLooM.app \u4E3A zip"),
      bullet("\u5728 v2 \u5206\u652F\u5408\u5E76\u5230 main \u540E\uFF0C\u521B\u5EFA Release\u300Atag v2.0.0"),
      bullet("Release \u63CF\u8FF0\uFF1a\u67B6\u6784\u53D8\u66F4\u8BF4\u660E + \u5B89\u88C5\u6B65\u9AA4 + \u622A\u56FE"),
      new Paragraph({ children: [new PageBreak()] }),

      // === 13. 开发顺序 ===
      h1("13. \u5F00\u53D1\u987A\u5E8F\u4E0E\u4F9D\u8D56\u5173\u7CFB"),
      spacer(),
      makeTable(
        ["Phase", "\u5185\u5BB9", "\u4F9D\u8D56", "\u9884\u4F30\u65F6\u95F4"],
        [
          ["Phase 0", "\u9879\u76EE\u9AA8\u67B6 + \u6253\u5305\u9A8C\u8BC1", "\u65E0", "1-2 \u5929"],
          ["Phase 1", "ModelManager", "Phase 0", "2-3 \u5929"],
          ["Phase 2", "SmartRouter", "Phase 1", "2-3 \u5929"],
          ["Phase 3", "Orchestrator + Cache", "Phase 2", "3-4 \u5929"],
          ["Phase 4", "\u5B89\u5168\u5C42", "Phase 2\uFF08\u53EF\u5E76\u884C\uFF09", "1-2 \u5929"],
          ["Phase 5", "API \u670D\u52A1\u5C42", "Phase 1,2,3", "2-3 \u5929"],
          ["Phase 6", "CLI \u5DE5\u5177", "Phase 1\uFF08\u53EF\u5E76\u884C\uFF09", "1-2 \u5929"],
          ["Phase 7", "Tauri GUI", "Phase 5", "3-5 \u5929"],
          ["Phase 8", "\u6253\u5305\u6784\u5EFA", "Phase 7", "2-3 \u5929"],
          ["Phase 9", "\u6D4B\u8BD5 + \u53D1\u5E03", "Phase 8", "2-3 \u5929"]
        ],
        [1200, 3000, 3000, 2160]
      ),
      spacer(),
      pBold("\u53EF\u5E76\u884C\u7684\u65F6\u95F4\u6BB5\uFF1A"),
      bullet("Phase 4\uFF08\u5B89\u5168\u5C42\uFF09\u4E0E Phase 3\uFF08\u7F16\u6392\u5668\uFF09\u53EF\u540C\u65F6\u5F00\u53D1"),
      bullet("Phase 6\uFF08CLI\uFF09\u4E0E Phase 5\uFF08API\uFF09\u53EF\u540C\u65F6\u5F00\u53D1"),
      bullet("Phase 7\uFF08Tauri\uFF09\u5728 Phase 5 \u5B8C\u6210\u540E\u624D\u5F00\u59CB"),
      new Paragraph({ children: [new PageBreak()] }),

      // === 14. 迁移清单 ===
      h1("14. v1 \u2192 v2 \u4EE3\u7801\u8FC1\u79FB\u6E05\u5355"),
      spacer(),
      makeTable(
        ["v1 \u6587\u4EF6", "v2 \u5F52\u5C5E", "\u8FC1\u79FB\u65B9\u5F0F"],
        [
          ["custom_callbacks.py \u6B63\u5219\u89C4\u5219", "core/smart_router.py RULES", "\u76F4\u63A5\u590D\u5236"],
          ["custom_callbacks.py INFERENCE_MODELS", "core/smart_router.py", "\u76F4\u63A5\u590D\u5236"],
          ["custom_callbacks.py QuotaTracker", "core/callbacks.py", "Prometheus \u2192 SQLite"],
          ["task_orchestrator.py is_complex()", "core/orchestrator.py", "\u76F4\u63A5\u590D\u5236"],
          ["task_orchestrator.py decompose() prompt", "core/orchestrator.py", "\u76F4\u63A5\u590D\u5236"],
          ["task_orchestrator.py aggregate() prompt", "core/orchestrator.py", "\u76F4\u63A5\u590D\u5236"],
          ["semantic_router/app.py PII \u6B63\u5219", "core/security.py", "\u76F4\u63A5\u590D\u5236"],
          ["semantic_router/app.py \u8D8A\u72F1\u6B63\u5219", "core/security.py", "\u76F4\u63A5\u590D\u5236"],
          ["semantic_router/app.py MMLU \u5173\u952E\u8BCD", "core/security.py", "\u76F4\u63A5\u590D\u5236"],
          ["config_worker.yaml model_list", "SQLite models \u8868", "\u8FC1\u79FB\u6570\u636E"],
          ["config_worker.yaml fallbacks", "SmartRouter._build_fallbacks()", "\u8FC1\u79FB\u903B\u8F91"],
          ["litellm_cli.py init \u5411\u5BFC", "cli/lloom.py init", "\u7CBE\u7B80\uFF08\u53BB Docker\uFF09"],
          ["webapp/app.py API \u7AEF\u70B9", "api/server.py", "Flask\u2192FastAPI \u91CD\u5199"],
          ["tauri-app main.rs \u8FDB\u7A0B\u7BA1\u7406", "main.rs", "Docker\u2192\u5B50\u8FDB\u7A0B \u91CD\u5199"],
          ["tauri-app index.html UI", "index.html", "\u6539 API URL\uFF0C\u4FDD\u7559\u5E03\u5C40"]
        ],
        [3500, 3000, 2860]
      ),
    ]
  }]
});

Packer.toBuffer(doc).then(buffer => {
  fs.writeFileSync("/Users/orange/LLooMv2/LLooM-v2-重构计划.docx", buffer);
  console.log("DOCX generated successfully");
});
