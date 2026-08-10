#!/usr/bin/env python3
"""
Semantic Router Service — vLLM Semantic Router Integration

Following the vLLM Semantic Router architecture:
  1. Signal extraction (keyword, domain)
  2. Decision engine (AND/OR rules)
  3. Plugin chain (PII, jailbreak, hallucination, system_prompt)

Sits in front of LiteLLM Worker as a security + classification proxy.
  Client → Semantic Router (8888) → LiteLLM Worker (4001) → Model providers

Plugins:
  - PII detection (regex-based, 7 types)
  - Jailbreak detection (pattern-based, 5 attack categories)
  - Domain classification (MMLU 14 categories, LLM-based)
  - Hallucination detection (LLM-based, optional)
"""

import os
import re
import json
import time
import logging
import yaml
import requests
from typing import Optional
from flask import Flask, request, Response, jsonify, stream_with_context

# ==================================================
# Logging
# ==================================================
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s [SR] %(message)s",
)
logger = logging.getLogger("semantic_router")

# ==================================================
# Configuration
# ==================================================
INSTALL_DIR = os.path.dirname(os.path.abspath(__file__))
CONFIG_PATH = os.path.join(INSTALL_DIR, "config.yaml")

with open(CONFIG_PATH, "r", encoding="utf-8") as f:
    CONFIG = yaml.safe_load(f)

BACKEND_URL = CONFIG.get("backend", {}).get("url", "http://litellm-worker:4000")
BACKEND_KEY = os.getenv(CONFIG.get("backend", {}).get("api_key_env", "LITELLM_MASTER_KEY"), "sk-1234")

PII_ENABLED = CONFIG.get("pii", {}).get("enabled", True)
PII_ACTION = CONFIG.get("pii", {}).get("action", "mask")

JAILBREAK_ENABLED = CONFIG.get("jailbreak", {}).get("enabled", True)
JAILBREAK_ACTION = CONFIG.get("jailbreak", {}).get("action", "block")

HALLUCINATION_ENABLED = CONFIG.get("hallucination", {}).get("enabled", False)
HALLUCINATION_ACTION = CONFIG.get("hallucination", {}).get("action", "header")

DOMAIN_ENABLED = CONFIG.get("classifier", {}).get("domain", {}).get("enabled", True)
DOMAIN_TIMEOUT = CONFIG.get("classifier", {}).get("domain", {}).get("timeout", 10)
DOMAIN_MAX_TOKENS = CONFIG.get("classifier", {}).get("domain", {}).get("max_tokens", 20)

MMLU_CATEGORIES = CONFIG.get("mmlu_categories", [
    "math", "physics", "chemistry", "biology", "computer_science",
    "engineering", "business", "law", "economics", "health",
    "psychology", "philosophy", "history", "other"
])

# LLM classifier auto-selection (same strategy as custom_callbacks.py)
DASHSCOPE_API_KEY = os.getenv("DASHSCOPE_API_KEY", "")
if DASHSCOPE_API_KEY:
    CLASSIFIER_API_BASE = os.getenv("DASHSCOPE_API_BASE", "https://dashscope.aliyuncs.com/compatible-mode/v1")
    CLASSIFIER_API_KEY = DASHSCOPE_API_KEY
    CLASSIFIER_MODEL = "qwen3.6-flash"
else:
    CLASSIFIER_API_BASE = os.getenv("OLLAMA_API_BASE", "http://host.docker.internal:11434") + "/v1"
    CLASSIFIER_API_KEY = "ollama"
    CLASSIFIER_MODEL = "qwen2.5:latest"

# Statistics
STATS = {
    "requests_total": 0,
    "pii_detected": 0,
    "pii_blocked": 0,
    "pii_masked": 0,
    "jailbreak_detected": 0,
    "jailbreak_blocked": 0,
    "hallucination_detected": 0,
    "domain_classifications": {},
}

app = Flask(__name__)


# ==================================================
# Plugin 1: PII Detection
# ==================================================

