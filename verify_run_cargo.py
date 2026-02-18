import asyncio
import logging
from coder_mcp.runtime.rust_env import RustCodingEnvironment

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


async def verify():
    # Use a temporary workspace or just the current directory if safe,
    # but RustCodingEnvironment usually mounts a workspace.
    # We can pass None for workspace_dir as per previous conversations/code?
    # actually the code says: if workspace_dir: ... else: self.workspace_dir = None
    # effectively meaning no workspace.
    # But for run_cargo to work, we probably need a valid rust project if we were running a specific project.
    # However, `run_cargo` just runs `cargo run`. If there is no Cargo.toml, it will fail.
    # The default Docker image likely has a default project or we need to init one.
    # The `RustCodingEnvironment` seems to expect a workspace with a project.

    # Let's use a dummy workspace or rely on the fact that `str_replace`
    # assumes `src/main.rs`.

    # Let's create a temporary directory with a valid cargo project for testing
    import tempfile
    import os
    import subprocess

    with tempfile.TemporaryDirectory() as tmpdir:
        # Create a new cargo project
        subprocess.run(["cargo", "init", "--bin", "."], cwd=tmpdir, check=True)

        async with RustCodingEnvironment(
            workspace_dir=tmpdir,
            image_name="coder-mcp-numrs2",
            port_mappings=[
                "8000:8000"
            ],  # ensure we map ports if needed, but host_port is random usually
        ) as rust_env:
            logger.info("Testing successful execution...")
            output, success = await rust_env.run_cargo()
            logger.info(f"Output: {output[:50]}...")
            logger.info(f"Success: {success}")

            if success:
                print("✅ Successful execution test passed")
            else:
                print("❌ Successful execution test failed")

            logger.info("Testing failed execution...")
            # Modify code to be invalid
            await rust_env.str_replace("fn main() {", "fn main() { invalid_syntax")

            output, success = await rust_env.run_cargo()
            logger.info(f"Output: {output[:50]}...")
            logger.info(f"Success: {success}")

            if not success:
                print("✅ Failed execution test passed")
            else:
                print("❌ Failed execution test failed (expected success=False)")


if __name__ == "__main__":
    asyncio.run(verify())
