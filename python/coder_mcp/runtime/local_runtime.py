from typing import override
from typing import Self
import socket
from typing import Any

from agents.mcp import MCPServerStreamableHttp

from coder_mcp import CServer
from coder_mcp.runtime.runtime import Runtime
from coder_mcp.types import CoderToolName


class LocalRuntime(Runtime):
    """Runtime that connects to a local MCP server.

    Use this when you have an MCP server already running locally.

    Example:
        # Start server: cd coder-mcp && cargo run
        async with LocalRuntime(workdir=".") as runtime:
            async with runtime.coder_mcp() as server:
                # use server
                pass
    """

    def __init__(
        self,
        workdir: str,
        port: int | None = None,
    ):
        """Initialize LocalRuntime.

        Args:
            workdir: Path to workspace directory
            port: Port to listen on. If None, a free port will be chosen.
        """
        self.workdir = workdir
        self.port = port
        self.url = ""
        self._server: CServer | None = None

    def _find_free_port(self) -> int:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.bind(("", 0))
            return s.getsockname()[1]

    @override
    async def __aenter__(self) -> Self:
        """Start local MCP server."""
        if self.port is None:
            self.port = self._find_free_port()

        self.url = f"http://localhost:{self.port}/mcp"
        self._server = CServer()
        await self._server.start(self.workdir, self.port)
        await self._wait_for_health(f"http://localhost:{self.port}/health")
        return self

    @override
    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Stop local MCP server."""
        if self._server:
            await self._server.stop()

    @override
    def coder_mcp(
        self,
        allowed_tool_names: list[CoderToolName] | None = None,
        blocked_tool_names: list[CoderToolName] | None = None,
    ) -> MCPServerStreamableHttp:
        tool_filter = {}
        if allowed_tool_names:
            tool_filter["allowed_tool_names"] = allowed_tool_names
        if blocked_tool_names:
            tool_filter["blocked_tool_names"] = blocked_tool_names

        return MCPServerStreamableHttp(
            name="Local MCP Server",
            params={
                "url": self.url,
            },
            tool_filter=tool_filter,  # type: ignore
            cache_tools_list=False,
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