PII_PATTERNS = {
    "EMAIL_ADDRESS": re.compile(
        r'(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}(?![A-Za-z])',
        re.IGNORECASE
    ),
    "PHONE_NUMBER": re.compile(
        r'(?<!\d)(?:1[3-9]\d{9})(?!\d)'  # Chinese mobile (strict 11 digits)
        r'|(?<!\d)(?:\+?1\s*(?:[.-]\s*)?(?:\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}))(?!\d)',  # US
        re.IGNORECASE
    ),
    "US_SSN": re.compile(r'(?<!\d)\d{3}-\d{2}-\d{4}(?!\d)'),
    "CREDIT_CARD": re.compile(
        r'(?<!\d)(?:\d[ -]*?){13,16}(?!\d)'
    ),
    "IP_ADDRESS": re.compile(
        r'(?<!\d)(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)(?!\d)'
    ),
    "ID_CARD": re.compile(  # Chinese ID card (18 digits)
        r'(?<!\d)\d{17}[\dXx](?!\d)'
    ),
    "BANK_ACCOUNT": re.compile(  # IBAN
        r'(?<![A-Z])[A-Z]{2}\d{2}[A-Z0-9]{10,30}(?![A-Z0-9])', re.IGNORECASE
    ),
}

PII_MASKS = {
    "EMAIL_ADDRESS": "[EMAIL]",
    "PHONE_NUMBER": "[PHONE]",
    "US_SSN": "[SSN]",
    "CREDIT_CARD": "[CARD]",
    "IP_ADDRESS": "[IP]",
    "ID_CARD": "[ID]",
    "BANK_ACCOUNT": "[BANK]",
}


def detect_pii(text: str) -> dict:
    """Detect PII in text. Returns dict of {type: [matches]}."""
    findings = {}
    for pii_type, pattern in PII_PATTERNS.items():
        matches = pattern.findall(text)
        if matches:
            findings[pii_type] = matches
    return findings


def mask_pii(text: str, findings: dict) -> str:
    """Mask PII in text."""
    for pii_type, matches in findings.items():
        mask = PII_MASKS.get(pii_type, "[REDACTED]")
        for match in matches:
            if isinstance(match, tuple):
                match = match[0] if match[0] else (match[1] if len(match) > 1 else "")
            text = text.replace(str(match), mask)
    return text


def process_pii(text: str) -> tuple[str, dict]:
    """
    Process PII in text based on configured action.
    Returns (processed_text, report)
    """
    if not PII_ENABLED:
        return text, {}

    findings = detect_pii(text)
    if not findings:
        return text, {}

    STATS["pii_detected"] += 1
    report = {
        "detected": {k: len(v) for k, v in findings.items()},
        "action": PII_ACTION,
    }

    if PII_ACTION == "block":
        STATS["pii_blocked"] += 1
        report["blocked"] = True
        return None, report
    elif PII_ACTION == "mask":
        STATS["pii_masked"] += 1
        masked = mask_pii(text, findings)
        return masked, report
    else:  # warn
        return text, report


# ==================================================
# Plugin 2: Jailbreak Detection
# ==================================================

JAILBREAK_PATTERNS = [
    # DAN attacks
    (re.compile(r'you\s+are\s+(now\s+)?(?:DAN|do\s+anything\s+now)', re.IGNORECASE), "DAN_ATTACK"),
    # Instruction override
    (re.compile(r'ignore\s+(?:previous|prior|above|all)\s+(?:instructions?|prompts?|rules?|guidelines?)', re.IGNORECASE), "INSTRUCTION_OVERRIDE"),
    (re.compile(r'disregard\s+(?:previous|prior|above|all)\s+(?:instructions?|prompts?)', re.IGNORECASE), "INSTRUCTION_OVERRIDE"),
    (re.compile(r'forget\s+(?:everything|all|previous)', re.IGNORECASE), "INSTRUCTION_OVERRIDE"),
    # Role manipulation
    (re.compile(r'(?:pretend|act\s+as|roleplay)\s+(?:you\s+are\s+)?(?:a|an)?\s*(?:different|unrestricted|unfiltered|unlimited|free|evil|hacker)', re.IGNORECASE), "ROLE_MANIPULATION"),
    (re.compile(r'you\s+are\s+now\s+(?:in\s+)?(?:developer|root|admin|god|unlimited)\s+mode', re.IGNORECASE), "ROLE_MANIPULATION"),
    # Safety bypass
    (re.compile(r'(?:no|without|remove|disable)\s+(?:safety|restrictions?|guidelines?|rules?|limits?|filters?|guardrails?)', re.IGNORECASE), "SAFETY_BYPASS"),
    (re.compile(r'(?:bypass|circumvent|override)\s+(?:safety|security|content\s+filter)', re.IGNORECASE), "SAFETY_BYPASS"),
    # Prompt injection
    (re.compile(r'(?:system|admin|developer)\s+prompt\s*(?:is|:|=\s*)', re.IGNORECASE), "PROMPT_INJECTION"),
    (re.compile(r'reveal|show|print|output\s+(?:your|the)\s+(?:system\s+)?prompt', re.IGNORECASE), "PROMPT_INJECTION"),
    # Jailbreak keywords
    (re.compile(r'\bjailbreak\b', re.IGNORECASE), "JAILBREAK_KEYWORD"),
]


