default:
  just --list

# Build the docker image
build-image:
    docker build -t coder-mcp -f docker/Dockerfile .

build-image-numrs2:
    docker build -t coder-mcp-numrs2 -f docker/Dockerfile.numrs2 .

# Push docker image to AWS ECR
# Usage: just push-ecr [repo_name="coder-mcp-numrs2"]
push-ecr repo_name="coder-mcp-numrs2":
    #!/usr/bin/env bash
    set -euo pipefail
    
    REGION="eu-north-1"
    LOCAL_IMAGE="coder-mcp-numrs2:latest"
    REPO_NAME="{{repo_name}}"
    
    echo "🔍 AWS Account IDを取得しています..."
    AWS_ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
    ECR_URI_BASE="${AWS_ACCOUNT_ID}.dkr.ecr.${REGION}.amazonaws.com"
    FULL_IMAGE_URI="${ECR_URI_BASE}/${REPO_NAME}:latest"
    
    echo "📦 ECRリポジトリ '${REPO_NAME}' を確認/作成しています..."
    aws ecr describe-repositories --repository-names "${REPO_NAME}" --region "${REGION}" >/dev/null 2>&1 || \
      aws ecr create-repository --repository-name "${REPO_NAME}" --region "${REGION}"
    
    echo "🔑 ECRにログインしています..."
    aws ecr get-login-password --region "${REGION}" | docker login --username AWS --password-stdin "${ECR_URI_BASE}"
    
    echo "🏷️ イメージにタグを付けています: ${LOCAL_IMAGE} -> ${FULL_IMAGE_URI}"
    docker tag "${LOCAL_IMAGE}" "${FULL_IMAGE_URI}"
    
    echo "🚀 ECRへイメージをプッシュしています..."
    docker push "${FULL_IMAGE_URI}"
    
    echo "⚡ SOCIインデックスを作成・プッシュしています (Fargate起動高速化用)..."
    soci create --namespace moby "${FULL_IMAGE_URI}"
    soci push --namespace moby "${FULL_IMAGE_URI}"
    
    echo "✅ プッシュおよびSOCIインデックス登録完了: ${FULL_IMAGE_URI}"

# Push docker image to Google Artifact Registry
# Usage: just push-gar [image_uri="europe-north1-docker.pkg.dev/dsat2-405406/shimose-repo/coder-mcp-numrs2"]
push-gar image_uri="europe-north1-docker.pkg.dev/dsat2-405406/shimose-repo/coder-mcp-numrs2":
    #!/usr/bin/env bash
    set -euo pipefail

    LOCAL_IMAGE="coder-mcp-numrs2:latest"
    FULL_IMAGE_URI="{{image_uri}}:latest"
    REGISTRY_HOST="$(echo "{{image_uri}}" | cut -d/ -f1)"

    echo "🔑 Artifact Registry (${REGISTRY_HOST}) 向けに docker credential helper を設定しています..."
    gcloud auth configure-docker "${REGISTRY_HOST}" --quiet

    echo "🏷️ イメージにタグを付けています: ${LOCAL_IMAGE} -> ${FULL_IMAGE_URI}"
    docker tag "${LOCAL_IMAGE}" "${FULL_IMAGE_URI}"

    echo "🚀 Artifact Registry へイメージをプッシュしています..."
    docker push "${FULL_IMAGE_URI}"

    echo "✅ プッシュ完了: ${FULL_IMAGE_URI}"

# Delete Cloud Run services that contain a specific pattern in their name
# Usage: just delete-cloudrun-services [pattern="coder-mcp-numrs2"] [region="europe-north1"]
delete-cloudrun-services pattern="coder-mcp-numrs2" region="europe-north1":
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "🔍 リージョン '{{region}}' で '{{pattern}}' を含むCloud Runサービスを検索しています..."
    SERVICES=$(gcloud run services list --region="{{region}}" --filter="metadata.name ~ {{pattern}}" --format="value(metadata.name)")
    
    if [ -z "$SERVICES" ]; then
        echo "✨ 削除対象のサービスは見つかりませんでした。"
        exit 0
    fi
    
    echo "🗑️ 以下のサービスを削除します:"
    echo "$SERVICES"
    
    echo "$SERVICES" | xargs -P 32 -I {} gcloud run services delete {} --region="{{region}}" --quiet
    
    echo "✅ 削除完了しました。"