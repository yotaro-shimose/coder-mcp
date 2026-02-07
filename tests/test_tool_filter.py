import asyncio
import logging
import sys
from coder_mcp.runtime.local_runtime import LocalRuntime

# Setup logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


async def test_tool_filter():
    print("--- Starting Tool Filter Test ---")
    async with LocalRuntime(workdir=".") as runtime:
        # Test 1: Bash only
        print("\n[Test 1] Connecting with allowed_tool_names=['bash']...")
        async with runtime.coder_mcp(allowed_tool_names=["bash"]) as client:
            tools = await client.list_tools()
            # tools is a list of Tool objects
            tool_names = [t.name for t in tools]
            print(f"Available tools: {tool_names}")

            if "bash" not in tool_names:
                print("FAIL: 'bash' not found in tools")
                sys.exit(1)

            if "view_file" in tool_names:
                print("FAIL: 'view_file' should NOT be in tools")
                sys.exit(1)

            # Try calling bash
            print("Calling 'bash' tool...")
            try:
                result = await client.call_tool(
                    "bash", {"command": "echo 'Hello Bash'"}
                )
                print(f"Result: {result}")
            except Exception as e:
                print(f"FAIL: Failed to call bash: {e}")
                sys.exit(1)

        # Test 2: Read-only defaults
        print("\n[Test 2] Connecting with coder_mcp_readonly()...")
        async with runtime.coder_mcp_readonly() as client:
            tools = await client.list_tools()
            tool_names = [t.name for t in tools]
            print(f"Available tools: {tool_names}")

            allowed_readonly = [
                "view_file",
                "list_directory",
                "search_filenames",
                "search_content",
            ]
            for t in tool_names:
                if t not in allowed_readonly:
                    print(f"FAIL: Tool '{t}' should NOT be available in read-only mode")
                    sys.exit(1)

            if "bash" in tool_names:
                print("FAIL: 'bash' should NOT be in read-only tools")
                sys.exit(1)

            # Try calling view_file (should work)
            print("Calling 'list_directory'...")
            try:
                await client.call_tool("list_directory", {"path": "."})
                print("Success calling 'list_directory'")
            except Exception as e:
                print(f"FAIL: Failed to call list_directory: {e}")
                sys.exit(1)

    print("\n--- All Tests Passed ---")


if __name__ == "__main__":
    asyncio.run(test_tool_filter())