def detect_jailbreak(text: str) -> dict:
    """Detect jailbreak attempts. Returns report dict."""
    detections = []
    for pattern, attack_type in JAILBREAK_PATTERNS:
        matches = pattern.findall(text)
        if matches:
            detections.append({
                "type": attack_type,
                "count": len(matches),
                "sample": matches[0][:100] if matches else "",
            })
    return {"detected": detections} if detections else {}


def process_jailbreak(text: str) -> tuple[bool, dict]:
    """
    Process jailbreak detection.
    Returns (should_block, report)
    """
    if not JAILBREAK_ENABLED:
        return False, {}

    report = detect_jailbreak(text)
    if not report:
        return False, {}

    STATS["jailbreak_detected"] += 1
    report["action"] = JAILBREAK_ACTION

    if JAILBREAK_ACTION == "block":
        STATS["jailbreak_blocked"] += 1
        return True, report
    else:  # warn
        return False, report


# ==================================================
# Plugin 3: Domain Classification (MMLU 14)
# ==================================================

# Keyword signals for each MMLU domain (fast first pass)
DOMAIN_KEYWORDS = {
    "math": ["calculate", "equation", "solve", "derivative", "integral", "algebra",
             "geometry", "calculus", "probability", "statistics", "theorem", "matrix",
             "数学", "计算", "方程", "微积分", "概率", "统计", "矩阵", "几何"],
    "physics": ["physics", "mechanics", "thermodynamics", "electromagnetism", "quantum",
                "relativity", "force", "energy", "velocity", "acceleration", "wavelength",
                "物理", "力学", "热力学", "电磁", "量子", "相对论", "速度", "加速度"],
    "chemistry": ["chemistry", "molecule", "reaction", "compound", "element", "bond",
                  "organic", "inorganic", "acid", "base", "oxidation", "catalyst",
                  "化学", "分子", "反应", "化合物", "元素", "催化剂", "氧化"],
    "biology": ["biology", "cell", "organism", "evolution", "genetics", "DNA", "RNA",
                "protein", "enzyme", "photosynthesis", "mitosis", "ecosystem",
                "生物", "细胞", "进化", "遗传", "蛋白质", "酶", "光合作用"],
    "computer_science": ["programming", "code", "algorithm", "data structure", "software",
                         "database", "network", "compiler", "operating system", "API",
                         "编程", "代码", "算法", "数据结构", "软件", "数据库", "网络"],
    "engineering": ["engineering", "circuit", "mechanical", "structural", "control system",
                    "signal processing", "embedded", "CAD", "manufacturing",
                    "工程", "电路", "机械", "结构", "制造", "嵌入式"],
    "business": ["business", "management", "marketing", "strategy", "finance",
                 "investment", "revenue", "profit", "startup", "entrepreneur",
                 "商业", "管理", "营销", "战略", "金融", "投资", "利润", "创业"],
    "law": ["law", "legal", "court", "contract", "regulation", "compliance",
            "statute", "liability", "intellectual property", "copyright",
            "法律", "法院", "合同", "法规", "合规", "知识产权", "版权"],
    "economics": ["economics", "market", "supply", "demand", "GDP", "inflation",
                  "trade", "fiscal", "monetary", "macroeconomics", "microeconomics",
                  "经济", "市场", "供给", "需求", "通胀", "贸易", "财政"],
    "health": ["health", "medical", "disease", "symptom", "treatment", "medicine",
               "patient", "diagnosis", "therapy", "drug", "vaccine",
               "健康", "医疗", "疾病", "症状", "治疗", "药物", "疫苗", "诊断"],
    "psychology": ["psychology", "behavior", "cognitive", "emotion", "mental",
                   "therapy", "personality", "consciousness", "perception",
                   "心理", "行为", "认知", "情绪", "意识", "感知"],
    "philosophy": ["philosophy", "ethics", "metaphysics", "existence", "consciousness",
                   "morality", "nihilism", "existentialism", "rationalism",
                   "哲学", "伦理", "道德", "存在", "形而上学"],
    "history": ["history", "historical", "ancient", "medieval", "modern", "century",
                "revolution", "empire", "dynasty", "war", "civilization",
                "历史", "古代", "中世纪", "现代", "革命", "帝国", "朝代"],
}

