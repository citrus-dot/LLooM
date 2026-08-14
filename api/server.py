"""FastAPI server — minimal health endpoint for Phase 0."""

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from core.config import get_api_port
from core.database import init_db

app = FastAPI(title="LLooM v2 API", version="2.0.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

@app.on_event("startup")
async def startup():
    init_db()

@app.get("/api/health")
async def health():
    return {"status": "ok", "version": "2.0.0"}

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=get_api_port())
