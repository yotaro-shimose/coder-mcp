import logging
from typing import Self
from pathlib import Path
from typing import Dict, Optional, List
from coder_mcp.runtime.docker_runtime import DockerRuntime


logger = logging.getLogger(__name__)


class RustCodingEnvironment(DockerRuntime):
    """A specialized Docker runtime for Rust development with sccache support.

    This environment automatically configures sccache to use a persistent host directory
    and mounts a workspace directory for project files.
    """

    def __init__(
        self,
        image_name: str = "coder-mcp",
        cache_dir: str | Path = "./.sccache",
        cargo_cache_dir: str | Path = "./.cargo_cache",
        workspace_dir: str | Path = "./workspace",
        container_name: Optional[str] = None,
        host_port: Optional[int] = None,
        env_vars: Optional[Dict[str, str]] = None,
        volumes: Optional[Dict[str, str]] = None,
        port_mappings: Optional[List[str]] = None,
    ):
        self.image_name = image_name
        self.cache_dir = Path(cache_dir).resolve()
        self.cargo_cache_dir = Path(cargo_cache_dir).resolve()
        self.workspace_dir = Path(workspace_dir).resolve()

        # Merge environment variables for Rust caching
        env = env_vars or {}
        env.setdefault("RUSTC_WRAPPER", "/usr/local/bin/sccache")
        env.setdefault("SCCACHE_DIR", "/var/cache/sccache")
        env.setdefault("CARGO_INCREMENTAL", "0")

        # Merge volume mounts
        vols = volumes or {}
        vols[str(self.cache_dir)] = "/var/cache/sccache"
        vols[str(self.cargo_cache_dir / "registry")] = "/usr/local/cargo/registry"
        vols[str(self.cargo_cache_dir / "git")] = "/usr/local/cargo/git"
        vols[str(self.workspace_dir)] = "/workspace"

        super().__init__(
            workspace_dir=str(self.workspace_dir),
            image_name=image_name,
            container_name=container_name,
            host_port=host_port,
            env_vars=env,
            volumes=vols,
            port_mappings=port_mappings,
        )

    async def __aenter__(self) -> Self:
        """Starts the Rust coding environment."""
        logger.debug("🦀 Initializing Rust Coding Environment...")

        # Ensure cache directories exist with proper permissions
        # The Docker container runs as non-root user, so we need 777 permissions
        for dir_path in [
            self.cache_dir,
            self.cargo_cache_dir,
            self.cargo_cache_dir / "registry",
            self.cargo_cache_dir / "git",
        ]:
            dir_path.mkdir(parents=True, exist_ok=True)
            dir_path.chmod(0o777)

        return await super().__aenter__()