DOMAIN_SYSTEM_PROMPT = """You are a domain classifier. Classify the user's query into exactly ONE of the following 14 MMLU categories. Return ONLY the category name, nothing else:

- math: Mathematics, algebra, calculus, statistics, probability
- physics: Physics, mechanics, thermodynamics, electromagnetism
- chemistry: Chemistry, molecules, reactions, chemical compounds
- biology: Biology, cells, genetics, evolution, organisms
- computer_science: Programming, algorithms, software, databases, networks
- engineering: Engineering, circuits, mechanical design, control systems
- business: Business, management, marketing, finance, strategy
- law: Law, legal, contracts, regulations, compliance
- economics: Economics, markets, trade, GDP, inflation
- health: Health, medicine, diseases, symptoms, treatments
- psychology: Psychology, behavior, cognition, emotions, mental health
- philosophy: Philosophy, ethics, metaphysics, morality
- history: History, historical events, ancient civilizations, wars
- other: General queries not fitting the above categories"""


def _keyword_classify_domain(text: str) -> Optional[str]:
    """Fast keyword-based domain classification (zero cost)."""
    text_lower = text.lower()
    scores = {}
    for domain, keywords in DOMAIN_KEYWORDS.items():
        score = sum(1 for kw in keywords if kw.lower() in text_lower)
        if score > 0:
            scores[domain] = score
    if scores:
        return max(scores, key=scores.get)
    return None


def _llm_classify_domain(text: str) -> str:
    """LLM-based domain classification (fallback)."""
    truncated = text[:500]
    try:
        resp = requests.post(
            f"{CLASSIFIER_API_BASE}/chat/completions",
            headers={
                "Authorization": f"Bearer {CLASSIFIER_API_KEY}",
                "Content-Type": "application/json",
            },
            json={
                "model": CLASSIFIER_MODEL,
                "messages": [
                    {"role": "system", "content": DOMAIN_SYSTEM_PROMPT},
                    {"role": "user", "content": truncated},
                ],
                "max_tokens": DOMAIN_MAX_TOKENS,
                "temperature": 0,
            },
            timeout=DOMAIN_TIMEOUT,
        )
        resp.raise_for_status()
        result = resp.json()["choices"][0]["message"]["content"].strip().lower()
        if result in MMLU_CATEGORIES:
            return result
        for cat in MMLU_CATEGORIES:
            if cat in result:
                return cat
        return "other"
    except Exception as e:
        logger.warning("Domain LLM classification failed: %s, fallback to 'other'", e)
        return "other"


def classify_domain(text: str) -> tuple[str, str]:
    """
    Classify text into MMLU 14 categories.
    Returns (domain, method) where method is 'keyword' or 'llm'
    """
    if not DOMAIN_ENABLED:
        return "other", "disabled"

    # First pass: keyword matching (fast, zero cost)
    domain = _keyword_classify_domain(text)
    if domain:
        method = "keyword"
    else:
        # Second pass: LLM classification
        domain = _llm_classify_domain(text)
        method = "llm"

    STATS["domain_classifications"][domain] = STATS["domain_classifications"].get(domain, 0) + 1
    return domain, method


# ==================================================
# Plugin 4: Hallucination Detection
# ==================================================

HALLUCINATION_PROMPT = """Analyze the following LLM response for potential hallucinations. Check if the response contains:
1. Contradictory statements
2. Unsupported factual claims
3. Fabricated information presented as fact

Return a JSON object: {"hallucination_detected": true/false, "confidence": 0.0-1.0, "reason": "brief explanation"}

User query: {query}
LLM response: {response}"""


