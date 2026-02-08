import logging
import asyncio
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Dict, Optional


logger = logging.getLogger(__name__)


class TempWorkspace:
    def __init__(
        self,
        template_dir: Path | None = None,
        injections: Optional[Dict[Path, str]] = None,
        prefix: str = "workspace_",
        copy_method: str = "copy",
    ):
        """
        Args:
            template_dir: The source directory to copy as the base template.
            injections: A dictionary mapping source paths (absolute) to destination paths RELATIVE to the workspace root.
            prefix: Prefix for the temporary directory.
            copy_method: Method to copy the template. "copy" (default) or "clone".
        """
        self.template_dir = template_dir
        self.injections = injections or {}
        self.prefix = prefix
        self.copy_method = copy_method
        self.temp_dir: Optional[Path] = None
        self._temp_dir_ctx: Optional[tempfile.TemporaryDirectory] = None

    def _setup(self) -> Path:
        # 1. Create Temp Directory using TemporaryDirectory
        self._temp_dir_ctx = tempfile.TemporaryDirectory(prefix=self.prefix)
        self.temp_dir = Path(self._temp_dir_ctx.name)

        # 2. Copy Template or Clone
        if self.template_dir is not None and self.template_dir.exists():
            if self.copy_method == "clone":
                # Use git clone to preserve origin
                try:
                    subprocess.run(
                        ["git", "clone", str(self.template_dir), str(self.temp_dir)],
                        check=True,
                        capture_output=True,
                    )
                except subprocess.CalledProcessError as e:
                    # Fallback to copy if clone fails (e.g. not a git repo)
                    logger.warning(f"Git clone failed: {e}. Falling back to copy.")

                    # Clean up any partial clone artifacts
                    for item in self.temp_dir.iterdir():
                        if item.is_dir():
                            shutil.rmtree(item)
                        else:
                            item.unlink()

                    shutil.copytree(
                        self.template_dir, self.temp_dir, dirs_exist_ok=True
                    )
            else:
                shutil.copytree(self.template_dir, self.temp_dir, dirs_exist_ok=True)

        # 3. Injections
        for src, dest_rel in self.injections.items():
            dest = self.temp_dir / dest_rel

            # Clean up destination if it exists (overwrite)
            if dest.exists():
                if dest.is_dir():
                    shutil.rmtree(dest)
                else:
                    dest.unlink()

            dest.parent.mkdir(parents=True, exist_ok=True)

            if src.is_dir():
                shutil.copytree(src, dest, dirs_exist_ok=True)
            else:
                shutil.copy2(src, dest)

        # 4. Git Init (Auto-init if not present)
        # This is often needed for GitRepository to work
        if not (self.temp_dir / ".git").exists():
            subprocess.run(
                ["git", "init"], cwd=str(self.temp_dir), check=True, capture_output=True
            )
            # Configure git to avoid errors
            subprocess.run(
                ["git", "config", "user.email", "bot@example.com"],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )
            subprocess.run(
                ["git", "config", "user.name", "Bot"],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )

            # Initial commit to have a HEAD
            subprocess.run(
                ["git", "add", "."],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )
            subprocess.run(
                ["git", "commit", "-m", "Initial commit from TempWorkspace"],
                cwd=str(self.temp_dir),
                check=False,
                capture_output=True,
            )

        # 5. CHMOD 777 for Docker
        # Essential for Docker containers to read/write mapped volumes without permission issues
        subprocess.run(["chmod", "-R", "777", str(self.temp_dir)], check=False)

        return self.temp_dir

    def _cleanup(self):
        if self._temp_dir_ctx:
            try:
                self._temp_dir_ctx.cleanup()
            except Exception:
                if self.temp_dir and self.temp_dir.exists():
                    try:
                        shutil.rmtree(self.temp_dir, ignore_errors=True)
                    except Exception as e_:
                        logger.warning(
                            f"Failed to cleanup temp directory {self.temp_dir}: {e_}"
                        )
            finally:
                self._temp_dir_ctx = None
                self.temp_dir = None

    def __enter__(self) -> Path:
        return self._setup()

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._cleanup()

    async def __aenter__(self) -> Path:
        return await asyncio.to_thread(self._setup)

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await asyncio.to_thread(self._cleanup)
