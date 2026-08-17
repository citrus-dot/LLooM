"""Fast, checksum-verified provisioning of ChromaDB's default embedding model.

Why this module exists
---------------------
`chromadb`'s default embedding function (`ONNXMiniLM_L6_V2`) lazily downloads a
~79 MB `onnx.tar.gz` from `chroma-onnx-models.s3.amazonaws.com`. That host is
effectively unusable from mainland China — measured throughput was **6.25 KB/s**,
i.e. >3 hours for one archive, which is why semantic-cache init used to hang and
time out.

Setting `HF_ENDPOINT` (as the previous implementation did) has **no effect**:
chroma never contacts HuggingFace for this model. The env var was a no-op.

The fix relies on chroma's own cache contract. `_download_model_if_not_exists()`
skips *both* the download and the archive checksum when all six extracted files
already exist in `~/.cache/chroma/onnx_models/all-MiniLM-L6-v2/onnx/`. So we
provision exactly those files ourselves, pulled from a fast mirror of the
upstream `sentence-transformers/all-MiniLM-L6-v2` repository — the same weights
chroma re-exports — and verify every byte against a pinned sha256 before putting
it in place.

Measured throughput (same machine, same session):
    chroma-onnx-models.s3.amazonaws.com     6.25 KB/s   (baseline, unusable)
    hf-mirror.com                          11.8 MB/s   (~1900x faster)
    modelscope.cn                           7.8 MB/s
So a cold init drops from "hours / never" to roughly 10 seconds.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

MODEL_NAME = "all-MiniLM-L6-v2"

# Layout owned by chromadb — do not change without checking
# chromadb/utils/embedding_functions/onnx_mini_lm_l6_v2.py.
CHROMA_MODEL_ROOT = Path.home() / ".cache" / "chroma" / "onnx_models" / MODEL_NAME
EXTRACTED_DIR = CHROMA_MODEL_ROOT / "onnx"
STAGING_DIR = CHROMA_MODEL_ROOT / "onnx.staging"
LEGACY_ARCHIVE = CHROMA_MODEL_ROOT / "onnx.tar.gz"

# The six files chroma requires, pinned to the upstream
# `sentence-transformers/all-MiniLM-L6-v2` main revision.
#
# `model.onnx`'s sha256 is HuggingFace's LFS oid (which *is* the sha256); the
# small files were hashed locally after download. All six were confirmed
# byte-identical on the ModelScope mirror, so one checksum table covers every
# mirror — a tampered or truncated file from any source is rejected.
FILES: dict[str, dict[str, Any]] = {
    "model.onnx": {
        "remote": "onnx/model.onnx",
        "size": 90405214,
        "sha256": "6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452",
    },
    "tokenizer.json": {
        "remote": "tokenizer.json",
        "size": 466247,
        "sha256": "be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037",
    },
    "vocab.txt": {
        "remote": "vocab.txt",
        "size": 231508,
        "sha256": "07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3",
    },
    "config.json": {
        "remote": "config.json",
        "size": 612,
        "sha256": "953f9c0d463486b10a6871cc2fd59f223b2c70184f49815e7efbcab5d8908b41",
    },
    "tokenizer_config.json": {
        "remote": "tokenizer_config.json",
        "size": 350,
        "sha256": "acb92769e8195aabd29b7b2137a9e6d6e25c476a4f15aa4355c233426c61576b",
    },
    "special_tokens_map.json": {
        "remote": "special_tokens_map.json",
        "size": 112,
        "sha256": "303df45a03609e4ead04bc3dc1536d0ab19b5358db685b6f3da123d05ec200e3",
    },
}

TOTAL_BYTES = sum(f["size"] for f in FILES.values())

# Ordered fastest-first (measured). Each entry is (label, url template).
_DEFAULT_MIRRORS: list[tuple[str, str]] = [
    (
        "hf-mirror.com",
        "https://hf-mirror.com/sentence-transformers/all-MiniLM-L6-v2/resolve/main/{remote}",
    ),
    (
        "modelscope.cn",
        "https://modelscope.cn/models/sentence-transformers/all-MiniLM-L6-v2/resolve/master/{remote}",
    ),
    (
        "huggingface.co",
        "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/{remote}",
    ),
]

# Per-socket-read budget. A mirror that connects then stalls fails fast and we
# move to the next one, rather than burning the whole init timeout.
_READ_TIMEOUT = 20.0
# Max wall-clock time for a single file; and for a whole provisioning run. Either
# guard fails the current mirror (or the whole run) instead of hanging.
_FILE_TIMEOUT = 180.0
_OVERALL_TIMEOUT = 600.0
_CHUNK = 1 << 20  # 1 MiB

_lock = threading.Lock()
_progress: dict[str, Any] = {
    "phase": "idle",  # idle | downloading | verifying | done | error
    "mirror": "",
    "file": "",
    "done_bytes": 0,
    "total_bytes": TOTAL_BYTES,
    "file_done": 0,
    "file_total": 0,
    "speed_bps": 0.0,
    "error": "",
}


def mirrors() -> list[tuple[str, str]]:
    """Mirror list, overridable via `LLOOM_EMBED_MIRRORS`.

    The env var takes comma-separated URL templates containing `{remote}`, e.g.
    an internal artifact host. Templates are tried in the given order and always
    checksum-verified, so an untrusted mirror cannot inject a bad model.
    """
    raw = os.getenv("LLOOM_EMBED_MIRRORS", "").strip()
    if not raw:
        return list(_DEFAULT_MIRRORS)
    out: list[tuple[str, str]] = []
    for tpl in (p.strip() for p in raw.split(",")):
        if not tpl:
            continue
        if "{remote}" not in tpl:
            tpl = tpl.rstrip("/") + "/{remote}"
        label = tpl.split("//")[-1].split("/")[0] or "custom"
        out.append((label, tpl))
    return out or list(_DEFAULT_MIRRORS)


def progress() -> dict[str, Any]:
    """Thread-safe snapshot of download progress for the status endpoint."""
    with _lock:
        snap = dict(_progress)
    total = snap["total_bytes"] or 1
    snap["percent"] = round(min(100.0, snap["done_bytes"] / total * 100), 1)
    ft = snap.get("file_total") or 1
    snap["file_percent"] = round(min(100.0, (snap.get("file_done") or 0) / ft * 100), 1)
    return snap


def _set(**kw: Any) -> None:
    with _lock:
        _progress.update(kw)


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def _file_ok(path: Path, spec: dict[str, Any], strict: bool = True) -> bool:
    """Check a file against the pinned spec.

    `strict` additionally verifies sha256. Skipping it matters at startup, where
    re-hashing a 90 MB model on every boot would be pure waste — an exact size
    match on a file we wrote ourselves is a good enough liveness signal, and the
    content was already hash-verified when it was downloaded.
    """
    try:
        if not path.is_file() or path.stat().st_size != spec["size"]:
            return False
        return _sha256(path) == spec["sha256"] if strict else True
    except OSError:
        return False


def is_provisioned(strict: bool = False) -> bool:
    """True when every required file is present (and hash-clean if `strict`)."""
    return all(
        _file_ok(EXTRACTED_DIR / name, spec, strict=strict)
        for name, spec in FILES.items()
    )


def _download_one(url: str, dest: Path, spec: dict[str, Any], base_done: int) -> None:
    """Stream one file to `dest`, hashing as we go.

    Supports Range-based resume (HTTP 206): a `.part` left by an interrupted
    download is continued rather than restarted, so a flaky mirror only costs the
    missing tail. A per-file wall-clock timeout (and the per-socket-read timeout)
    fail the mirror fast instead of hanging. Raises on any size/sha256 mismatch.
    """
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")

    # Resume from a usable partial download if one exists.
    resume_from = 0
    if tmp.is_file():
        sz = tmp.stat().st_size
        if 0 < sz < spec["size"]:
            resume_from = sz

    # Pre-hash the already-downloaded prefix so the final digest is continuous.
    hasher = hashlib.sha256()
    if resume_from:
        with open(tmp, "rb") as ph:
            for block in iter(lambda: ph.read(1 << 20), b""):
                hasher.update(block)
    written = resume_from

    headers = {
        "User-Agent": "LLooM/2.0 (semantic-cache model provisioner)",
        "Accept": "*/*",
    }
    if resume_from:
        headers["Range"] = f"bytes={resume_from}-"

    req = urllib.request.Request(url, headers=headers)
    file_start = time.monotonic()
    window_start = file_start
    window_bytes = 0

    with urllib.request.urlopen(req, timeout=_READ_TIMEOUT) as resp:
        # 206 = resumed; 200 = full (server ignored Range -> restart fresh).
        if resume_from and resp.status == 200:
            written = 0
            hasher = hashlib.sha256()
        elif resp.status not in (200, 206):
            raise OSError(f"HTTP {resp.status}")
        # Append when we have a verified prefix (206); otherwise write fresh.
        mode = "ab" if written > 0 else "wb"
        with open(tmp, mode) as fh:
            while True:
                chunk = resp.read(_CHUNK)
                if not chunk:
                    break
                fh.write(chunk)
                hasher.update(chunk)
                written += len(chunk)
                window_bytes += len(chunk)
                elapsed = time.monotonic() - window_start
                if elapsed >= 0.5:
                    _set(
                        done_bytes=base_done + written,
                        file_done=written,
                        file_total=spec["size"],
                        speed_bps=window_bytes / elapsed,
                    )
                    window_start = time.monotonic()
                    window_bytes = 0
                if time.monotonic() - file_start > _FILE_TIMEOUT:
                    raise OSError(f"per-file timeout after {_FILE_TIMEOUT:.0f}s")

    if written != spec["size"]:
        tmp.unlink(missing_ok=True)
        raise OSError(f"size mismatch: got {written}, want {spec['size']}")
    if hasher.hexdigest() != spec["sha256"]:
        tmp.unlink(missing_ok=True)
        raise OSError(f"sha256 mismatch: got {hasher.hexdigest()[:16]}…, want {spec['sha256'][:16]}…")
    tmp.replace(dest)
    _set(done_bytes=base_done + written, file_done=spec["size"], file_total=spec["size"])


def provision(force: bool = False) -> dict[str, Any]:
    """Ensure the model is on disk, downloading from the fastest live mirror.

    Files already present and checksum-clean are reused, so a retry after a
    partial failure only fetches what is missing. Every file is verified before
    it is moved into the directory chroma reads, so chroma either sees a complete
    valid model or nothing at all.
    """
    if not force and is_provisioned(strict=True):
        _set(
            phase="done",
            done_bytes=TOTAL_BYTES,
            mirror="local cache",
            file="",
            error="",
        )
        return {"ok": True, "mirror": "local cache", "skipped": True}

    STAGING_DIR.mkdir(parents=True, exist_ok=True)
    errors: list[str] = []
    run_start = time.monotonic()

    for label, template in mirrors():
        _set(phase="downloading", mirror=label, error="", speed_bps=0.0)
        done = 0
        try:
            for name, spec in FILES.items():
                if time.monotonic() - run_start > _OVERALL_TIMEOUT:
                    raise OSError(f"overall provisioning timeout after {_OVERALL_TIMEOUT:.0f}s")
                final = EXTRACTED_DIR / name
                staged = STAGING_DIR / name
                # Reuse anything already verified (previous run or partial retry).
                if not force and _file_ok(final, spec):
                    done += spec["size"]
                    _set(done_bytes=done, file=name, file_done=spec["size"], file_total=spec["size"])
                    continue
                if _file_ok(staged, spec):
                    done += spec["size"]
                    _set(done_bytes=done, file=name, file_done=spec["size"], file_total=spec["size"])
                    continue
                _set(file=name, file_done=0, file_total=spec["size"])
                _download_one(template.format(remote=spec["remote"]), staged, spec, done)
                done += spec["size"]

            # All six verified — publish atomically, file by file.
            _set(phase="verifying", file="", file_done=0, file_total=0, speed_bps=0.0)
            EXTRACTED_DIR.mkdir(parents=True, exist_ok=True)
            for name in FILES:
                staged = STAGING_DIR / name
                if staged.is_file():
                    os.replace(staged, EXTRACTED_DIR / name)

            if not is_provisioned():
                raise OSError("post-install verification failed")

            # The stale S3 archive (often a truncated partial) is dead weight now.
            LEGACY_ARCHIVE.unlink(missing_ok=True)
            shutil.rmtree(STAGING_DIR, ignore_errors=True)

            _set(
                phase="done",
                mirror=label,
                done_bytes=TOTAL_BYTES,
                file="",
                error="",
            )
            return {"ok": True, "mirror": label, "skipped": False}

        except (urllib.error.URLError, OSError, ValueError) as e:
            errors.append(f"{label}: {e}")
            _set(error=f"{label} failed: {e}")
            continue

    shutil.rmtree(STAGING_DIR, ignore_errors=True)
    msg = "all mirrors failed — " + "; ".join(errors)
    _set(phase="error", error=msg, speed_bps=0.0)
    raise OSError(msg)


def verify_model() -> dict[str, Any]:
    """Run the model through chroma's own pipeline and sanity-check the output.

    Guards against a file that passes checksum but is the wrong *kind* of export
    (e.g. a quantised or pooled variant chroma can't drive): chroma feeds
    input_ids/attention_mask/token_type_ids and expects `last_hidden_state`,
    then does its own mean-pooling and L2 normalisation.
    """
    from chromadb.utils.embedding_functions.onnx_mini_lm_l6_v2 import ONNXMiniLM_L6_V2

    ef = ONNXMiniLM_L6_V2()
    texts = [
        "The cat sits outside",
        "There is a cat outside",
        "Quantum chromodynamics describes the strong interaction",
    ]
    vecs = ef(texts)

    dim = len(vecs[0])
    if dim != 384:
        raise ValueError(f"unexpected embedding dim {dim}, want 384")

    def norm(v: Any) -> float:
        return float(sum(x * x for x in v) ** 0.5)

    norms = [norm(v) for v in vecs]
    if any(abs(n - 1.0) > 1e-3 for n in norms):
        raise ValueError(f"embeddings are not L2-normalised: {norms}")

    def cos(a: Any, b: Any) -> float:
        return float(sum(x * y for x, y in zip(a, b)))

    near = cos(vecs[0], vecs[1])   # same topic -> should be high
    far = cos(vecs[0], vecs[2])    # unrelated  -> should be low
    if not (near > 0.5 and far < 0.3 and near - far > 0.3):
        raise ValueError(
            f"semantic ordering looks wrong (related={near:.3f}, unrelated={far:.3f})"
        )

    return {
        "dim": dim,
        "l2_norm": round(norms[0], 6),
        "similarity_related": round(near, 4),
        "similarity_unrelated": round(far, 4),
    }


def purge(model: bool = False) -> dict[str, Any]:
    """Remove download leftovers. Keeps the verified model unless `model=True`.

    The model is the expensive part, so a routine cleanup keeps it — that makes
    re-initialisation instant instead of re-downloading ~87 MB.
    """
    removed: list[str] = []
    if LEGACY_ARCHIVE.exists():
        LEGACY_ARCHIVE.unlink(missing_ok=True)
        removed.append(str(LEGACY_ARCHIVE))
    if STAGING_DIR.exists():
        shutil.rmtree(STAGING_DIR, ignore_errors=True)
        removed.append(str(STAGING_DIR))
    if model and EXTRACTED_DIR.exists():
        shutil.rmtree(EXTRACTED_DIR, ignore_errors=True)
        removed.append(str(EXTRACTED_DIR))
        _set(phase="idle", done_bytes=0, mirror="", file="", error="", speed_bps=0.0)
    return {"removed": removed}


# An already-provisioned model should report as complete from the first status
# poll after a restart, not as 0% idle.
if is_provisioned():
    _set(phase="done", done_bytes=TOTAL_BYTES, mirror="local cache")