def detect_hallucination(query: str, response: str) -> dict:
    """Detect hallucination in LLM response (LLM-based)."""
    if not HALLUCINATION_ENABLED:
        return {}

    try:
        truncated_resp = response[:2000]
        prompt = HALLUCINATION_PROMPT.format(query=query[:500], response=truncated_resp)
        resp = requests.post(
            f"{CLASSIFIER_API_BASE}/chat/completions",
            headers={
                "Authorization": f"Bearer {CLASSIFIER_API_KEY}",
                "Content-Type": "application/json",
            },
            json={
                "model": CLASSIFIER_MODEL,
                "messages": [{"role": "user", "content": prompt}],
                "max_tokens": 100,
                "temperature": 0,
            },
            timeout=15,
        )
        resp.raise_for_status()
        result_text = resp.json()["choices"][0]["message"]["content"].strip()
        # Try to parse JSON
        if result_text.startswith("{"):
            result = json.loads(result_text)
        else:
            result = {"hallucination_detected": False, "confidence": 0.0, "reason": "parse_error"}

        if result.get("hallucination_detected"):
            STATS["hallucination_detected"] += 1

        return result
    except Exception as e:
        logger.warning("Hallucination detection failed: %s", e)
        return {}


# ==================================================
# Proxy Logic
# ==================================================

def extract_user_text(messages: list) -> str:
    """Extract user message text from chat completion request."""
    parts = []
    for msg in messages:
        if msg.get("role") == "user":
            content = msg.get("content", "")
            if isinstance(content, list):
                for part in content:
                    if isinstance(part, dict) and part.get("type") == "text":
                        parts.append(part.get("text", ""))
            elif isinstance(content, str):
                parts.append(content)
    return " ".join(parts)


def run_plugin_chain(body: dict) -> tuple[Optional[dict], Optional[Response]]:
    """
    Run the plugin chain on request body.
    Returns (modified_body, error_response)
    If error_response is not None, the request should be blocked.
    """
    messages = body.get("messages", [])
    user_text = extract_user_text(messages)
    if not user_text.strip():
        return body, None

    sr_headers = {}

    # --- Plugin 1: PII Detection ---
    if PII_ENABLED:
        processed_text, pii_report = process_pii(user_text)
        if pii_report.get("blocked"):
            logger.info("[PII] Request blocked: %s", pii_report["detected"])
            return None, (jsonify({
                "error": {
                    "message": "Request blocked: PII detected. Please remove sensitive information.",
                    "type": "pii_detection_error",
                    "code": "pii_blocked",
                    "details": pii_report,
                }
            }), 400)
        if pii_report:
            logger.info("[PII] Detected (action=%s): %s", PII_ACTION, pii_report["detected"])
            sr_headers["X-SR-PII-Detected"] = "true"
            sr_headers["X-SR-PII-Types"] = ",".join(pii_report.get("detected", {}).keys())
            if PII_ACTION == "mask" and processed_text:
                # Replace user text in messages
                for msg in messages:
                    if msg.get("role") == "user":
                        if isinstance(msg.get("content"), str):
                            msg["content"] = processed_text

    # --- Plugin 2: Jailbreak Detection ---
    if JAILBREAK_ENABLED:
        should_block, jb_report = process_jailbreak(user_text)
        if jb_report:
            logger.info("[Jailbreak] Detected: %s", jb_report["detected"])
            sr_headers["X-SR-Jailbreak-Detected"] = "true"
            sr_headers["X-SR-Jailbreak-Types"] = ",".join(d["type"] for d in jb_report["detected"])
            if should_block:
                return None, (jsonify({
                    "error": {
                        "message": "Request blocked: Potential jailbreak attempt detected.",
                        "type": "jailbreak_detection_error",
                        "code": "jailbreak_blocked",
                        "details": jb_report,
                    }
                }), 403)

    # --- Plugin 3: Domain Classification ---
    if DOMAIN_ENABLED:
        domain, method = classify_domain(user_text)
        logger.info("[Domain] %s (method=%s)", domain, method)
        sr_headers["X-SR-Domain"] = domain
        sr_headers["X-SR-Domain-Method"] = method

    # Store headers for later use
    body["_sr_headers"] = sr_headers
    return body, None


