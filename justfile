default:
  just --list

# Build the docker image
build-image:
    docker build -t coder-mcp -f docker/Dockerfile .

build-image-numrs2:
    docker build -t coder-mcp-numrs2 -f docker/Dockerfile.numrs2 .