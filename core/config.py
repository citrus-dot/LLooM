"""Configuration management — reads/writes .env file."""

import os
from pathlib import Path
from dotenv import load_dotenv

load_dotenv()

def get_data_dir() -> Path:
    """Get the data directory for SQLite, ChromaDB, conversations."""
    data_dir = Path(os.getenv("LLOOM_DATA_DIR", "./data"))
    data_dir.mkdir(parents=True, exist_ok=True)
    return data_dir

def get_db_path() -> Path:
    return get_data_dir() / "lloom.db"

def get_cache_dir() -> Path:
    path = get_data_dir() / "chroma"
    path.mkdir(parents=True, exist_ok=True)
    return path

def get_conversations_dir() -> Path:
    path = get_data_dir() / "conversations"
    path.mkdir(parents=True, exist_ok=True)
    return path

def get_api_port() -> int:
    return int(os.getenv("LLOOM_API_PORT", "7860"))

def get_env(key: str, default: str = "") -> str:
    return os.getenv(key, default)

def read_env_file() -> dict[str, str]:
    """Read all key-value pairs from .env file."""
    env_path = Path(".env")
    if not env_path.exists():
        return {}
    result = {}
    for line in env_path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" in line:
            key, _, value = line.partition("=")
            result[key.strip()] = value.strip()
    return result

def write_env_file(updates: dict[str, str]) -> None:
    """Update or add key-value pairs in .env file."""
    env_path = Path(".env")
    existing = read_env_file()
    existing.update(updates)
    lines = []
    for key, value in existing.items():
        lines.append(f"{key}={value}")
    env_path.write_text("\n".join(lines) + "\n")