def proxy_to_backend(path: str, method: str, body: dict, headers: dict, stream: bool) -> Response:
    """Proxy request to LiteLLM Worker backend."""
    url = f"{BACKEND_URL}{path}"

    # Forward original headers + SR headers
    fwd_headers = {
        "Authorization": headers.get("Authorization", f"Bearer {BACKEND_KEY}"),
        "Content-Type": "application/json",
    }
    # Pass through SR headers
    sr_headers = body.pop("_sr_headers", {})
    for k, v in sr_headers.items():
        fwd_headers[k] = v

    # Remove internal fields before forwarding
    clean_body = {k: v for k, v in body.items() if not k.startswith("_sr_")}

    if stream:
        # Stream response without buffering
        resp = requests.post(
            url, headers=fwd_headers, json=clean_body,
            stream=True, timeout=300,
        )

        # Add SR headers to streaming response
        stream_headers = {"X-SR-Status": "processed"}
        for k, v in sr_headers.items():
            stream_headers[k] = v

        def generate():
            for chunk in resp.iter_content(chunk_size=4096):
                if chunk:
                    yield chunk

        return Response(
            stream_with_context(generate()),
            content_type=resp.headers.get("Content-Type", "text/event-stream; charset=utf-8"),
            status=resp.status_code,
            headers=stream_headers,
        )
    else:
        resp = requests.post(
            url, headers=fwd_headers, json=clean_body,
            timeout=300,
        )

        response_content = resp.content
        response_headers = dict(resp.headers)
        status_code = resp.status_code

        # Add SR headers to response so clients can see domain/PII/jailbreak info
        for k, v in sr_headers.items():
            response_headers[k] = v

        # --- Plugin 4: Hallucination Detection (non-streaming only) ---
        if HALLUCINATION_ENABLED and status_code == 200 and "application/json" in resp.headers.get("Content-Type", ""):
            try:
                resp_data = resp.json()
                choices = resp_data.get("choices", [])
                if choices:
                    response_text = choices[0].get("message", {}).get("content", "")
                    user_text = extract_user_text(body.get("messages", []))
                    if response_text and user_text:
                        hal_result = detect_hallucination(user_text, response_text)
                        if hal_result.get("hallucination_detected"):
                            logger.info("[Hallucination] Detected: confidence=%.2f", hal_result.get("confidence", 0))
                            if HALLUCINATION_ACTION == "header":
                                response_headers["X-Hallucination-Detected"] = "true"
                                response_headers["X-Hallucination-Confidence"] = str(hal_result.get("confidence", 0))
                            elif HALLUCINATION_ACTION == "body":
                                resp_data["warning"] = {
                                    "hallucination_detected": True,
                                    "confidence": hal_result.get("confidence", 0),
                                    "reason": hal_result.get("reason", ""),
                                }
                                response_content = json.dumps(resp_data, ensure_ascii=False).encode("utf-8")
                            elif HALLUCINATION_ACTION == "block":
                                return jsonify({
                                    "error": {
                                        "message": "Response blocked: Potential hallucination detected.",
                                        "type": "hallucination_detection_error",
                                        "details": hal_result,
                                    }
                                }), 403
            except Exception as e:
                logger.warning("[Hallucination] Check failed: %s", e)

        # Filter out hop-by-hop headers
        for h in ["Transfer-Encoding", "Content-Encoding", "Content-Length", "Connection"]:
            response_headers.pop(h, None)

        return Response(
            response_content,
            status=status_code,
            headers=response_headers,
            content_type=resp.headers.get("Content-Type", "application/json"),
        )


# ==================================================
# Flask Routes
# ==================================================

@app.route("/v1/chat/completions", methods=["POST"])
def chat_completions():
    """Main proxy endpoint with semantic routing plugins."""
    STATS["requests_total"] += 1
    body = request.get_json(silent=True) or {}
    stream = body.get("stream", False)

    # Run plugin chain
    processed_body, error = run_plugin_chain(body)
    if error is not None:
        return error

    # Proxy to LiteLLM Worker
    return proxy_to_backend(
        "/v1/chat/completions",
        "POST",
        processed_body,
        dict(request.headers),
        stream,
    )


@app.route("/v1/models", methods=["GET"])
def list_models():
    """Proxy to backend."""
    resp = requests.get(
        f"{BACKEND_URL}/v1/models",
        headers={"Authorization": request.headers.get("Authorization", f"Bearer {BACKEND_KEY}")},
        timeout=30,
    )
    return Response(resp.content, status=resp.status_code,
                    content_type=resp.headers.get("Content-Type", "application/json"))


