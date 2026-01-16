import asyncio
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Self, override

from agents.mcp import MCPServerStreamableHttp

from coder_mcp.runtime import Runtime
from coder_mcp.utils import chmod_recursive


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
        self._container_id: Optional[str] = None

    @override
    async def __aenter__(self) -> Self:
        # 0. Ensure workspace_dir is world-writable for the container user (recursive)
        chmod_recursive(self.workspace_dir)

        # 1. Verify image exists
        proc = await asyncio.create_subprocess_exec(
            "docker",
            "inspect",
            "--type=image",
            self.image_name,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        await proc.communicate()
        if proc.returncode != 0:
            raise RuntimeError(
                f"Docker image '{self.image_name}' not found. Please build it first."
            )

        # 2. Prepare docker run command
        cmd = [
            "docker",
            "run",
            "-d",
            "--name",
            self.container_name,
            "--rm",
        ]

        # Add port mapping
        if self.host_port:
            cmd.extend(["-p", f"{self.host_port}:3000"])
        else:
            # Use -P to publish all exposed ports with random host ports
            cmd.append("-P")

        # Add environment variables
        for k, v in self.env_vars.items():
            cmd.extend(["-e", f"{k}={v}"])

        # Add volumes
        for host_path, container_path in self.volumes.items():
            cmd.extend(["-v", f"{host_path}:{container_path}"])

        # Add extra port mappings
        for mapping in self.port_mappings:
            cmd.extend(["-p", mapping])

        cmd.append(self.image_name)

        # 3. Start container
        print(f"🐳 Running: {' '.join(cmd)}")
        proc = await asyncio.create_subprocess_exec(
            *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            print(f"❌ Container creation failed: {stderr.decode()}")
            raise RuntimeError(f"Failed to start Docker container: {stderr.decode()}")

        self._container_id = stdout.decode().strip()
        print(f"✅ Container created successfully (ID: {self._container_id[:12]})")

        # If host_port was not specified, find what Docker assigned
        if not self.host_port:
            # Give Docker a moment to set up port mappings
            await asyncio.sleep(0.5)

            proc = await asyncio.create_subprocess_exec(
                "docker",
                "port",
                self.container_name,
                "3000",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, stderr = await proc.communicate()
            if proc.returncode != 0:
                raise RuntimeError(
                    f"Failed to get assigned port from Docker.\n"
                    f"stderr: {stderr.decode()}\n"
                    f"stdout: {stdout.decode()}"
                )
            # stdout is something like "0.0.0.0:49483\n:::49483"
            port_output = stdout.decode()
            for line in port_output.splitlines():
                if ":" in line:
                    self.host_port = int(line.split(":")[-1])
                    break
            if not self.host_port:
                raise RuntimeError(
                    f"Could not determine assigned port from Docker.\n"
                    f"Port output: {port_output}"
                )

        print(
            f"🚀 Started Docker container '{self.container_name}' on port {self.host_port}."
        )

        # 4. Wait for healthy
        await self._wait_for_health()
        return self

    @override
    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if self._container_id:
            print(
                f"🛑 Stopping and removing Docker container '{self.container_name}'..."
            )
            proc = await asyncio.create_subprocess_exec(
                "docker",
                "stop",
                self.container_name,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            await proc.communicate()
            self._container_id = None
            print("👋 Container stopped.")

    @override
    def coder_mcp(self) -> MCPServerStreamableHttp:
        mcp_url = f"http://localhost:{self.host_port}/mcp"
        return MCPServerStreamableHttp(
            name="Docker MCP Server",
            params={
                "url": mcp_url,
                "timeout": 15,
            },
            cache_tools_list=True,
            # Allow long-running commands (e.g., cargo build, rustup) up to 5 minutes
            client_session_timeout_seconds=300,
        )

    @override
    def coder_mcp_readonly(self) -> MCPServerStreamableHttp:
        mcp_url = f"http://localhost:{self.host_port}/mcp-readonly"
        return MCPServerStreamableHttp(
            name="Docker MCP Server (Read Only)",
            params={
                "url": mcp_url,
                "timeout": 15,
            },
            cache_tools_list=True,
            # Allow long-running commands (e.g., cargo build, rustup) up to 5 minutes
            client_session_timeout_seconds=300,
        )

    async def _wait_for_health(self, timeout: float = 30.0):
        """Wait for the server to respond to health checks."""
        health_url = f"http://localhost:{self.host_port}/health"
        try:
            await super()._wait_for_health(health_url, timeout)
        except RuntimeError as e:
            # If it timed out, try to get logs for debugging.
            proc = await asyncio.create_subprocess_exec(
                "docker",
                "logs",
                self.container_name,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, _ = await proc.communicate()
            print(f"❌ Server failed to become healthy. Logs:\n{stdout.decode()}")
            raise e
