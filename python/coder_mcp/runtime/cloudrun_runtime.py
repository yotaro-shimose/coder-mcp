import asyncio
import logging
import os
import time
import uuid
from typing import Any, Self, override

from google.api_core.exceptions import GoogleAPICallError, NotFound
from google.cloud import run_v2

from coder_mcp.runtime.runtime import CoderMCPRuntimeError, Runtime

logger = logging.getLogger(__name__)


_TOKEN_TTL_SECONDS = 55 * 60  # 55 minutes
_cached_token: str | None = None
_cached_token_time: float = 0.0


async def fetch_id_token() -> str:
    """gcloud CLI の ADC を使って ID トークン (OIDC) を取得する。

    55 分間キャッシュし、期限切れ時に自動で再取得する。
    2回までリトライを試みる。
    google.oauth2.id_token.fetch_id_token はサービスアカウントキーか
    メタデータサーバーが必要で、ローカルのユーザー認証情報では動かない。
    そのため gcloud auth print-identity-token を使う。
    """
    global _cached_token, _cached_token_time
    if _cached_token and (time.monotonic() - _cached_token_time) < _TOKEN_TTL_SECONDS:
        return _cached_token

    max_attempts = 3
    last_exception = None

    for attempt in range(1, max_attempts + 1):
        try:
            proc = await asyncio.create_subprocess_exec(
                "gcloud",
                "auth",
                "print-identity-token",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, stderr = await proc.communicate()
            if proc.returncode != 0:
                raise RuntimeError(
                    f"gcloud auth print-identity-token failed: {stderr.decode().strip()}"
                )
            token = stdout.decode().strip()
            if not token:
                raise RuntimeError("gcloud auth print-identity-token returned empty")

            _cached_token = token
            _cached_token_time = time.monotonic()
            return token
        except Exception as e:
            last_exception = e
            if attempt < max_attempts:
                logger.warning(
                    f"Error fetching ID token (attempt {attempt}/{max_attempts}): {e}. "
                    "Retrying in 1s..."
                )
                await asyncio.sleep(1)

    raise RuntimeError(
        f"Failed to fetch ID token after {max_attempts} attempts: {last_exception}"
    )


class CloudRunRuntime(Runtime):
    """Context manager for running the MCP server on Google Cloud Run.

    This deploys a Cloud Run service and routes requests to its auto-assigned URL.
    Useful for remote, stateful sandboxes that persist for the lifetime of the agent.

    Configuration can be passed via constructor args or environment variables:
        CLOUDRUN_IMAGE_URI   - Container image URI (Artifact Registry)
        CLOUDRUN_PROJECT_ID  - GCP project ID
        CLOUDRUN_REGION      - GCP region (default: europe-north1)
    """

    _mcp_name = "CloudRun MCP Server"

    def __init__(
        self,
        image_uri: str | None = None,
        project_id: str | None = None,
        region: str | None = None,
        env_vars: dict[str, str] | None = None,
    ):
        self.image_uri = image_uri or os.environ.get("CLOUDRUN_IMAGE_URI", "")
        self.project_id = project_id or os.environ.get("CLOUDRUN_PROJECT_ID", "")
        self.region = region or os.environ.get("CLOUDRUN_REGION", "europe-north1")
        self.env_vars = env_vars or {}
        self.service_name = f"coder-mcp-numrs2-{uuid.uuid4().hex[:8]}"

        if not self.image_uri:
            raise ValueError(
                "image_uri is required. Set CLOUDRUN_IMAGE_URI env var or pass it explicitly."
            )
        if not self.project_id:
            raise ValueError(
                "project_id is required. Set CLOUDRUN_PROJECT_ID env var or pass it explicitly."
            )

        self.service_url: str | None = None
        self._id_token: str | None = None
        self._client = run_v2.ServicesClient()

    @property
    def _parent(self) -> str:
        return f"projects/{self.project_id}/locations/{self.region}"

    @property
    def _service_full_name(self) -> str:
        return f"{self._parent}/services/{self.service_name}"

    async def _deploy_service(self) -> run_v2.Service:
        """Create or update the Cloud Run service. Returns the deployed Service."""
        loop = asyncio.get_running_loop()

        container_env = [
            run_v2.EnvVar(name=k, value=v) for k, v in self.env_vars.items()
        ]

        service = run_v2.Service(
            labels={"managed-by": "coder-mcp"},
            template=run_v2.RevisionTemplate(
                containers=[
                    run_v2.Container(
                        image=self.image_uri,
                        ports=[run_v2.ContainerPort(container_port=3000)],
                        resources=run_v2.ResourceRequirements(
                            limits={"cpu": "4.0", "memory": "8Gi"}, cpu_idle=True
                        ),
                        env=container_env,
                    ),
                ],
                max_instance_request_concurrency=1,
                scaling=run_v2.RevisionScaling(max_instance_count=1),
            ),
        )

        def _create_or_update() -> Any:
            try:
                self._client.get_service(name=self._service_full_name)
                # Service exists — update
                service.name = self._service_full_name
                return self._client.update_service(service=service)
            except NotFound:
                # Create new service
                return self._client.create_service(
                    parent=self._parent,
                    service=service,
                    service_id=self.service_name,
                )

        operation = await loop.run_in_executor(None, _create_or_update)

        # Wait for LRO to complete
        def _wait() -> run_v2.Service:
            return operation.result(timeout=300)

        result = await loop.run_in_executor(None, _wait)
        if result is None:
            raise RuntimeError("Cloud Run deploy returned None")
        return result

    async def _fetch_id_token(self) -> str:
        """Fetch ID token via gcloud CLI (cached for 55 min)."""
        return await fetch_id_token()

    @override
    async def __aenter__(self) -> Self:
        logger.debug(
            f"🚀 Initializing CloudRunRuntime in {self.region} "
            f"(project: {self.project_id})..."
        )

        # 1. Deploy service
        logger.debug("📦 Deploying Cloud Run service...")
        try:
            deployed = await self._deploy_service()
        except GoogleAPICallError as e:
            raise CoderMCPRuntimeError(
                f"Failed to deploy Cloud Run service: {e}"
            ) from e

        self.service_url = deployed.uri
        if not self.service_url:
            raise RuntimeError("Cloud Run service deployed but no URL was assigned.")

        logger.debug(
            f"✅ Cloud Run service deployed: {self.service_url} "
            f"(revision: {deployed.latest_ready_revision})"
        )

        # 2. Get ID token for authenticated requests
        logger.debug("🔑 Fetching ID token...")
        self._id_token = await self._fetch_id_token()

        # 3. Wait for health endpoint (base class handles auth via get_headers)
        await self._wait_for_health()

        return self

    @override
    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        if self.service_url:
            logger.debug(f"🛑 Deleting Cloud Run service {self.service_name}...")
            loop = asyncio.get_running_loop()
            try:

                def _delete():
                    operation = self._client.delete_service(
                        name=self._service_full_name
                    )
                    operation.result(timeout=120)

                await loop.run_in_executor(None, _delete)
                logger.debug("✅ Cloud Run service deleted.")
            except Exception as e:
                logger.warning(f"Error deleting Cloud Run service: {e}")
            self.service_url = None
            self._id_token = None

    @override
    def get_api_url(self) -> str:
        if not self.service_url:
            raise RuntimeError("Service not deployed (no URL).")
        return self.service_url

    @override
    def get_headers(self) -> dict[str, str]:
        headers: dict[str, str] = {}
        if self._id_token:
            headers["Authorization"] = f"Bearer {self._id_token}"
        return headers
