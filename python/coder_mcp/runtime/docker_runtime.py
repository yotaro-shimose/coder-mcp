import asyncio
import logging
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Self, override

import docker
import docker.errors

from coder_mcp.runtime import Runtime
from coder_mcp.utils import chmod_recursive

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

    _mcp_name = "Docker MCP Server"

    def __init__(
        self,
        workspace_dir: Optional[str | Path] = None,
        image_name: str = "coder-mcp",
        container_name: Optional[str] = None,
        host_port: Optional[int] = None,
        env_vars: Optional[Dict[str, str]] = None,
        volumes: Optional[Dict[str, str]] = None,
        port_mappings: Optional[List[str]] = None,
    ):
        """Initialize DockerRuntime.

        Args:
            workspace_dir: Host directory to mount as /workspace in container (optional).
                          If provided, all file operations will persist here.
            image_name: Docker image to run (default: coder-mcp)
            container_name: Optional custom container name
            host_port: Optional fixed host port (otherwise dynamically assigned)
            env_vars: Additional environment variables for the container
            volumes: Additional volume mounts {host_path: container_path}
            port_mappings: Additional port mappings
        """
        # Resolve workspace path and ensure it exists if provided
        if workspace_dir:
            self.workspace_dir = Path(workspace_dir).resolve()
            self.workspace_dir.mkdir(parents=True, exist_ok=True)
        else:
            self.workspace_dir = None

        self.image_name = image_name
        self.container_name = container_name or f"mcp-server-{uuid.uuid4().hex[:8]}"
        self.host_port = host_port
        self.env_vars = env_vars or {}

        # Auto-mount workspace_dir to /workspace if provided, plus any additional volumes
        self.volumes = {}
        if self.workspace_dir:
            self.volumes[str(self.workspace_dir)] = "/workspace"

        if volumes:
            for host_path, container_path in volumes.items():
                self.volumes[str(Path(host_path).resolve())] = container_path

        self.port_mappings = port_mappings or []
        self._container = None
        self.client = docker.from_env()

    @override
    async def __aenter__(self) -> Self:
        # 0. Ensure workspace_dir is world-writable for the container user (recursive) if provided
        if self.workspace_dir:
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

        volumes = {}
        if self.workspace_dir:
            volumes[str(self.workspace_dir)] = {"bind": "/workspace", "mode": "rw"}

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
    def get_api_url(self) -> str:
        return f"http://localhost:{self.host_port}"

    async def _wait_for_health(self, timeout: float = 60.0):
        """Wait for health, with Docker-specific error logging on failure."""
        try:
            await super()._wait_for_health(timeout)
        except RuntimeError as e:
            # If it timed out, try to get logs for debugging.
            if self._container:
                loop = asyncio.get_running_loop()
                logs = await loop.run_in_executor(None, self._container.logs)
                logger.error(
                    f"❌ Server failed to become healthy. Logs:\n{logs.decode('utf-8')}"
                )
            raise e
