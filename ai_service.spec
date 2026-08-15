# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for the Python AI micro-service.

Builds a standalone executable for `api/ai_service.py` — the only remaining
Python component. It wraps litellm (LLM provider calls) behind a small FastAPI
service. Output: `dist/ai-service/ai-service`.

The Rust core spawns this executable (see `processes::start_ai`).
"""

from PyInstaller.utils.hooks import collect_submodules, collect_data_files

hidden_imports = (
    collect_submodules("litellm")
    + collect_submodules("fastapi")
    + collect_submodules("uvicorn")
    + collect_submodules("starlette")
    + collect_submodules("pydantic")
    + [
        "uvicorn.protocols.http.auto",
        "uvicorn.protocols.http.h11_impl",
        "uvicorn.protocols.websockets.auto",
        "uvicorn.protocols.websockets.wsproto_impl",
        "uvicorn.lifespan.on",
        "tiktoken",
        "tiktoken_ext",
        "tiktoken_ext.openai_public",
        "tiktoken_ext.encoding",
        "httpx",
        "anyio",
        "sniffio",
        "email.mime.text",
        "email.mime.multipart",
        "backoff",
    ]
)

datas = (
    collect_data_files("litellm")
    + [(".env.example", ".")]
)

a = Analysis(
    ["api/ai_service.py"],
    pathex=["."],
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
        "chromadb",
        "sqlalchemy",
    ],
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="ai-service",
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
    name="ai-service",
    onedir=True,
    strip=False,
    upx=True,
    upx_exclude=[],
    name_as_version=False,
)
