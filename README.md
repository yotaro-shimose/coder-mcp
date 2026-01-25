# Coder MCP

Coder MCP is a Model Context Protocol (MCP) server that provides tools for filesystem manipulation and bash command execution. It is designed to be used by AI agents to perform coding tasks.

## Runtimes

The `coder_mcp` Python package provides two runtimes for hosting the MCP server: `LocalRuntime` and `DockerRuntime`.

### LocalRuntime

`LocalRuntime` runs the MCP server directly on the host machine. This is useful for local development or when you want the agent to have direct access to your filesystem.

**Typical Usage:**

```python
import asyncio
from coder_mcp.runtime import LocalRuntime

async def main():
    # workdir: The directory where the MCP server will operate
    async with LocalRuntime(workdir="./workspace") as runtime:
        # Get the MCP server client
        async with runtime.coder_mcp() as server:
            # Now you can use the server with your agent
            pass
            
        # Or get a read-only version of the server
        async with runtime.coder_mcp_readonly() as readonly_server:
            pass

if __name__ == "__main__":
    asyncio.run(main())
```

### DockerRuntime

`DockerRuntime` runs the MCP server inside a Docker container. This is useful for sandboxing the agent's environment, ensuring that file operations and commands are isolated from the host system.

**Typical Usage:**

```python
import asyncio
from coder_mcp.runtime import DockerRuntime

async def main():
    # workspace_dir: Host directory to mount as /workspace in the container
    # image_name: The Docker image to use (default: "coder-mcp")
    async with DockerRuntime(
        workspace_dir="/path/to/host/workspace", 
        image_name="coder-mcp"
    ) as runtime:
        # Get the MCP server client
        async with runtime.coder_mcp() as server:
            # The server runs inside the container
            # File operations in `server` will affect /workspace inside the container
            # (which is mounted from /path/to/host/workspace)
            pass

if __name__ == "__main__":
    asyncio.run(main())
```

**Requirements:**
- Docker must be installed and running.
- The `coder-mcp` image (or your custom image) must be built and available.

