import logging
from pathlib import Path
from typing import Dict, List, Optional, Self

import httpx
from pydantic import BaseModel, ValidationError

from coder_mcp.runtime.docker_runtime import DockerRuntime

logger = logging.getLogger(__name__)


class CommandOutput(BaseModel):
    output: str
    exit_code: Optional[int] = None


class RustEnvError(Exception):
    """An error that occurred while interacting with the Rust coding environment."""

    pass


class RustCodingEnvironment(DockerRuntime):
    """A specialized Docker runtime for Rust development with sccache support.

    This environment automatically configures sccache to use a persistent host directory
    and mounts a workspace directory for project files.
    """

    def __init__(
        self,
        image_name: str = "coder-mcp",
        cache_dir: str | Path = "./.sccache",
        cargo_cache_dir: str | Path = "./.cargo_cache",
        workspace_dir: Optional[str | Path] = None,
        container_name: Optional[str] = None,
        host_port: Optional[int] = None,
        env_vars: Optional[Dict[str, str]] = None,
        volumes: Optional[Dict[str, str]] = None,
        port_mappings: Optional[List[str]] = None,
    ):
        self.image_name = image_name
        self.cache_dir = Path(cache_dir).resolve()
        self.cargo_cache_dir = Path(cargo_cache_dir).resolve()
        if workspace_dir:
            self.workspace_dir = Path(workspace_dir).resolve()
        else:
            self.workspace_dir = None

        # Merge environment variables for Rust caching
        env = env_vars or {}
        env.setdefault("RUSTC_WRAPPER", "/usr/local/bin/sccache")
        env.setdefault("SCCACHE_DIR", "/var/cache/sccache")
        env.setdefault("CARGO_INCREMENTAL", "0")

        # Merge volume mounts
        vols = volumes or {}
        vols[str(self.cache_dir)] = "/var/cache/sccache"
        vols[str(self.cargo_cache_dir / "registry")] = "/usr/local/cargo/registry"
        vols[str(self.cargo_cache_dir / "git")] = "/usr/local/cargo/git"
        if self.workspace_dir:
            vols[str(self.workspace_dir)] = "/workspace"

        super().__init__(
            workspace_dir=self.workspace_dir,
            image_name=image_name,
            container_name=container_name,
            host_port=host_port,
            env_vars=env,
            volumes=vols,
            port_mappings=port_mappings,
        )

    async def __aenter__(self) -> Self:
        """Starts the Rust coding environment."""
        logger.debug("🦀 Initializing Rust Coding Environment...")

        # Ensure cache directories exist with proper permissions
        # The Docker container runs as non-root user, so we need 777 permissions
        for dir_path in [
            self.cache_dir,
            self.cargo_cache_dir,
            self.cargo_cache_dir / "registry",
            self.cargo_cache_dir / "git",
        ]:
            dir_path.mkdir(parents=True, exist_ok=True)
            dir_path.chmod(0o777)

        return await super().__aenter__()

    async def run_cargo(self) -> tuple[str, bool]:
        """Runs `cargo run` in the container via REST API.

        Returns:
            A tuple of (output, success), where success is True if exit code is 0.
        """
        url = f"http://localhost:{self.host_port}/run"
        async with httpx.AsyncClient() as client:
            try:
                response = await client.post(
                    url, json={}, timeout=300.0
                )  # Long timeout for compilation
            except httpx.RequestError as e:
                raise RustEnvError(f"HTTP request failed: {e}") from e
            if response.status_code != 200:
                raise RustEnvError(
                    f"cargo run failed ({response.status_code}): {response.text}"
                )
            try:
                data = CommandOutput.model_validate(response.json())
            except (ValueError, ValidationError) as e:
                raise RustEnvError(
                    f"cargo run returned invalid format ({response.status_code}): {response.text}"
                ) from e

            success = data.exit_code == 0
            return data.output, success

    async def str_replace(self, old_str: str, new_str: str) -> tuple[str, bool]:
        """Performs string replacement on src/main.rs via REST API.

        Returns:
            A tuple of (output, success), where success is True if exit code is 0.
        """
        url = f"http://localhost:{self.host_port}/str_replace"
        async with httpx.AsyncClient() as client:
            try:
                response = await client.post(
                    url, json={"old_str": old_str, "new_str": new_str}, timeout=10.0
                )
            except httpx.RequestError as e:
                raise RustEnvError(f"HTTP request failed: {e}") from e

            try:
                data = CommandOutput.model_validate(response.json())
            except ValidationError as e:
                raise RustEnvError(
                    f"str_replace returned invalid format ({response.status_code}): {response.text}"
                ) from e

            if data.exit_code is not None:
                success = data.exit_code == 0
                return data.output, success

            if response.status_code != 200:
                raise RustEnvError(
                    f"str_replace failed ({response.status_code}): {response.text}"
                )

            return data.output, True

    async def view_file(
        self, path: str, start_line: int | None = None, end_line: int | None = None
    ) -> str:
        """Reads file content via REST API."""
        url = f"http://localhost:{self.host_port}/view_file"
        payload: Dict[str, str | int] = {"path": path}
        if start_line is not None:
            payload["start_line"] = start_line
        if end_line is not None:
            payload["end_line"] = end_line

        async with httpx.AsyncClient() as client:
            try:
                response = await client.post(url, json=payload, timeout=10.0)
            except httpx.RequestError as e:
                raise RustEnvError(f"HTTP request failed: {e}") from e
            if response.status_code != 200:
                raise RustEnvError(
                    f"view_file failed ({response.status_code}): {response.text}"
                )
            try:
                data = CommandOutput.model_validate(response.json())
                return data.output
            except ValidationError as e:
                raise RustEnvError(
                    f"view_file returned invalid format ({response.status_code}): {response.text}"
                ) from e

    async def set_content(self, path: str, content: str) -> str:
        """Sets file content via REST API."""
        url = f"http://localhost:{self.host_port}/set_content"
        payload = {"path": path, "content": content}

        async with httpx.AsyncClient() as client:
            try:
                response = await client.post(url, json=payload, timeout=10.0)
            except httpx.RequestError as e:
                raise RustEnvError(f"HTTP request failed: {e}") from e
            if response.status_code != 200:
                raise RustEnvError(
                    f"set_content failed ({response.status_code}): {response.text}"
                )
            try:
                data = CommandOutput.model_validate(response.json())
                return data.output
            except ValidationError as e:
                raise RustEnvError(
                    f"set_content returned invalid format ({response.status_code}): {response.text}"
                ) from e
