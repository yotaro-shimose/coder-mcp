"""Runtime abstraction for OpenHands agent.

Provides base class and implementations for different execution environments.
"""

from typing import Self
from abc import ABC, abstractmethod
from typing import Any

from agents.mcp import MCPServerStreamableHttp


class Runtime(ABC):
    """Base class for runtime environments.

    A Runtime provides an MCP server connection for the agent to use.
    Implementations handle setup/teardown of the execution environment.
    """

    @abstractmethod
    async def __aenter__(self) -> Self:
        """Enter runtime context and return self."""
        pass

    @abstractmethod
    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Exit runtime context and cleanup."""
        pass

    @abstractmethod
    @abstractmethod
    def coder_mcp(self) -> MCPServerStreamableHttp: ...
    @abstractmethod
    def coder_mcp_readonly(self) -> MCPServerStreamableHttp: ...
