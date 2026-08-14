# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for LLooM v2 Python core.

Builds a onedir bundle containing the FastAPI server, litellm, chromadb,
and all dependencies. The output directory is 'dist/lloom-server'.
"""

import os
from PyInstaller.utils.hooks import collect_submodules, collect_data_files

block_cipher = None

# Collect all submodules for packages with dynamic imports
litellm_hidden = collect_submodules("litellm")
chromadb_hidden = collect_submodules("chromadb")
uvicorn_hidden = collect_submodules("uvicorn")
fastapi_hidden = collect_submodules("fastapi")
starlette_hidden = collect_submodules("starlette")
pydantic_hidden = collect_submodules("pydantic")

hidden_imports = (
    litellm_hidden
    + chromadb_hidden
    + uvicorn_hidden
    + fastapi_hidden
    + starlette_hidden
    + pydantic_hidden
    + [
        # Uvicorn protocol implementations
        "uvicorn.protocols.http.auto",
        "uvicorn.protocols.http.h11_impl",
        "uvicorn.protocols.websockets.auto",
        "uvicorn.protocols.websockets.wsproto_impl",
        "uvicorn.lifespan.on",
        # litellm core
        "litellm",
        "litellm.llms.openai",
        "litellm.llms.anthropic",
        "litellm.llms.azure",
        "litellm.llms.ollama",
        "litellm.llms.cohere",
        "litellm.llms.huggingface",
        "litellm.llms.bedrock",
        "litellm.llms.vertex_ai",
        "litellm.llms.gemini",
        "litellm.integrations.custom_logger",
        "litellm.utils",
        # ChromaDB
        "chromadb.config",
        "chromadb.db",
        "chromadb.api",
        "chromadb.telemetry",
        # tiktoken — encoding registry loaded dynamically
        "tiktoken",
        "tiktoken_ext",
        "tiktoken_ext.openai_public",
        "tiktoken_ext.encoding",
        # Others
        "httpx",
        "anyio",
        "sniffio",
        "email.mime.text",
        "email.mime.multipart",
        "backoff",
        "opentelemetry.instrumentation",
    ]
)

# Collect data files
datas = (
    collect_data_files("litellm")
    + collect_data_files("chromadb")
    + [
        # Include our source modules
        ("core", "core"),
        ("api", "api"),
        ("cli", "cli"),
        (".env.example", "."),
        ("pyproject.toml", "."),
        # tiktoken encoding cache (pre-downloaded)
        ("tiktoken_cache", "tiktoken_cache"),
    ]
)

a = Analysis(
    ["lloom_server.py"],
    pathex=[os.path.abspath(".")],
    binaries=[],
    datas=datas,
    hiddenimports=hidden_imports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "tkinter",
        "matplotlib",
        "PIL",
        "pytest",
        "IPython",
        "notebook",
        "jupyter",
    ],
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="lloom-server",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,
    disable_windowed_traceback=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name="lloom-server",
    onedir=True,
    strip=False,
    upx=True,
    upx_exclude=[],
    name_as_version=False,
)
