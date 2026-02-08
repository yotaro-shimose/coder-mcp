import logging
import asyncio
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Self, override

from agents.mcp import MCPServerStreamableHttp

from coder_mcp.runtime import Runtime
from coder_mcp.utils import chmod_recursive
from coder_mcp.types import CoderToolName

import docker
import docker.errors
import httpx

logger = logging.getLogger(__name__)


class DockerRuntime(Runtime):
    """Context manager for running the MCP server inside a Docker container.

    The workspace_dir is required to ensure files persist after the container stops.
    It will be mounted to /workspace inside the container.

    Example:
        async with DockerRuntime(
            workspace_dir="/path/to/my/project",
            image_name="coder-mcp"
        ) as runtime:
            async with OpenHandsAgent(runtime=runtime) as agent:
                result = await agent.run("Create hello.py")
                # Files will be saved to /path/to/my/project
    """

    def __init__(
        self,
        workspace_dir: str | Path,
        image_name: str = "coder-mcp",
        container_name: Optional[str] = None,
        host_port: Optional[int] = None,
        env_vars: Optional[Dict[str, str]] = None,
        volumes: Optional[Dict[str, str]] = None,
        port_mappings: Optional[List[str]] = None,
    ):
        """Initialize DockerRuntime.

        Args:
            workspace_dir: Host directory to mount as /workspace in container (required).
                          All file operations go here and persist after container stops.
            image_name: Docker image to run (default: coder-mcp)
            container_name: Optional custom container name
            host_port: Optional fixed host port (otherwise dynamically assigned)
            env_vars: Additional environment variables for the container
            volumes: Additional volume mounts {host_path: container_path}
            port_mappings: Additional port mappings
        """
        # Resolve workspace path and ensure it exists
        self.workspace_dir = Path(workspace_dir).resolve()
        self.workspace_dir.mkdir(parents=True, exist_ok=True)

        self.image_name = image_name
        self.container_name = container_name or f"mcp-server-{uuid.uuid4().hex[:8]}"
        self.host_port = host_port
        self.env_vars = env_vars or {}

        # Auto-mount workspace_dir to /workspace, plus any additional volumes
        self.volumes = {str(self.workspace_dir): "/workspace"}
        if volumes:
            for host_path, container_path in volumes.items():
                self.volumes[str(Path(host_path).resolve())] = container_path

        self.port_mappings = port_mappings or []
        self._container = None
        self.client = docker.from_env()

    @override
    async def __aenter__(self) -> Self:
        # 0. Ensure workspace_dir is world-writable for the container user (recursive)
        chmod_recursive(self.workspace_dir)

        # 1. Verify image exists
        loop = asyncio.get_running_loop()
        try:
            await loop.run_in_executor(None, self.client.images.get, self.image_name)
        except docker.errors.ImageNotFound:
            raise RuntimeError(
                f"Docker image '{self.image_name}' not found. Please build it first."
            )

        # 2. Prepare docker run arguments
        ports = {}
        if self.host_port:
            ports["3000/tcp"] = self.host_port
        else:
            # Publish all exposed ports to random host ports
            ports["3000/tcp"] = None

        # Add extra port mappings
        for mapping in self.port_mappings:
            # mapping is like "8080:80" or "80"
            if ":" in mapping:
                host, container = mapping.split(":")
                ports[f"{container}/tcp"] = host
            else:
                ports[f"{mapping}/tcp"] = None

        volumes = {str(self.workspace_dir): {"bind": "/workspace", "mode": "rw"}}
        for host_path, container_path in self.volumes.items():
            if container_path == "/workspace":
                continue
            volumes[str(Path(host_path).resolve())] = {
                "bind": container_path,
                "mode": "rw",
            }

        # 3. Start container
        logger.debug(
            f"🐳 Starting container '{self.container_name}' with image '{self.image_name}'"
        )

        try:
            self._container = await loop.run_in_executor(
                None,
                lambda: self.client.containers.run(
                    self.image_name,
                    detach=True,
                    name=self.container_name,
                    remove=True,
                    ports=ports,
                    environment=self.env_vars,
                    volumes=volumes,
                ),
            )
        except docker.errors.APIError as e:
            logger.error(f"❌ Container creation failed: {e}")
            raise RuntimeError(f"Failed to start Docker container: {e}")

        logger.debug(
            f"✅ Container created successfully (ID: {self._container.short_id})"
        )

        # If host_port was not specified, find what Docker assigned
        # If host_port was not specified, find what Docker assigned
        if not self.host_port:
            # Give Docker a moment to set up port mappings
            await asyncio.sleep(0.5)

            # Reload container attributes to get the assigned ports
            await loop.run_in_executor(None, self._container.reload)

            # Ports structure: {'3000/tcp': [{'HostIp': '0.0.0.0', 'HostPort': '32768'}], ...}
            ports = self._container.attrs["NetworkSettings"]["Ports"]
            if "3000/tcp" in ports and ports["3000/tcp"]:
                self.host_port = int(ports["3000/tcp"][0]["HostPort"])

            if not self.host_port:
                raise RuntimeError(
                    f"Could not determine assigned port from Docker.\nPorts: {ports}"
                )

        logger.debug(
            f"🚀 Started Docker container '{self.container_name}' on port {self.host_port}."
        )

        # 4. Wait for healthy
        await self._wait_for_health()
        return self

    @override
    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if self._container:
            logger.debug(
                f"🛑 Stopping and removing Docker container '{self.container_name}'..."
            )
            loop = asyncio.get_running_loop()
            try:
                await loop.run_in_executor(None, self._container.stop)
                # We used remove=True in run(), so it should auto-remove
            except Exception as e:
                logger.warning(f"Error stopping container: {e}")

            self._container = None
            logger.debug("👋 Container stopped.")

    @override
    def coder_mcp(
        self,
        allowed_tool_names: list[CoderToolName] | None = None,
        blocked_tool_names: list[CoderToolName] | None = None,
    ) -> MCPServerStreamableHttp:
        mcp_url = f"http://localhost:{self.host_port}/mcp"
        tool_filter = {}
        if allowed_tool_names:
            tool_filter["allowed_tool_names"] = allowed_tool_names
        if blocked_tool_names:
            tool_filter["blocked_tool_names"] = blocked_tool_names

        return MCPServerStreamableHttp(
            name="Docker MCP Server",
            params={
                "url": mcp_url,
                "timeout": 15,
            },
            tool_filter=tool_filter,  # type: ignore
            cache_tools_list=True,
            # Allow long-running commands (e.g., cargo build, rustup) up to 5 minutes
            client_session_timeout_seconds=300,
        )

    @override
    def coder_mcp_readonly(self) -> MCPServerStreamableHttp:
        return self.coder_mcp(
            allowed_tool_names=[
                "view_file",
                "list_directory",
                "search_filenames",
                "search_content",
            ]
        )

    @override
    async def tree(
        self,
        path: str = ".",
        exclude: list[str] | None = None,
        truncate: int = 10,
    ) -> str:

        params = {"path": path, "truncate": str(truncate)}
        if exclude:
            params["exclude"] = ",".join(exclude)

        url = f"http://localhost:{self.host_port}/tree"

        async with httpx.AsyncClient() as client:
            try:
                response = await client.get(url, params=params, timeout=5.0)
                response.raise_for_status()
                return response.text
            except Exception as e:
                return f"Error fetching tree structure: {e}"

    async def _wait_for_health(self, url: str | None = None, timeout: float = 30.0):
        """Wait for the server to respond to health checks."""
        if url is None:
            url = f"http://localhost:{self.host_port}/health"
        try:
            await super()._wait_for_health(url, timeout)
        except RuntimeError as e:
            # If it timed out, try to get logs for debugging.
            if self._container:
                loop = asyncio.get_running_loop()
                logs = await loop.run_in_executor(None, self._container.logs)
                logger.error(
                    f"❌ Server failed to become healthy. Logs:\n{logs.decode('utf-8')}"
                )
            raise e
