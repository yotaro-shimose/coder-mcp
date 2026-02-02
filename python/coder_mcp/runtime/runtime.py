"""Runtime abstraction for OpenHands agent.

Provides base class and implementations for different execution environments.
"""

import logging
import asyncio
import time
from urllib.request import urlopen
from typing import Self
from abc import ABC, abstractmethod
from typing import Any

from agents.mcp import MCPServerStreamableHttp


logger = logging.getLogger(__name__)


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
    def coder_mcp(self) -> MCPServerStreamableHttp: ...
    @abstractmethod
    def coder_mcp_readonly(self) -> MCPServerStreamableHttp: ...

    async def _wait_for_health(self, url: str, timeout: float = 30.0):
        """Wait for the server to respond to health checks at the given URL."""
        logger.debug(f"⏳ Waiting for server at {url} to become healthy...")
        start_time = time.time()

        while time.time() - start_time < timeout:
            try:
                loop = asyncio.get_running_loop()

                def check():
                    with urlopen(url, timeout=1) as response:
                        return response.getcode() == 200

                if await loop.run_in_executor(None, check):
                    logger.debug("✅ Server is healthy!")
                    return
            except Exception:
                pass
            await asyncio.sleep(1)

        raise RuntimeError(f"Server at {url} failed to become healthy in {timeout}s.")
