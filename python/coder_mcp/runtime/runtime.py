"""Runtime abstraction for OpenHands agent.

Provides base class and implementations for different execution environments.
"""

import logging
from abc import ABC, abstractmethod
from typing import Any, Dict, Self

import httpx
from agents.mcp import MCPServerStreamableHttp
from pydantic import BaseModel, ValidationError

from coder_mcp.types import CoderToolName

logger = logging.getLogger(__name__)


class CommandOutput(BaseModel):
    output: str
    exit_code: int | None = None


class CoderMCPRuntimeError(Exception):
    """An error that occurred while interacting with the runtime REST API."""

    pass


class Runtime(ABC):
    """Base class for runtime environments.

    A Runtime provides an MCP server connection for the agent to use.
    Implementations handle setup/teardown of the execution environment
    and must provide ``get_api_url()`` and ``get_headers()``.
    """

    # ------------------------------------------------------------------
    # Subclass-overridable MCP configuration
    # ------------------------------------------------------------------
    _mcp_name: str = "MCP Server"
    _mcp_timeout: int = 30
    _mcp_cache_tools: bool = True
    _mcp_session_timeout: int | None = 300
    _shared_client: httpx.AsyncClient | None = None

    # ------------------------------------------------------------------
    # Lifecycle (subclass responsibility)
    # ------------------------------------------------------------------

    @abstractmethod
    async def __aenter__(self) -> Self:
        """Enter runtime context and return self."""
        pass

    @abstractmethod
    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Exit runtime context and cleanup."""
        pass

    # ------------------------------------------------------------------
    # Abstract: subclass provides URL / auth
    # ------------------------------------------------------------------

    @classmethod
    def get_client(cls) -> httpx.AsyncClient:
        if cls._shared_client is None or cls._shared_client.is_closed:
            # 最初に呼ばれた時だけ作成
            cls._shared_client = httpx.AsyncClient(
                limits=httpx.Limits(max_connections=100), timeout=10.0
            )
        return cls._shared_client

    @abstractmethod
    def get_api_url(self) -> str:
        """Get the base URL for the runtime API."""
        pass

    def get_headers(self) -> dict[str, str]:
        """Get extra HTTP headers required by this runtime (e.g. auth tokens)."""
        return {}

    # ------------------------------------------------------------------
    # MCP server creation (concrete, using get_api_url + get_headers)
    # ------------------------------------------------------------------

    def coder_mcp(
        self,
        allowed_tool_names: list[CoderToolName] | None = None,
        blocked_tool_names: list[CoderToolName] | None = None,
    ) -> MCPServerStreamableHttp:
        mcp_url = f"{self.get_api_url()}/mcp"
        tool_filter: dict[str, Any] = {}
        if allowed_tool_names:
            tool_filter["allowed_tool_names"] = allowed_tool_names
        if blocked_tool_names:
            tool_filter["blocked_tool_names"] = blocked_tool_names

        headers = self.get_headers()
        params: dict[str, Any] = {"url": mcp_url}
        if self._mcp_timeout:
            params["timeout"] = self._mcp_timeout
        if headers:
            params["headers"] = headers

        kwargs: dict[str, Any] = {
            "name": self._mcp_name,
            "params": params,
            "tool_filter": tool_filter,  # type: ignore
            "cache_tools_list": self._mcp_cache_tools,
        }
        if self._mcp_session_timeout is not None:
            kwargs["client_session_timeout_seconds"] = self._mcp_session_timeout

        return MCPServerStreamableHttp(**kwargs)

    def coder_mcp_readonly(self) -> MCPServerStreamableHttp:
        return self.coder_mcp(
            allowed_tool_names=[
                "view_file",
                "list_directory",
                "search_filenames",
                "search_content",
            ]
        )

    # ------------------------------------------------------------------
    # REST API helpers (concrete)
    # ------------------------------------------------------------------

    async def run_cargo(self) -> tuple[str, bool]:
        """Runs ``cargo run`` in the container via REST API.

        Returns:
            A tuple of (output, success), where success is True if exit code is 0.
        """
        url = f"{self.get_api_url()}/run"
        try:
            response = await self.get_client().post(
                url, json={}, headers=self.get_headers(), timeout=300.0
            )
        except httpx.RequestError as e:
            raise CoderMCPRuntimeError(f"HTTP request failed: {e}") from e
        if response.status_code != 200:
            raise CoderMCPRuntimeError(
                f"cargo run failed ({response.status_code}): {response.text}"
            )
        try:
            data = CommandOutput.model_validate(response.json())
        except (ValueError, ValidationError) as e:
            raise CoderMCPRuntimeError(
                f"cargo run returned invalid format ({response.status_code}): {response.text}"
            ) from e

        success = data.exit_code == 0
        return data.output, success

    async def str_replace(self, old_str: str, new_str: str) -> tuple[str, bool]:
        """Performs string replacement on src/main.rs via REST API.

        Returns:
            A tuple of (output, success), where success is True if exit code is 0.
        """
        url = f"{self.get_api_url()}/str_replace"
        try:
            response = await self.get_client().post(
                url,
                json={"old_str": old_str, "new_str": new_str},
                headers=self.get_headers(),
                timeout=10.0,
            )
        except httpx.RequestError as e:
            raise CoderMCPRuntimeError(f"HTTP request failed: {e}") from e

        try:
            data = CommandOutput.model_validate(response.json())
        except ValidationError as e:
            raise CoderMCPRuntimeError(
                f"str_replace returned invalid format ({response.status_code}): {response.text}"
            ) from e

        if data.exit_code is not None:
            success = data.exit_code == 0
            return data.output, success

        if response.status_code != 200:
            raise CoderMCPRuntimeError(
                f"str_replace failed ({response.status_code}): {response.text}"
            )

        return data.output, True

    async def view_file(
        self, path: str, start_line: int | None = None, end_line: int | None = None
    ) -> str:
        """Reads file content via REST API."""
        url = f"{self.get_api_url()}/view_file"
        payload: Dict[str, str | int] = {"path": path}
        if start_line is not None:
            payload["start_line"] = start_line
        if end_line is not None:
            payload["end_line"] = end_line

        try:
            response = await self.get_client().post(
                url, json=payload, headers=self.get_headers(), timeout=10.0
            )
        except httpx.RequestError as e:
            raise CoderMCPRuntimeError(f"HTTP request failed: {e}") from e
        if response.status_code != 200:
            raise CoderMCPRuntimeError(
                f"view_file failed ({response.status_code}): {response.text}"
            )
        try:
            data = CommandOutput.model_validate(response.json())
            return data.output
        except ValidationError as e:
            raise CoderMCPRuntimeError(
                f"view_file returned invalid format ({response.status_code}): {response.text}"
            ) from e

    async def set_content(self, path: str, content: str) -> str:
        """Sets file content via REST API."""
        url = f"{self.get_api_url()}/set_content"
        payload = {"path": path, "content": content}

        try:
            response = await self.get_client().post(
                url, json=payload, headers=self.get_headers(), timeout=10.0
            )
        except httpx.RequestError as e:
            raise CoderMCPRuntimeError(f"HTTP request failed: {e}") from e
        if response.status_code != 200:
            raise CoderMCPRuntimeError(
                f"set_content failed ({response.status_code}): {response.text}"
            )
        try:
            data = CommandOutput.model_validate(response.json())
            return data.output
        except ValidationError as e:
            raise CoderMCPRuntimeError(
                f"set_content returned invalid format ({response.status_code}): {response.text}"
            ) from e

    async def tree(
        self,
        path: str = ".",
        exclude: list[str] | None = None,
        truncate: int = 10,
    ) -> str:
        """Get the tree structure of the workspace via REST API."""
        params: dict[str, str] = {"path": path, "truncate": str(truncate)}
        if exclude:
            params["exclude"] = ",".join(exclude)

        url = f"{self.get_api_url()}/tree"
        headers = self.get_headers()
        client = self.get_client()
        try:
            response = await client.get(
                url, params=params, headers=headers, timeout=10.0
            )
            response.raise_for_status()
            return response.text
        except Exception as e:
            return f"Error fetching tree structure: {e}"

    # ------------------------------------------------------------------
    # Health check (supports auth via get_headers)
    # ------------------------------------------------------------------

    async def _wait_for_health(self, timeout: float = 30.0):
        """Wait for the server to respond to health checks at the given URL."""
        health_url = f"{self.get_api_url()}/health"
        logger.debug(f"⏳ Waiting for server at {health_url} to become healthy...")
        headers = self.get_headers()
        client = self.get_client()
        try:
            response = await client.get(health_url, headers=headers, timeout=timeout)
            if response.status_code == 200:
                logger.info("✅ Server is healthy!")
                return
            else:
                raise CoderMCPRuntimeError(
                    f"Server at health_url failed to become healthy in {timeout}s. {response.status_code}: {response.text}"
                )
        except httpx.TimeoutException as e:
            raise CoderMCPRuntimeError(
                f"Server at health_url failed to become healthy in {timeout}s. {e}"
            )
        except Exception as e:
            logger.error(
                f"Unexpected error to acccess to {health_url}: {type(e)}, {str(e)}"
            )
            raise RuntimeError(f"Unexpected error: {e}") from e
