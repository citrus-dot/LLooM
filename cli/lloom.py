"""LLooM CLI entry point."""

import click


@click.group()
def cli():
    """LLooM v2 — Intelligent LLM Routing Platform"""
    pass


@cli.command()
def init():
    """Interactive initialization wizard."""
    click.echo("LLooM v2 init wizard (TODO — Phase 6)")


@cli.command()
def serve():
    """Start the API server."""
    import uvicorn
    from core.config import get_api_port
    from core.database import init_db

    init_db()
    click.echo(f"Starting LLooM API on port {get_api_port()}...")
    uvicorn.run("api.server:app", host="0.0.0.0", port=get_api_port(), reload=True)


@cli.group()
def model():
    """Model management."""
    pass


@model.command(name="list")
def model_list():
    """List registered models."""
    click.echo("Model list (TODO — Phase 1)")


if __name__ == "__main__":
    cli()
