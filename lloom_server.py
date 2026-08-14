#!/usr/bin/env python3
"""LLooM v2 — PyInstaller entry point launcher.

This script is the entry point for PyInstaller packaging.
It sets up sys.path for the frozen environment and starts the FastAPI server.
"""

import os
import sys

# In PyInstaller onedir mode, the app directory is next to the executable
if getattr(sys, "frozen", False):
    base_dir = os.path.dirname(sys.executable)
    # Add the base dir to sys.path so `from core...` and `from api...` work
    sys.path.insert(0, base_dir)
    # Also add the _internal dir where PyInstaller puts packages
    internal_dir = os.path.join(base_dir, "_internal")
    if os.path.isdir(internal_dir):
        sys.path.insert(0, internal_dir)
    # Set working directory to the app dir so relative paths (.env, data/) work
    os.chdir(base_dir)
else:
    # Development mode — use script's parent directory
    base_dir = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, base_dir)
    os.chdir(base_dir)

# Set up environment
os.environ.setdefault("LLOOM_DATA_DIR", os.path.join(base_dir, "data"))

# Use bundled tiktoken cache
tiktoken_cache = os.path.join(base_dir, "tiktoken_cache")
if os.path.isdir(tiktoken_cache):
    os.environ.setdefault("TIKTOKEN_CACHE_DIR", tiktoken_cache)

# Fix SSL certificates in frozen environment
import ssl
import certifi
ssl_cert = certifi.where()
if os.path.exists(ssl_cert):
    os.environ.setdefault("SSL_CERT_FILE", ssl_cert)
    os.environ.setdefault("REQUESTS_CA_BUNDLE", ssl_cert)
    ssl._create_default_https_context = ssl.create_default_context
    ssl._create_default_https_context().load_verify_locations(ssl_cert)

from api.server import app  # noqa: E402
import uvicorn  # noqa: E402
from core.config import get_api_port  # noqa: E402

if __name__ == "__main__":
    port = get_api_port()
    print(f"LLooM v2 API server starting on port {port}...")
    uvicorn.run(app, host="0.0.0.0", port=port)
