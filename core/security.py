"""Security layer — PII detection, jailbreak interception, domain classification.

Migrated from v1 semantic_router/app.py as pure functions (no Flask, no HTTP proxy).
All regex patterns use lookbehind/lookahead assertions (not \b) for Chinese compatibility.
"""

import re
from typing import Any

import litellm

from core.config import get_env

# ── PII Detection (7 types) ──

PII_PATTERNS: dict[str, re.Pattern] = {
    "EMAIL_ADDRESS": re.compile(
        r'(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}(?![A-Za-z])',
        re.IGNORECASE,
    ),
    "PHONE_NUMBER": re.compile(
        r'(?<!\d)(?:1[3-9]\d{9})(?!\d)'
        r'|(?<!\d)(?:\+?1\s*(?:[.-]\s*)?(?:\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}))(?!\d)',
        re.IGNORECASE,
    ),
    "US_SSN": re.compile(r'(?<!\d)\d{3}-\d{2}-\d{4}(?!\d)'),
    "CREDIT_CARD": re.compile(r'(?<!\d)(?:\d[ -]*?){13,16}(?!\d)'),
    "IP_ADDRESS": re.compile(
        r'(?<!\d)(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)(?!\d)'
    ),
    "ID_CARD": re.compile(r'(?<!\d)\d{17}[\dXx](?!\d)'),
    "BANK_ACCOUNT": re.compile(
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

PII_ACTION_MASK = "mask"
PII_ACTION_BLOCK = "block"
PII_ACTION_WARN = "warn"

# ── Jailbreak Detection (5 attack types + keyword) ──

JAILBREAK_PATTERNS: list[tuple[re.Pattern, str]] = [
    (re.compile(r'you\s+are\s+(now\s+)?(?:DAN|do\s+anything\s+now)', re.IGNORECASE), "DAN_ATTACK"),
    (re.compile(r'ignore\s+(?:previous|prior|above|all)\s+(?:instructions?|prompts?|rules?|guidelines?)', re.IGNORECASE), "INSTRUCTION_OVERRIDE"),
    (re.compile(r'disregard\s+(?:previous|prior|above|all)\s*(?:instructions?|prompts?)', re.IGNORECASE), "INSTRUCTION_OVERRIDE"),
    (re.compile(r'forget\s+(?:everything|all|previous)', re.IGNORECASE), "INSTRUCTION_OVERRIDE"),
    (re.compile(r'(?:pretend|act\s+as|roleplay)\s+(?:you\s+are\s+)?(?:a|an)?\s*(?:different|unrestricted|unfiltered|unlimited|free|evil|hacker)', re.IGNORECASE), "ROLE_MANIPULATION"),
    (re.compile(r'you\s+are\s+now\s+(?:in\s+)?(?:developer|root|admin|god|unlimited)\s+mode', re.IGNORECASE), "ROLE_MANIPULATION"),
    (re.compile(r'(?:no|without|remove|disable)\s+(?:safety|restrictions?|guidelines?|rules?|limits?|filters?|guardrails?)', re.IGNORECASE), "SAFETY_BYPASS"),
    (re.compile(r'(?:bypass|circumvent|override)\s+(?:safety|security|content\s+filter)', re.IGNORECASE), "SAFETY_BYPASS"),
    (re.compile(r'(?:system|admin|developer)\s+prompt\s*(?:is|:|=\s*)', re.IGNORECASE), "PROMPT_INJECTION"),
    (re.compile(r'reveal|show|print|output\s+(?:your|the)\s+(?:system\s+)?prompt', re.IGNORECASE), "PROMPT_INJECTION"),
    (re.compile(r'(?<!\w)jailbreak(?!\w)', re.IGNORECASE), "JAILBREAK_KEYWORD"),
]

JAILBREAK_ACTION_BLOCK = "block"
JAILBREAK_ACTION_WARN = "warn"

# ── Domain Classification (MMLU 14 categories) ──

MMLU_CATEGORIES = [
    "math", "physics", "chemistry", "biology", "computer_science",
    "engineering", "business", "law", "economics", "health",
    "psychology", "philosophy", "history", "other",
]

DOMAIN_KEYWORDS: dict[str, list[str]] = {
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

DOMAIN_MAX_TOKENS = 20
DOMAIN_TIMEOUT = 10
DOMAIN_MAX_TEXT_LEN = 500


# ── PII functions ──

def detect_pii(text: str) -> dict[str, list[str]]:
    """Detect PII in text. Returns dict of {type: [matches]}."""
    findings: dict[str, list[str]] = {}
    for pii_type, pattern in PII_PATTERNS.items():
        matches = pattern.findall(text)
        if matches:
            cleaned = []
            for match in matches:
                if isinstance(match, tuple):
                    match = match[0] if match[0] else (match[1] if len(match) > 1 else "")
                if match:
                    cleaned.append(str(match))
            if cleaned:
                findings[pii_type] = cleaned
    return findings


def mask_pii(text: str, findings: dict[str, list[str]]) -> str:
    """Mask PII in text."""
    for pii_type, matches in findings.items():
        mask = PII_MASKS.get(pii_type, "[REDACTED]")
        for match in matches:
            text = text.replace(match, mask)
    return text


def process_pii(text: str, action: str = PII_ACTION_MASK) -> tuple[str | None, dict]:
    """Process PII. Returns (processed_text_or_None, report)."""
    findings = detect_pii(text)
    if not findings:
        return text, {}

    report = {
        "detected": {k: len(v) for k, v in findings.items()},
        "action": action,
    }

    if action == PII_ACTION_BLOCK:
        report["blocked"] = True
        return None, report
    elif action == PII_ACTION_MASK:
        masked = mask_pii(text, findings)
        return masked, report
    else:
        return text, report


# ── Jailbreak functions ──

def detect_jailbreak(text: str) -> dict:
    """Detect jailbreak attempts. Returns report dict."""
    detections = []
    for pattern, attack_type in JAILBREAK_PATTERNS:
        matches = pattern.findall(text)
        if matches:
            detections.append({
                "type": attack_type,
                "count": len(matches),
                "sample": str(matches[0][:100]) if matches else "",
            })
    return {"detected": detections} if detections else {}


def process_jailbreak(text: str, action: str = JAILBREAK_ACTION_BLOCK) -> tuple[bool, dict]:
    """Process jailbreak detection. Returns (should_block, report)."""
    report = detect_jailbreak(text)
    if not report:
        return False, {}

    report["action"] = action
    if action == JAILBREAK_ACTION_BLOCK:
        return True, report
    else:
        return False, report


# ── Domain classification functions ──

def _keyword_classify_domain(text: str) -> str | None:
    """Fast keyword-based domain classification (zero cost)."""
    text_lower = text.lower()
    scores: dict[str, int] = {}
    for domain, keywords in DOMAIN_KEYWORDS.items():
        score = sum(1 for kw in keywords if kw.lower() in text_lower)
        if score > 0:
            scores[domain] = score
    if scores:
        return max(scores, key=scores.get)
    return None


def _llm_classify_domain(text: str) -> str:
    """LLM-based domain classification (fallback)."""
    from core import database as db
    truncated = text[:DOMAIN_MAX_TEXT_LEN]

    dashscope_key = get_env("DASHSCOPE_API_KEY")
    if dashscope_key:
        model = db.get_model("qwen3.6-flash")
        litellm_model = model["litellm_model"] if model else "openai/qwen3.6-flash"
        api_base = get_env("DASHSCOPE_API_BASE") or "https://dashscope.aliyuncs.com/compatible-mode/v1"
        api_key = dashscope_key
    else:
        model = db.get_model("qwen2.5-local")
        litellm_model = model["litellm_model"] if model else "ollama/qwen2.5:latest"
        api_base = get_env("OLLAMA_API_BASE") or "http://localhost:11434"
        api_key = "ollama"

    try:
        response = litellm.completion(
            model=litellm_model,
            api_base=api_base,
            api_key=api_key,
            messages=[
                {"role": "system", "content": DOMAIN_SYSTEM_PROMPT},
                {"role": "user", "content": truncated},
            ],
            max_tokens=DOMAIN_MAX_TOKENS,
            temperature=0,
            timeout=DOMAIN_TIMEOUT,
        )
        result = response.choices[0].message.content.strip().lower()
        if result in MMLU_CATEGORIES:
            return result
        for cat in MMLU_CATEGORIES:
            if cat in result:
                return cat
        return "other"
    except Exception:
        return "other"


def classify_domain(text: str) -> tuple[str, str]:
    """Classify text into MMLU 14 categories. Returns (domain, method)."""
    domain = _keyword_classify_domain(text)
    if domain:
        return domain, "keyword"
    domain = _llm_classify_domain(text)
    return domain, "llm"


# ── Unified security check ──

def check(text: str, pii_action: str = PII_ACTION_MASK, jailbreak_action: str = JAILBREAK_ACTION_BLOCK) -> dict[str, Any]:
    """Run all security checks on text. Returns comprehensive result.

    Pipeline: PII → Jailbreak (if not blocked) → Domain (if not blocked)
    """
    result: dict[str, Any] = {
        "pii": {},
        "jailbreak": {},
        "domain": "",
        "domain_method": "",
        "blocked": False,
        "block_reason": "",
        "processed_text": text,
    }

    # PII
    processed_text, pii_report = process_pii(text, pii_action)
    result["pii"] = pii_report
    if pii_report.get("blocked"):
        result["blocked"] = True
        result["block_reason"] = "pii"
        return result
    if processed_text is not None:
        result["processed_text"] = processed_text

    # Jailbreak (runs on ORIGINAL text, not masked)
    should_block, jb_report = process_jailbreak(text, jailbreak_action)
    result["jailbreak"] = jb_report
    if should_block:
        result["blocked"] = True
        result["block_reason"] = "jailbreak"
        return result

    # Domain (only if not blocked)
    domain, method = classify_domain(text)
    result["domain"] = domain
    result["domain_method"] = method

    return result


def extract_user_text(messages: list[dict]) -> str:
    """Extract the last user message text from messages."""
    for msg in reversed(messages):
        if msg.get("role") == "user":
            content = msg.get("content", "")
            if isinstance(content, list):
                return " ".join(
                    part.get("text", "") for part in content if isinstance(part, dict)
                )
            return content
    return ""