@app.route("/v1/embeddings", methods=["POST"])
def embeddings():
    """Proxy to backend."""
    body = request.get_data()
    resp = requests.post(
        f"{BACKEND_URL}/v1/embeddings",
        headers={
            "Authorization": request.headers.get("Authorization", f"Bearer {BACKEND_KEY}"),
            "Content-Type": "application/json",
        },
        data=body,
        timeout=60,
    )
    return Response(resp.content, status=resp.status_code,
                    content_type=resp.headers.get("Content-Type", "application/json"))


@app.route("/health", methods=["GET"])
def health():
    """Health check endpoint."""
    return jsonify({
        "status": "healthy",
        "backend": BACKEND_URL,
        "classifier": CLASSIFIER_MODEL,
        "plugins": {
            "pii": {"enabled": PII_ENABLED, "action": PII_ACTION},
            "jailbreak": {"enabled": JAILBREAK_ENABLED, "action": JAILBREAK_ACTION},
            "hallucination": {"enabled": HALLUCINATION_ENABLED, "action": HALLUCINATION_ACTION},
            "domain": {"enabled": DOMAIN_ENABLED, "categories": len(MMLU_CATEGORIES)},
        },
    })


@app.route("/check", methods=["POST"])
def check_text():
    """Run plugin chain on text without forwarding to backend.
    Used by orchestrator for pre-check before task decomposition.
    """
    body = request.get_json(silent=True) or {}
    text = body.get("text", "")

    result = {
        "pii": {},
        "jailbreak": {},
        "domain": "",
        "domain_method": "",
        "blocked": False,
        "block_reason": "",
    }

    if PII_ENABLED:
        _, pii_report = process_pii(text)
        result["pii"] = pii_report
        if pii_report.get("blocked"):
            result["blocked"] = True
            result["block_reason"] = "pii"

    if JAILBREAK_ENABLED and not result["blocked"]:
        should_block, jb_report = process_jailbreak(text)
        result["jailbreak"] = jb_report
        if should_block:
            result["blocked"] = True
            result["block_reason"] = "jailbreak"

    if DOMAIN_ENABLED and not result["blocked"]:
        domain, method = classify_domain(text)
        result["domain"] = domain
        result["domain_method"] = method

    return jsonify(result)


@app.route("/stats", methods=["GET"])
def stats():
    """Statistics endpoint."""
    return jsonify({
        "stats": STATS,
        "config": {
            "backend_url": BACKEND_URL,
            "classifier_model": CLASSIFIER_MODEL,
            "pii_enabled": PII_ENABLED,
            "jailbreak_enabled": JAILBREAK_ENABLED,
            "hallucination_enabled": HALLUCINATION_ENABLED,
            "domain_enabled": DOMAIN_ENABLED,
            "mmlu_categories": MMLU_CATEGORIES,
        },
    })


# Catch-all proxy for other endpoints
@app.route("/<path:path>", methods=["GET", "POST", "PUT", "DELETE", "PATCH"])
def proxy_catchall(path):
    """Proxy all other requests to backend."""
    url = f"{BACKEND_URL}/{path}"
    headers = {
        "Authorization": request.headers.get("Authorization", f"Bearer {BACKEND_KEY}"),
        "Content-Type": request.headers.get("Content-Type", "application/json"),
    }
    body = request.get_data() if request.method in ("POST", "PUT", "PATCH") else None

    resp = requests.request(
        method=request.method,
        url=url,
        headers=headers,
        data=body,
        params=request.args,
        timeout=60,
    )
    return Response(resp.content, status=resp.status_code,
                    content_type=resp.headers.get("Content-Type", "application/json"))


if __name__ == "__main__":
    logger.info("Semantic Router starting on port 8888")
    logger.info("Backend: %s", BACKEND_URL)
    logger.info("Classifier: %s", CLASSIFIER_MODEL)
    logger.info("Plugins: PII=%s, Jailbreak=%s, Hallucination=%s, Domain=%s",
                PII_ENABLED, JAILBREAK_ENABLED, HALLUCINATION_ENABLED, DOMAIN_ENABLED)
    app.run(host="0.0.0.0", port=8888, debug=False)
