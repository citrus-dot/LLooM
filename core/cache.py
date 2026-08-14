"""SemanticCache — ChromaDB-based semantic caching for LLM responses.

Replaces v1's Qdrant + litellm cache with a local ChromaDB instance.
No external service required — ChromaDB runs in-process with SQLite backend.
"""

import hashlib
import time
from typing import Any

import chromadb
from chromadb.config import Settings as ChromaSettings

from core.config import get_cache_dir, get_env

DEFAULT_SIMILARITY_THRESHOLD = 0.95
DEFAULT_TTL = 86400  # 24 hours
COLLECTION_NAME = "lloom_cache"


class SemanticCache:
    """Semantic cache using ChromaDB for vector similarity search.

    Usage:
        cache = SemanticCache()
        cache.start()
        hit = cache.get("user query", model="qwen-plus")
        if hit:
            return hit["response"]
        # ... call LLM ...
        cache.put("user query", response_text, model="qwen-plus")
    """

    def __init__(
        self,
        similarity_threshold: float = DEFAULT_SIMILARITY_THRESHOLD,
        ttl: int = DEFAULT_TTL,
    ):
        self.similarity_threshold = similarity_threshold
        self.ttl = ttl
        self._client: chromadb.api.ClientAPI | None = None
        self._collection = None
        self._enabled = False

    def start(self) -> None:
        """Initialize ChromaDB client and collection."""
        if self._enabled:
            return
        try:
            self._client = chromadb.PersistentClient(
                path=str(get_cache_dir()),
                settings=ChromaSettings(anonymized_telemetry=False),
            )
            self._collection = self._client.get_or_create_collection(
                name=COLLECTION_NAME,
                metadata={"hnsw:space": "cosine"},
            )
            self._enabled = True
        except Exception:
            self._enabled = False

    def stop(self) -> None:
        """Close the cache."""
        self._collection = None
        self._client = None
        self._enabled = False

    @property
    def enabled(self) -> bool:
        return self._enabled

    def _get_embedding_function(self):
        """Return the embedding function, or None to use ChromaDB default."""
        return None

    def _doc_id(self, query: str, model: str) -> str:
        raw = f"{model}:{query}"
        return hashlib.md5(raw.encode()).hexdigest()

    def get(self, query: str, model: str = "default") -> dict[str, Any] | None:
        """Check cache for a semantically similar query. Returns hit dict or None."""
        if not self._enabled or not self._collection:
            return None

        try:
            results = self._collection.query(
                query_texts=[query],
                n_results=1,
                where={"model": model} if model != "default" else None,
            )
            if not results or not results["ids"] or not results["ids"][0]:
                return None

            distance = results["distances"][0][0]
            similarity = 1 - distance
            if similarity < self.similarity_threshold:
                return None

            metadata = results["metadatas"][0][0]
            cached_at = metadata.get("cached_at", 0)
            if self.ttl > 0 and (time.time() - cached_at) > self.ttl:
                return None

            return {
                "response": metadata.get("response", ""),
                "model": metadata.get("model", model),
                "similarity": similarity,
                "cached_at": cached_at,
            }
        except Exception:
            return None

    def put(self, query: str, response: str, model: str = "default") -> None:
        """Store a query-response pair in cache."""
        if not self._enabled or not self._collection:
            return

        try:
            doc_id = self._doc_id(query, model)
            self._collection.upsert(
                ids=[doc_id],
                documents=[query],
                metadatas=[{
                    "response": response,
                    "model": model,
                    "cached_at": time.time(),
                }],
            )
        except Exception:
            pass

    def clear(self) -> int:
        """Clear all cached entries. Returns count deleted."""
        if not self._enabled or not self._client:
            return 0
        try:
            self._client.delete_collection(COLLECTION_NAME)
            self._collection = self._client.get_or_create_collection(
                name=COLLECTION_NAME,
                metadata={"hnsw:space": "cosine"},
            )
            return 1
        except Exception:
            return 0

    def count(self) -> int:
        """Return number of cached entries."""
        if not self._enabled or not self._collection:
            return 0
        try:
            return self._collection.count()
        except Exception:
            return 0


_cache: SemanticCache | None = None


def get_cache() -> SemanticCache:
    """Get the global cache instance (lazy init)."""
    global _cache
    if _cache is None:
        _cache = SemanticCache()
        _cache.start()
    return _cache
